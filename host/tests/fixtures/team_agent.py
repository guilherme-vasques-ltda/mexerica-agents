# Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
"""Deterministic ACP agent for delegation tests; no provider or network access."""

import json
import os
import pathlib
import subprocess
import sys
import threading
import time

output_lock = threading.Lock()
cancel = threading.Event()
permission = threading.Event()
permission_reply = None
permission_ids = set()
servers = []
# Cada session/new ganha um ID próprio (a primeira continua "session"); as notificações usam a do prompt.
sessions_created = int(pathlib.Path("session-count").read_text()) if pathlib.Path("session-count").exists() else 0
current_session = "session"
# As definições MCP de cada sessão: voltar a uma sessão já carregada não as reenvia.
servers_by_session = {}


def send(value):
    with output_lock:
        print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def update(value):
    send({"method": "session/update", "params": {"sessionId": current_session, "update": value}})


def mcp_call(server_name, tool, arguments):
    definitions = servers_by_session.get(current_session, servers)
    server = next(s for s in definitions if s["name"] == server_name)
    # O ambiente da definição vai junto, como faz o motor (inclui o slot da sessão que recebeu o servidor).
    env = {**os.environ, **{e["name"]: e["value"] for e in server.get("env", [])}}
    with subprocess.Popen([server["command"], *server["args"]], stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, text=True, env=env) as mcp:
        def rpc(identifier, method, params):
            mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params}) + "\n")
            mcp.stdin.flush()
            return json.loads(mcp.stdout.readline())

        initialized = rpc(1, "initialize", {"protocolVersion": "2025-06-18"})
        assert initialized["result"]["serverInfo"]["name"] == "codync-" + server_name
        tools = rpc(2, "tools/list", {})
        assert any(t["name"] == tool for t in tools["result"]["tools"])
        if server_name == "team":
            assert {t["name"] for t in tools["result"]["tools"]} == {"list_bots", "ask_bot", "message_bot", "bot_request_status", "my_team_chain"}
            roster = rpc(3, "tools/call", {"name": "list_bots", "arguments": {}})
            bots = json.loads(roster["result"]["content"][0]["text"])["bots"]
            assert any(b["id"] == arguments["botId"] for b in bots)
            assert all("team" in b and isinstance(b["teamChief"], bool) for b in bots)
        response = rpc(4, "tools/call", {"name": tool, "arguments": arguments})
        mcp.stdin.close()
        return response["result"]


def team_call(target, tool, message):
    return mcp_call("team", tool, {"botId": target, "message": message})


