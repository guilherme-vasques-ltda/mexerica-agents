"""ACP Codex-like: standalone/correlated MCP permission fallbacks, without a real model."""
import json
import sys
import time
from pathlib import Path

permission_serial = 0
current_session = "s1"
claude = "--claude" in sys.argv
# O arquivo vazio também comprova ausência de respostas recebidas.
with open("permission-responses.jsonl", "a"):
    pass


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def update(value):
    send({"method": "session/update", "params": {"sessionId": current_session, "update": value}})


def receive(line):
    request = json.loads(line)
    if "method" not in request and str(request.get("id", "")).startswith("tool-"):
        with open("permission-responses.jsonl", "a") as log:
            log.write(json.dumps(request) + "\n")
    return request


def permission(server, index, question=False, variant=None, wrong_session=False):
    global permission_serial
    call = f"tool-{permission_serial}-{index}"
    permission_serial += 1
    tool = {"toolCallId": call, "kind": "execute"}
    if index % 2:
        name = "remember" if "MEMORY_EXPOSURE" in current_text else "test"
        raw = {"server": server, "tool": name, "arguments": {}}
        if "UNKNOWN_ORIGIN" in current_text:
            raw["serverName"] = "external"
        event = {"sessionUpdate": "tool_call", **tool, "title": f"mcp.{server}.{name}", "rawInput": raw}
        if not claude:
            event["_meta"] = {"is_mcp_tool_call": True}
        update(event)
    else:
        tool["rawInput"] = {"serverName": server, "schema": {"type": "object", "properties": {}}}
    if claude:
        tool = {"toolCallId": call, "name": "TodoWrite", "kind": "think", "rawInput": {"todos": []}}
    params = {"sessionId": "sessao-antiga" if wrong_session else current_session, "toolCall": tool,
              "options": [{"optionId": "deny", "kind": "reject_once", "name": "Negar"},
                          {"optionId": "allow", "kind": "allow_once", "name": "Permitir"}]}
    if variant:
        session = "allow_session" if variant == "MCP" else "allow_for_session"
        params["options"].append({"optionId": session, "kind": "allow_always", "name": "Allow for session"})
        persistent = {
            "COMMAND": "accept_execpolicy_amendment",
            "NETWORK": "apply_network_policy_amendment:0",
            "MCP": "allow_always",
        }[variant]
        params["options"].append({"optionId": persistent, "kind": "allow_always", "name": "Always allow"})
        if variant == "NETWORK":
            params["options"].append({"optionId": "apply_network_policy_amendment:1",
                                      "kind": "reject_always", "name": "Never"})
    if not question:
        params["_meta"] = {"is_mcp_tool_approval": True}
    send({"id": call, "method": "session/request_permission", "params": params})
    for line in sys.stdin:
        response = receive(line)
        if response.get("id") == call:
            return response.get("result", {}).get("outcome", {}).get("optionId", "cancelled")
    return "closed"


for line in sys.stdin:
    request = receive(line)
    method = request.get("method")
    params = request.get("params", {})
    if method == "initialize":
        result = {"protocolVersion": 1, "agentInfo": {"name": "codex-acp", "version": "2.2.2"},
                  "agentCapabilities": {"_meta": {"claudeCode": {}}} if claude else {}}
    elif method == "session/new":
        counter = Path("session-count")
        count = int(counter.read_text()) + 1 if counter.exists() else 1
        counter.write_text(str(count))
        current_session = f"s{count}"
        result = {"sessionId": current_session, "configOptions": [
            {"id": "mode", "category": "mode", "currentValue": "default" if claude else "workspace-write"}]}
    elif method == "session/prompt":
        current_session = params["sessionId"]
        text = params["prompt"][0]["text"]
        current_text = text
        if "MEMORY_EXPOSURE" in text:
            Path("memory-clean").touch()
            deadline = time.monotonic() + 30
            while not Path("memory-expose").exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            update({"sessionUpdate": "tool_call", "toolCallId": "external-source", "kind": "read",
                    "title": "Agenda externa", "rawInput": {"server": "external", "tool": "read"},
                    "_meta": {"is_mcp_tool_call": True}})
        question = "QUESTION" in text
        # O Hub envolve a mensagem do usuário no contexto do turno.
        variant = next((value for value in ("COMMAND", "NETWORK", "MCP") if f"OPTIONS_{value}" in text), None)
        server_case = next((value.lower() for value in ("TEAM", "ROUTINES", "CONNECTORS", "CHAT", "MEMORY", "MANAGE", "COMPUTERS") if value in text), None)
        servers = ["external"] if "EXTERNAL" in text or variant else (
            [server_case] if server_case else (
                ["memory"] if question else ["chat", "memory", "manage", "computers"]))
        outcomes = [permission(server, 1 if "CORRELATED" in text else index, question or variant in ("COMMAND", "NETWORK"), variant, "WRONG_SESSION" in text)
                    for index, server in enumerate(servers)] if "NOOP" not in text else ["pronto"]
        if "MEMORY_EXPOSURE" in text:
            Path("memory-ready").touch()
            deadline = time.monotonic() + 30
            while not Path("memory-continue").exists() and time.monotonic() < deadline:
                time.sleep(0.02)
        if "WITNESS" in text and "allow" in outcomes:
            with open("efeito-perigoso", "w") as witness:
                witness.write("executado")
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": ",".join(outcomes)}})
        result = {"stopReason": "end_turn"}
    else:
        result = {}
    if "id" in request:
        send({"id": request["id"], "result": result})
