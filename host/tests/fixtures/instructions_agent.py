# Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
"""ACP fixture recording how a bot's instructions and compactions reach it.

`standard` advertises ACP `systemPrompt`; `reporting` reports compactions with `compaction_update`; `silent` only
shows them in `usage_update`; `codex` and `claude` answer `initialize` the way those adapters do, so the host asks
them to compact. A prompt containing COMPACT_NOW, or one that is a `/compact` command, compacts; one containing
`USAGE <n>` (optionally `SIZE <m>`) reports that much context in use. Every request is appended to
`requests.jsonl` in the working directory.
`hold` records the prompt but leaves the user's turn open until the host stops it.
"""
import json
import os
import pathlib
import subprocess
import re
import sys
import tomllib

mode = sys.argv[1] if len(sys.argv) > 1 else "reporting"
sessions = 0
claude_job_active = False
compactions = 0
scripted = set()
last_usage = {"used": 0, "size": 1000000}
permission_mode = os.environ.get("INITIAL_AGENT_MODE", "agent") if mode == "codex" else "acceptEdits"

# Marcador absoluto do início do processo de dream, antes de qualquer resposta ACP.
if mode == "codex" and len(sys.argv) > 2:
    config = json.loads(os.environ.get("CODEX_CONFIG", "{}"))
    if "You keep the memory a personal assistant bot" in config.get("developer_instructions", ""):
        pathlib.Path(sys.argv[2]).write_text("iniciado", encoding="utf-8")


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def update(session, value):
    global last_usage
    if value.get("sessionUpdate") == "usage_update":
        last_usage = {"used": value["used"], "size": value["size"]}
    send({"method": "session/update", "params": {"sessionId": session, "update": value}})


