"""Fixture ACP da memória automática: ferramentas MCP reais, conclusão controlada, sem provedor.

MEMORY_AUTO {"operations":[["remember",{"text":"..."}]],"outcome":"success"}
registra propostas e só então devolve end_turn, erro ou cancelamento. --external produz
uma exposição externa antes da proposta. Nenhum executor de fundo é simulado aqui.
"""
import json
import os
import pathlib
import subprocess
import sys
import time
import uuid

servers = []
session = ""


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def update(value):
    send({"method": "session/update", "params": {"sessionId": session, "update": value}})


def memory_call(name, arguments):
    server = next(s for s in servers if s["name"] == "memory")
    env = {**os.environ, **{item["name"]: item["value"] for item in server.get("env", [])}}
    with subprocess.Popen([server["command"], *server["args"]], env=env, text=True,
                          stdin=subprocess.PIPE, stdout=subprocess.PIPE) as process:
        def rpc(identifier, method, params):
            process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": identifier,
                                           "method": method, "params": params}) + "\n")
            process.stdin.flush()
            return json.loads(process.stdout.readline())["result"]
        rpc(1, "initialize", {"protocolVersion": "2025-06-18"})
        listed = rpc(2, "tools/list", {})
        pathlib.Path("memory-tools.json").write_text(json.dumps(listed))
        assert any(tool["name"] == name for tool in listed["tools"])
        result = rpc(3, "tools/call", {"name": name, "arguments": arguments})
        process.stdin.close()
        return result


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    params = request.get("params", {})
    if method == "initialize":
        result = {"agentCapabilities": {"loadSession": True, "sessionCapabilities": {"systemPrompt": True}}}
    elif method in ("session/new", "session/load"):
        session = params.get("sessionId", str(uuid.uuid4()))
        servers = params.get("mcpServers", [])
        result = {"sessionId": session}
    elif method == "session/prompt":
        text = params["prompt"][0]["text"]
        payload = json.loads(text.rsplit("MEMORY_AUTO ", 1)[1])
        if "--external" in sys.argv:
            update({"sessionUpdate": "tool_call_update", "toolCallId": "outside",
                    "status": "completed", "rawInput": {"server": "outside"},
                    "content": [{"type": "text", "text": "Material externo sem autoridade"}]})
        results = []
        for index, (name, arguments) in enumerate(payload["operations"]):
            identifier = f"memory-auto-{index}"
            update({"sessionUpdate": "tool_call", "toolCallId": identifier, "kind": "other",
                    "title": f"mcp.memory.{name}", "status": "in_progress",
                    "rawInput": {"server": "memory", "tool": name, "arguments": arguments}})
            time.sleep(0.1)
            answer = memory_call(name, arguments)
            results.append(answer)
            update({"sessionUpdate": "tool_call_update", "toolCallId": identifier,
                    "status": "completed", "content": [{"type": "content", "content": {
                        "type": "text", "text": answer["content"][0]["text"]}}]})
        pathlib.Path("memory-proposals.json").write_text(json.dumps(results))
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Concluído."}})
        outcome = payload.get("outcome", "success")
        if outcome == "error":
            send({"id": request["id"], "error": {"code": -32000, "message": "Falha controlada da fixture"}})
            continue
        result = {"stopReason": "cancelled" if outcome == "cancelled" else "end_turn"}
    else:
        result = {}
    if "id" in request:
        send({"id": request["id"], "result": result})
