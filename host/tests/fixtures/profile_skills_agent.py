"""ACP sintética: barreiras HTTP nos pedidos reais, sem ler arquivos de skills."""
import json
import sys
import urllib.request

pending_prompt = None
pending_responses = 0
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method is None and request.get("id") in ("candidate-read", "candidate-refusal"):
        report = urllib.request.Request(sys.argv[1], json.dumps({
            "method": request["id"] + "-response", "result": request.get("result"), "error": request.get("error")
        }).encode(), {"Content-Type": "application/json"})
        with urllib.request.urlopen(report, timeout=20) as response:
            response.read()
        if pending_prompt is not None:
            pending_responses -= 1
        if pending_prompt is not None and pending_responses == 0:
            print(json.dumps({"jsonrpc":"2.0", "id":pending_prompt, "result":{"stopReason":"end_turn"}}), flush=True)
            pending_prompt = None
        continue
    if "id" not in request or method is None:
        continue
    result = {}
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {
            "loadSession": True, "sessionCapabilities": {} if "--inline" in sys.argv else {"systemPrompt": {}}
        }}
    elif method in ("session/new", "session/load", "session/prompt"):
        report = urllib.request.Request(sys.argv[1], json.dumps(request).encode(),
                                        {"Content-Type": "application/json"})
        with urllib.request.urlopen(report, timeout=20) as response:
            action = json.loads(response.read())
        if method == "session/prompt" and action and action.get("holdCompletion"):
            pending_prompt = request["id"]
            pending_responses = action["responsesBeforeCompletion"]
            continue
        if method == "session/prompt" and action and action.get("readSkill"):
            print(json.dumps({"jsonrpc": "2.0", "id": "candidate-permission", "method": "session/request_permission",
                "params": {"sessionId": "skills-session", "toolCall": {"toolCallId": "candidate-tool",
                    "title": "Permissão do focal", "kind": "other"},
                    "options": [{"optionId": "allow", "name": "Permitir", "kind": "allow_once"}]}}), flush=True)
            print(json.dumps({"jsonrpc": "2.0", "id": "candidate-read", "method": "_mexerica/skills/read",
                "params": {"sessionId": "skills-session", "skillId": action["readSkill"],
                    "preparationId": request["params"]["_meta"]["mexericaSkillsReadCandidate"]["preparationId"]}}), flush=True)
            for phase in ("candidate-blocked", "candidate-completed"):
                report = urllib.request.Request(sys.argv[1], json.dumps({"method": phase}).encode(),
                    {"Content-Type": "application/json"})
                with urllib.request.urlopen(report, timeout=20) as response:
                    response.read()
                if phase == "candidate-blocked" and action.get("complete"):
                    print(json.dumps({"jsonrpc":"2.0", "id":request["id"], "result":{"stopReason":"end_turn"}}), flush=True)
            if not action.get("complete"):
                pending_prompt = request["id"]
                pending_responses = 1
            continue
        result = {"stopReason": "end_turn"} if method == "session/prompt" else {"sessionId": "skills-session"}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
