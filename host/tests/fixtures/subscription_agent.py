"""ACP local: primeira tentativa bloqueada pela assinatura, demais concluem."""
import json
import pathlib
import sys
import time


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


session_id = None
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    identifier = request.get("id")
    if identifier is None or method is None:
        continue
    if method == "initialize":
        result = {"protocolVersion": 1, "agentInfo": {"name": "fixture"},
                  "agentCapabilities": {"loadSession": True}}
    elif method == "session/new":
        if pathlib.Path("fail-new").exists():
            pathlib.Path("new-failed").touch()
            send({"id": identifier, "error": {"code": -32603, "message": "Falha transitória ao criar sessão"}})
            continue
        if pathlib.Path("limit-new").exists():
            send({"id": identifier, "error": {"code": -32603, "message": "Internal error", "data": {
                "codex_error_info": "UsageLimitExceeded", "resetsAt": int(time.time()) + 3600
            }}})
            continue
        counter = pathlib.Path("session-count")
        count = int(counter.read_text()) + 1 if counter.exists() else 1
        counter.write_text(str(count))
        session_id = f"session-{count}"
        result = {"sessionId": session_id}
    elif method == "session/load":
        if pathlib.Path("slow-load").exists():
            pathlib.Path("loading").touch()
            time.sleep(1)
        if pathlib.Path("limit-load").exists():
            send({"id": identifier, "error": {"code": -32603, "message": "Internal error", "data": {"codex_error_info": "UsageLimitExceeded"}}})
            continue
        if pathlib.Path("fail-load").exists():
            send({"id": identifier, "error": {"code": -32603, "message": "Session not found"}})
            continue
        session_id = request["params"]["sessionId"]
        result = {"sessionId": session_id}
    elif method == "session/prompt":
        with open("prompts.jsonl", "a") as log:
            log.write(json.dumps(request["params"]) + "\n")
        while pathlib.Path("limit-hold").exists():
            time.sleep(0.02)
        blocked = pathlib.Path("blocked")
        if pathlib.Path("stale-rejection").exists():
            send({"method": "session/update", "params": {"sessionId": session_id, "update": {
                "sessionUpdate": "usage_update", "used": 1, "size": 100,
                "_meta": {"_claude/rateLimit": {"status": "rejected"}}
            }}})
            send({"id": identifier, "error": {"code": -32603, "message": "Unrelated failure"}})
            continue
        if pathlib.Path("pending-cards").exists():
            send({"method": "session/update", "params": {"sessionId": session_id, "update": {
                "sessionUpdate": "tool_call", "toolCallId": "tool-1", "title": "Em andamento", "status": "in_progress"
            }}})
            send({"id": "approval-1", "method": "session/request_permission", "params": {
                "sessionId": session_id, "toolCall": {"title": "Aprovar", "toolCallId": "tool-1"},
                "options": [{"optionId": "allow", "name": "Permitir", "kind": "allow_once"}]
            }})
        if pathlib.Path("allowed-warning").exists():
            for status in ["rejected", "allowed_warning"]:
                send({"method": "session/update", "params": {"sessionId": session_id, "update": {
                    "sessionUpdate": "usage_update", "used": 1, "size": 100,
                    "_meta": {"_claude/rateLimit": {"status": status}}
                }}})
            send({"id": identifier, "error": {"code": -32603, "message": "Unrelated failure"}})
            continue
        if pathlib.Path("tool-stderr").exists():
            print("MCP tool: rate limit exceeded (HTTP 429)", file=sys.stderr, flush=True)
            sys.exit(1)
        if pathlib.Path("successful-rejection").exists():
            send({"method": "session/update", "params": {"sessionId": session_id, "update": {
                "sessionUpdate": "usage_update", "used": 1, "size": 100,
                "_meta": {"_claude/rateLimit": {"status": "rejected", "rateLimitType": "seven_day_opus"}}
            }}})
            blocked.touch()
        if not blocked.exists() or pathlib.Path("always-limit").exists():
            blocked.touch()
            if pathlib.Path("claude").exists():
                send({"method": "session/update", "params": {"sessionId": session_id, "update": {
                    "sessionUpdate": "usage_update", "used": 1, "size": 100,
                    "_meta": {"_claude/rateLimit": {"status": "rejected", "rateLimitType": "five_hour"}}
                }}})
                send({"id": identifier, "error": {"code": -32603, "message": "Internal error", "data": {"errorKind": "rate_limit"}}})
                continue
            data = {"message": "You've hit your usage limit", "codex_error_info": "UsageLimitExceeded"}
            if not pathlib.Path("no-reset").exists():
                data["rate_limits"] = {"primary": {"used_percent": 100, "resets_at": int(time.time()) + 3600}}
            send({"id": identifier, "error": {"code": -32603, "message": "Internal error", "data": data}})
            continue
        if pathlib.Path("hold-queued").exists() and "Pedido seguinte" in json.dumps(request["params"]):
            while pathlib.Path("hold-queued").exists():
                time.sleep(0.02)
        if pathlib.Path("hold").exists():
            while pathlib.Path("hold").exists():
                time.sleep(0.02)
        send({"method": "session/update", "params": {"sessionId": session_id, "update": {
            "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Concluído"}
        }}})
        result = {"stopReason": "end_turn"}
    else:
        result = {}
    send({"id": identifier, "result": result})