def compact(session):
    global compactions
    compactions += 1
    cid = f"c{compactions}"
    update(session, {"sessionUpdate": "compaction_update", "compactionId": cid, "status": "in_progress"})
    completed = {"sessionUpdate": "compaction_update", "compactionId": cid, "status": "completed"}
    summarized = {"sessionUpdate": "usage_update", "used": 20000, "size": 1000000}
    if mode == "codex":
        # codex-acp 2.7.0 (captura real, MEXE-42): repete a medida anterior, mede o pedido de resumo e mede o
        # contexto resumido, tudo ANTES do `completed`, e nada depois. CAPTURE_2_7_0 usa os números da captura;
        # LATE_*, FAIL_COMPACT (pedidos num prompt anterior) simulam avisos atrasados e falha.
        previous = {"sessionUpdate": "usage_update", **last_usage}
        summarized["used"] = 7147 if "CAPTURE_2_7_0" in scripted else 20000 - 2000 * (compactions - 1)
        summarized["size"] = previous["size"]
        update(session, previous)
        update(session, {**previous, "used": previous["used"] + 87000})
        update(session, summarized)
        if "LATE_DUPLICATE" in scripted and compactions > 1:
            update(session, {**completed, "compactionId": f"c{compactions - 1}"})
        if "LATE_OLDEST" in scripted and compactions > 2:
            update(session, {**completed, "compactionId": "c1"})
        if "LATE_STALE" in scripted:
            update(session, previous)
        if "FAIL_COMPACT" in scripted:
            update(session, {**completed, "status": "failed"})
            return
        update(session, completed)
        return
    if mode == "claude":
        completed["summary"] = [{"type": "text", "text": f"Summary {compactions}: the user is planning a trip."}]
    update(session, completed)
    update(session, summarized)


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    params = request.get("params", {})
    config = json.loads(os.environ.get("CODEX_CONFIG", "{}"))
    codex_job = mode == "codex" and "You name a personal assistant bot" in config.get("developer_instructions", "")
    claude_job = mode == "claude" and method == "session/new" and "You name a personal assistant bot" in str(params.get("_meta", {}).get("systemPrompt", ""))
    is_job = codex_job or claude_job
    log_path = pathlib.Path(os.environ["MEXERICA_HOME"]) / "background" / "requests.jsonl" if is_job else pathlib.Path("requests.jsonl")
    with open(log_path, "a", encoding="utf-8") as log:
        log.write(json.dumps({"method": method, "params": params, **({"codexConfig": json.loads(os.environ.get("CODEX_CONFIG", "{}")), "nativeCronDisabled": os.environ.get("CLAUDE_CODE_DISABLE_CRON"), "initialAgentMode": os.environ.get("INITIAL_AGENT_MODE")} if method == "initialize" else {})}) + "\n")
    if method == "initialize":
        if codex_job:
            for provider in config.get("model_providers", {}).values():
                command = provider.get("auth", {}).get("command")
                if command:
                    subprocess.run(command, shell=True, check=True)
            config_file = pathlib.Path(os.environ["CODEX_HOME"]) / "config.toml"
            if config_file.exists() and "mcp_servers" in tomllib.loads(config_file.read_text()):
                (pathlib.Path(os.environ["MEXERICA_HOME"]) / "efeito-config").write_text("executado")
        capabilities = {"loadSession": mode in ("claude", "grok"), "sessionCapabilities": {"additionalDirectories": {}}}
        if mode in ("standard", "grok"):
            capabilities["sessionCapabilities"]["systemPrompt"] = {}
        if mode == "claude":
            capabilities["_meta"] = {"claudeCode": {}}
        result = {"protocolVersion": 1, "agentCapabilities": capabilities}
        if mode == "codex":
            result["agentInfo"] = {"name": "@agentclientprotocol/codex-acp", "version": "2.2.2"}
    elif method == "session/new":
        if claude_job:
            claude_job_active = True
            options = params.get("_meta", {}).get("claudeCode", {}).get("options", {})
            if not options.get("strictMcpConfig"):
                for config_dir in [pathlib.Path.home(), pathlib.Path(os.environ.get("CLAUDE_CONFIG_DIR", ""))]:
                    persisted = config_dir / ".claude.json"
                    if persisted.exists():
                        for server in json.loads(persisted.read_text()).get("mcpServers", {}).values():
                            subprocess.run(server["command"], shell=True, check=True)
        sessions += 1
        result = {"sessionId": f"s{sessions}"}
    elif method == "session/load":
        result = {}
    elif method == "session/set_config_option" and params.get("configId") == "permission-policy":
        permission_mode = params["value"]
        result = {}
    elif method == "session/prompt":
        session = params["sessionId"]
        text = params["prompt"][0]["text"]
        if claude_job_active:
            (pathlib.Path(os.environ["MEXERICA_HOME"]) / "conclusao-claude").write_text("ok")
        if is_job:
            send({"id":"job-write", "method":"session/request_permission", "params":{
                "sessionId":session, "toolCall":{"kind":"edit", "title":"Escrever arquivo"},
                "options":[{"optionId":"allow", "kind":"allow_once"}]}})
            response = json.loads(next(sys.stdin))
            outcome = response.get("result", {}).get("outcome", {})
            if outcome.get("optionId") == "allow":
                (pathlib.Path(os.environ["MEXERICA_HOME"]) / "efeito-job").write_text("executado")
            (pathlib.Path(os.environ["MEXERICA_HOME"]) / "resposta-job").write_text(outcome.get("outcome", "ausente"))
        if mode == "hold-orphan-update":
            update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "sem-chamada",
                             "rawInput": {"server": "memory"}, "status": "completed",
                             "content": [{"type": "text", "text": "Preferência vinda de fora"}]})
            update(session, {"sessionUpdate": "agent_message_chunk", "content": {
                "type": "text", "text": "Atualização ACP recebida"}})
            continue
        if mode == "hold" or "--hold" in sys.argv:
            continue
        scripted.update(
            word for word in ("LATE_DUPLICATE", "LATE_OLDEST", "LATE_STALE", "FAIL_COMPACT", "CAPTURE_2_7_0") if word in text
        )
        if text.startswith("/compact") or ("COMPACT_NOW" in text and mode != "silent"):
            compact(session)
            if text.startswith("/compact"):
                send({"id": request["id"], "result": {"stopReason": "end_turn"}})
                continue
        usage = re.search(r"USAGE (\d+)", text)
        if usage:
            size = re.search(r"SIZE (\d+)", text)
            update(session, {"sessionUpdate": "usage_update", "used": int(usage.group(1)),
                             "size": int(size.group(1)) if size else 200000})
        update(session, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "ok"}})
        result = {"stopReason": "end_turn"}
        # DON-908: custo cumulativo ACP e tokens reais do turno, só nas cenas C8.
        cost = re.search(r"COST ([0-9.]+)", text)
        if cost:
            update(session, {"sessionUpdate": "usage_update", "used": 1234, "size": 200000,
                             "cost": {"amount": float(cost.group(1)), "currency": "USD"}})
        if cost or "C8_TOKENS" in text:
            result["usage"] = {"inputTokens": 10, "cachedReadTokens": 20,
                               "cachedWriteTokens": 30, "outputTokens": 40}
    else:
        result = {}
    if mode == "claude" and method in ("session/new", "session/load", "session/set_config_option"):
        if method in ("session/new", "session/load"):
            assert params["_meta"]["claudeCode"]["options"]["permissionMode"] == "default"
        result["configOptions"] = [{"id": "permission-policy", "category": "mode", "currentValue": permission_mode}]
    if mode == "codex" and method in ("session/new", "session/load", "session/set_config_option"):
        result["configOptions"] = [{"id": "permission-policy", "category": "mode", "currentValue": permission_mode}]
    if "id" in request:
        send({"id": request["id"], "result": result})
