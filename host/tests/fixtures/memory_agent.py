# Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
"""Fixture ACP real para o extrator contido e a nomeação, sem provedor LLM.

Memória reconhece apenas ExtractionInput novo (jobId/messages) e devolve operações
estritas com evidência humana do snapshot. Não traduz PLAN/cursor/dreamlog legado.
Os modos de falha, markers e metadados de conta/modelo/cota continuam no mesmo ACP.
"""
import json
import os
import pathlib
import time
import sys
import uuid

session = ""
system = ""
mode = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("DONNABOT_FIXTURE_BACKGROUND_MODE", "")
paused_background = False
source_wire_workspace = None
source_wire_prompt = "MEMORY_SOURCE_WIRE: confira a origem já registrada."


def pause_new_witness(request):
    """Testemunha de transporte descartável: nunca pausa o ACP da conversa do bot."""
    global paused_background
    params = request.get("params", {})
    cwd = pathlib.Path(params.get("cwd", ""))
    if request.get("method") == "session/new" and cwd.name == "work" and cwd.parent.name.startswith("donnabot-background-"):
        paused_background = True
    if not paused_background:
        return
    trace = pathlib.Path(os.environ["MEXERICA_HOME"]) / "pause-new"
    trace.mkdir(parents=True, exist_ok=True)
    with (trace / "requests.jsonl").open("a") as output:
        output.write(json.dumps(request) + "\n")
    if request.get("method") == "session/new":
        (trace / "ready").touch()
        deadline = time.monotonic() + 40
        while not (trace / "continue").exists():
            if time.monotonic() > deadline:
                raise TimeoutError("session/new da fixture não foi liberado")
            time.sleep(0.02)
    elif request.get("method") == "session/prompt":
        (trace / "prompt").touch()

def extraction_input(blocks):
    for block in blocks:
        if block.get("type") != "text":
            continue
        try:
            value = json.loads(block.get("text", ""))
        except (json.JSONDecodeError, TypeError):
            continue
        if (isinstance(value, dict) and isinstance(value.get("jobId"), str)
                and isinstance(value.get("messages"), list)):
            return value
    return None


def extraction_plan(snapshot):
    human = next((message for message in snapshot["messages"]
                  if message.get("fromUser") is True and isinstance(message.get("id"), str)
                  and message["id"] and "explain things to me concisely in Traditional Chinese"
                  in message.get("text", "")), None)
    if human is None:
        return {"operations": []}
    previous = next((note["file"] for note in snapshot.get("notes", [])
                     if note.get("file", {}).get("name") == "style.md"), None)
    return {"operations": [
        {"action": "remember", "text": "Prefers concise Traditional Chinese explanations",
         "shared": False, "evidence": [human["id"]], "nature": "inferred"},
        {"action": "note", "file": "style.md",
         "content": "---\ndescription: How the user likes answers\n---\n- Wants things explained concisely, in Traditional Chinese\n",
         "shared": False, "expectedHash": previous["hash"] if previous else None,
         "evidence": [human["id"]], "nature": "inferred"},
    ]}


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


