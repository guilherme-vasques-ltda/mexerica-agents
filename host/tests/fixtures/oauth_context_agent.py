"""ACP sintético: relata mcpServers ao endpoint local e conclui sem chamar MCP."""
import json
import sys
import urllib.request
import uuid


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def report(value):
    body = json.dumps(value).encode()
    request = urllib.request.Request(sys.argv[1], body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=10) as response:
        response.read()


gated = "--gateway-lifecycle" in sys.argv[2:]
session = None
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    identifier = request.get("id")
    if method is None or identifier is None:
        continue
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}
    elif method in ("session/new", "session/load"):
        session = request["params"].get("sessionId", str(uuid.uuid4()))
        servers = request["params"]["mcpServers"]
        report({"method": method, "servers": servers} if gated else servers)
        result = {"sessionId": session}
    elif method == "session/prompt":
        if gated:
            report({"method": method, "sessionId": session})
        send({"method": "session/update", "params": {"sessionId": session, "update": {
            "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Fixture concluída."}
        }}})
        result = {"stopReason": "end_turn"}
    else:
        result = {}
    send({"id": identifier, "result": result})