def prompt(request):
    global current_session
    current_session = request["params"].get("sessionId", "session")
    text = request["params"]["prompt"][0]["text"]
    with open("prompts.jsonl", "a") as log:
        log.write(json.dumps(text) + "\n")
    # O primeiro prompt também traz as instruções do perfil antes da mensagem.
    message_text = text.split("\n\n")[-1]
    if message_text.startswith(("HUB_FLOW ", "HUB_FLOW_QUIET ")):
        # HUB_FLOW_QUIET não anuncia tool_call: simula o MCP interno do Hub, cuja identidade o adaptador
        # verificado confirma, então a chamada não marca a sessão como ferramenta externa (D125).
        quiet = message_text.startswith("HUB_FLOW_QUIET ")
        steps = json.loads(message_text.removeprefix("HUB_FLOW_QUIET " if quiet else "HUB_FLOW "))
        results = []
        created = ""
        for index, step in enumerate(steps):
            server, tool, arguments = step
            arguments = json.loads(json.dumps(arguments).replace("$created", created))
            identifier = f"hub-flow-{request['id']}-{index}"
            if not quiet:
                update({"sessionUpdate": "tool_call", "toolCallId": identifier,
                        "title": f"mcp.{server}.{tool}", "kind": "other", "status": "in_progress",
                        "_meta": {"claudeCode": {"toolName": f"mcp__{server}__{tool}"}},
                        "rawInput": {"server": server, "tool": tool, "arguments": arguments}})
                # A notificação chega antes da chamada HTTP da ponte, como no adaptador real.
                time.sleep(0.5)
            result = mcp_call(server, tool, arguments)
            results.append(result)
            if not result.get("isError") and tool == "create_bot":
                created = json.loads(result["content"][0]["text"])["bot"]["id"]
            if not quiet:
                update({"sessionUpdate": "tool_call_update", "toolCallId": identifier,
                        "status": "completed", "content": [{"type": "content", "content": {
                            "type": "text", "text": result["content"][0]["text"]}}]})
            if result.get("isError"):
                break
        update({"sessionUpdate": "agent_message_chunk", "content": {
            "type": "text", "text": "Flow: " + json.dumps(results)}})
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})
        return
    if message_text.startswith(("DELEGATE ", "MESSAGE ", "ASK_REPORT ", "MESSAGE_REPORT ")):
        independent = message_text.startswith(("MESSAGE ", "MESSAGE_REPORT "))
        target = message_text.split()[1]
        update({"sessionUpdate": "tool_call", "toolCallId": "delegate", "title": "Ask reviewer", "status": "in_progress"})
        message = "REPORT" if "_REPORT " in message_text else "PERMISSION review the changes"
        if message_text.startswith("DELEGATE "):
            parts = message_text.split(maxsplit=2)
            if len(parts) == 3:
                message = parts[2]
        response = team_call(target, "message_bot" if independent else "ask_bot", message)
        assert not response.get("isError"), response
        result = json.loads(response["content"][0]["text"])
        if independent:
            assert result["status"] == "queued" and "reply" not in result
            text = "Message queued"
        else:
            text = "Team reply: " + result["reply"]
        update({"sessionUpdate": "tool_call_update", "toolCallId": "delegate", "status": "completed"})
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})
        return
    if text.split("\n\n")[-1] == "REPORT":
        response = mcp_call("chat", "send_message", {"text": "Report for the user"})
        if "requests your help" in text:
            assert response.get("isError"), response
            assert "requesting bot" in response["content"][0]["text"]
            assert "Do not use send_message or message_bot" in text
            reply = "Answer for the requesting bot"
        else:
            assert not response.get("isError"), response
            response = mcp_call("chat", "send_message", {"text": "Second report for the user"})
            assert not response.get("isError"), response
            reply = "Private completion trace"
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply}})
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})
        return
    if text.split("\n\n")[-1] == "CHAIN":
        route = json.loads(pathlib.Path("handoff.json").read_text())
        response = team_call(route["target"], route["tool"], "CHAIN")
        if response.get("isError"):
            reply = "Chain stopped: " + response["content"][0]["text"]
        else:
            result = json.loads(response["content"][0]["text"])
            reply = result.get("reply", "Chain forwarded")
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply}})
        # A text update can reach sync before the host handles turn completion.
        # Keep that interval deterministic when the bounded chain stops.
        if response.get("isError"):
            time.sleep(0.3)
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})
        return
    if text.split("\n\n")[-1].startswith("MANAGE "):
        tool, arguments = text.split("\n\n")[-1].split(" ", 2)[1:]
        response = mcp_call("manage", tool, json.loads(arguments))
        reply = ("Refused: " if response.get("isError") else "Done: ") + response["content"][0]["text"]
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply}})
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})
        return
    if text.split("\n\n")[-1] == "OK":
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "(pass)"}})
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})
        return
    if text.split("\n\n")[-1] == "THANKS":
        response = mcp_call("chat", "react", {"emoji": "❤️"})
        reply = "Held back a reaction" if response.get("isError") else "Reacted with a heart"
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply}})
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})
        return
    if "PERMISSION" in text:
        ids = ["permission-1", "permission-2"] if "PERMISSION_PARALLEL" in text else ["permission"]
        permission_ids.update(ids)
        for identifier in ids:
            send({"id": identifier, "method": "session/request_permission", "params": {
                "sessionId": current_session, "toolCall": {"toolCallId": "edit" if identifier == "permission" else identifier, "title": "Edit a file"},
                "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}],
            }})
        while not cancel.is_set() and not permission.wait(0.01):
            pass
    while "BLOCK" in text and not pathlib.Path("release").exists() and not cancel.wait(0.01):
        pass
    if text.startswith("[Channel:") or "\n[Channel:" in text:
        while "BLOCK" in text and not pathlib.Path("release").exists() and not cancel.wait(0.01):
            pass
        if cancel.is_set():
            send({"id": request["id"], "result": {"stopReason": "cancelled"}})
            return
        # A room turn: answer only when the user spoke last, otherwise pass.
        room = text.split("New messages in the room (oldest first):\n", 1)[-1].split("\n\n", 1)[0]
        last = room.strip().splitlines()[-1] if "New messages" in text else ""
        reply = "reply: group" if last.startswith("User:") else "(pass)"
        if "REACT" in last:
            response = mcp_call("chat", "react", {"emoji": "👍", "to": "user"})
            assert not response.get("isError"), response
            reply = "(pass)"
        # Two chunks, apart: the streamed entry is rewritten before it's final.
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply[:3]}})
        time.sleep(0.4)
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply[3:]}})
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})
        return
    if cancel.is_set():
        send({"id": request["id"], "result": {"stopReason": "cancelled"}})
    elif "FAIL" in text:
        send({"id": request["id"], "error": {"code": -32000, "message": "fixture failure"}})
    elif "REFUSE" in text:
        send({"id": request["id"], "result": {"stopReason": "refusal"}})
    else:
        if "EMPTY" not in text:
            update({"sessionUpdate": "agent_message_chunk", "content": {
                "type": "text", "text": "reply: " + text.split("\n\n")[-1],
            }})
        send({"id": request["id"], "result": {"stopReason": "end_turn"}})


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        send({"id": request["id"], "result": {"agentCapabilities": {
            "loadSession": True, "_meta": {"claudeCode": {}},
        }}})
    elif method in ("session/new", "session/load"):
        servers = request["params"]["mcpServers"]
        with open("sessions.jsonl", "a") as log:
            log.write(json.dumps(request) + "\n")
        pathlib.Path("servers.json").write_text(json.dumps(request["params"]["mcpServers"]))
        if method == "session/load":
            session_id = request["params"]["sessionId"]
        else:
            # Nunca repete um ID conhecido, inclusive um carregado depois de reiniciar.
            sessions_created += 1
            pathlib.Path("session-count").write_text(str(sessions_created))
            session_id = "session" if sessions_created == 1 else f"session-{sessions_created}"
            while session_id in servers_by_session:
                sessions_created += 1
                session_id = f"session-{sessions_created}"
        servers_by_session[session_id] = request["params"]["mcpServers"]
        send({"id": request["id"], "result": {"sessionId": session_id}})
    elif method == "session/prompt":
        cancel = threading.Event()
        permission = threading.Event()
        permission_ids.clear()
        threading.Thread(target=prompt, args=(request,), daemon=True).start()
    elif method == "session/cancel":
        cancel.set()
    elif request.get("id") in permission_ids:
        permission_reply = request.get("result")
        permission_ids.discard(request["id"])
        if not permission_ids:
            permission.set()
    elif "id" in request:
        send({"id": request["id"], "result": {}})