for line in sys.stdin:
    request = json.loads(line)
    if mode == "--pause-new":
        pause_new_witness(request)
    method = request.get("method")
    if mode.startswith("--adapter-") and len(sys.argv) > 2:
        with pathlib.Path(sys.argv[2]).open("a") as log:
            log.write(method + "\n")
    params = request.get("params", {})
    if mode == "--hold-source-wire":
        if method in ("session/new", "session/load"):
            source_wire_workspace = pathlib.Path(params["cwd"])
        if source_wire_workspace is not None:
            with (source_wire_workspace / "requests.jsonl").open("a") as output:
                output.write(json.dumps(request) + "\n")
    if method == "initialize":
        result = {"agentInfo": {"name": "@agentclientprotocol/claude-agent-acp", "version": "0.94.0"},
                  "agentCapabilities": {"loadSession": True, "_meta": {"claudeCode": {}}}}
        if mode == "--adapter-version-next":
            result["agentInfo"]["version"] = "0.100.0"
        elif mode == "--adapter-name-mismatch":
            result["agentInfo"]["name"] = "outro-conector"
        elif mode == "--adapter-other":
            del result["agentCapabilities"]["_meta"]
    elif method in ("session/new", "session/load"):
        session = params.get("sessionId", str(uuid.uuid4()))
        system = params.get("_meta", {}).get("systemPrompt", "")
        if not isinstance(system, str):
            system = ""
        if os.environ.get("DONNABOT_FIXTURE_BACKGROUND_MODE") == "mcp-test" and "You name a personal assistant bot" in system:
            log = pathlib.Path(os.environ["MEXERICA_HOME"]) / "background" / "requests.jsonl"
            log.parent.mkdir(parents=True, exist_ok=True)
            with log.open("a") as output:
                output.write(json.dumps(request) + "\n")
        result = {"sessionId": session, "configOptions": [{"id": "model", "category": "model", "type": "select",
                  "currentValue": "haiku", "options": [{"value": "haiku"}]}]}
    elif method == "session/set_config_option":
        result = {"configOptions": [{"id": "model", "category": "model", "type": "select",
                  "currentValue": "haiku", "options": [{"value": "haiku"}]}]}
    elif method == "session/prompt":
        text = system + "\n" + params["prompt"][0]["text"]
        snapshot = extraction_input(params["prompt"])
        if snapshot is not None:
            if mode.startswith("--tool-"):
                event = "tool_call_update" if mode == "--tool-update" else "tool_call"
                send({"method": "session/update", "params": {"sessionId": session, "update": {
                    "sessionUpdate": event, "toolCallId": "external", "kind": "read",
                    "title": "Conteúdo externo", "status": "completed"
                }}})
                if mode == "--tool-error":
                    send({"id": request["id"], "error": {"code": -32000, "message": "falha depois da ferramenta"}})
                    continue
            elif mode in ("--error", "--limit", "--invalid-json", "--missing-stop", "--bad-stop", "--empty-plan", "--naming-missing"):
                if mode == "--error":
                    send({"id": request["id"], "error": {"code": -32000, "message": "falha simulada"}})
                    continue
                if mode == "--limit":
                    send({"id": request["id"], "error": {"code": -32603, "message": "limite", "data": {"errorKind": "rate_limit"}}})
                    continue
            elif mode and pathlib.Path(mode).is_absolute() and not mode.endswith("naming-boundary"):
                marker = pathlib.Path(mode)
                marker.with_suffix(".ready").touch()
                deadline = time.monotonic() + 30
                while not marker.with_suffix(".continue").exists():
                    if time.monotonic() > deadline:
                        raise TimeoutError("dream fixture was not released")
                    time.sleep(0.02)
            reply = json.dumps(extraction_plan(snapshot))
            if mode == "--invalid-json":
                reply = "não é um plano"
            elif mode == "--empty-plan":
                reply = json.dumps({"operations": []})
            elif mode == "--oversized-output":
                human = next(message for message in snapshot["messages"] if message["fromUser"])
                reply = json.dumps({"operations": [{"action": "remember", "text": "x" * 128_001,
                    "shared": False, "evidence": [human["id"]], "nature": "inferred"}]})
        elif "You name a personal assistant bot" in text:
            if os.environ.get("DONNABOT_FIXTURE_BACKGROUND_MODE") == "mcp-test":
                (pathlib.Path(os.environ["MEXERICA_HOME"]) / "conclusao-claude").touch()
            if mode.endswith("naming-boundary"):
                marker = pathlib.Path(mode)
                marker.with_suffix(".ready").touch()
                deadline = time.monotonic() + 30
                while not marker.with_suffix(".continue").exists():
                    if time.monotonic() > deadline:
                        raise TimeoutError("naming fixture was not released")
                    time.sleep(0.02)
                reply = "name: Nome novo"
            elif mode == "--naming-tool":
                send({"method": "session/update", "params": {"sessionId": session, "update": {
                    "sessionUpdate": "tool_call", "toolCallId": "forbidden", "kind": "read", "title": "externo"}}})
                reply = "name: Não aplicar"
            elif mode == "--naming-permission":
                send({"id": "pedido", "method": "session/request_permission", "params": {
                    "sessionId": session, "toolCall": {"toolCallId": "forbidden", "title": "externo"}, "options": []}})
                answer = json.loads(sys.stdin.readline())
                if answer.get("result", {}).get("outcome", {}).get("outcome") != "cancelled":
                    raise RuntimeError("o pedido de permissão não foi cancelado")
                reply = "name: Não aplicar"
            else:
                reply = "NONE"
        elif mode == "--hold-source-wire" and source_wire_prompt in text:
            # Barreira do novo pedido humano: não emite conclusão nem outro trabalho de memória.
            (source_wire_workspace / "source-wire-ready").touch()
            continue
        else:
            reply = "I will keep my explanations short."
        chunks = [reply[start:start + 64_000] for start in range(0, len(reply), 64_000)] if mode == "--oversized-output" else [reply]
        for chunk in chunks:
            send({"method": "session/update", "params": {"sessionId": session, "update": {
                "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": chunk}
            }}})
        result = {"stopReason": "end_turn"}
        if snapshot is not None or "You name a personal assistant bot" in text:
            result["_meta"] = {"quota": {"model_usage": [{"model": "claude-fixture-real"}],
                                  "token_count": {"inputTokens": 17, "outputTokens": 5,
                                                  "cachedInputTokens": 2, "cachedWriteTokens": 0}}}
        if "You name a personal assistant bot" in text and mode == "--naming-missing":
            result = {}
        if snapshot is not None:
            if mode == "--missing-stop":
                result = {}
            elif mode == "--bad-stop":
                result = {"stopReason": "cancelled"}
    else:
        result = {}
    if "id" in request:
        send({"id": request["id"], "result": result})
