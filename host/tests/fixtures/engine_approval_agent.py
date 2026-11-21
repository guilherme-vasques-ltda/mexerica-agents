#!/usr/bin/env python3
"""Motor de mentira que se comporta como o Kilo ou o Cline, para provar se o pedido de aprovação chega ao Hub.

Uso: engine_approval_agent.py <kilo|cline> [argumentos do motor]. Nenhuma rede, modelo ou conta real.

Kilo (kilocode v7.8.32): a regra de cada ferramenta sai do ruleset padrão do agente `build` mais a variável
KILO_PERMISSION; a última regra que casa vence. Padrões copiados da leitura de packages/opencode/src/agent/agent.ts
(baseDefaults, "*": "allow") e packages/opencode/src/kilocode/agent/index.ts (bash). Se a regra é «allow», o motor
executa sem pedir (nenhum session/request_permission); se é «ask», pede com as opções once/always/reject.

Cline (apps/cli/src/acp/acpAgent.ts): pergunta por padrão; com a opção de sessão auto_approve ligada (ou com
--auto-approve true / --yolo no lançamento) executa sem pedir. A opção é boolean e aceita string.

Cada tentativa de ferramenta grava uma linha em decisions.jsonl; executar cria o arquivo executou-<ferramenta>.
"""
import json
import os
import re
import subprocess
import sys
from pathlib import Path

engine = sys.argv[1]
args = sys.argv[2:]
cwd = Path.cwd()


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def log(name, value):
    with (cwd / name).open("a", encoding="utf-8") as out:
        out.write(json.dumps(value) + "\n")


def wild(pattern, text):
    return re.fullmatch(re.escape(pattern).replace(r"\*", ".*"), text, re.S) is not None


def from_config(config):
    rules = []
    for key, value in config.items():
        if isinstance(value, str):
            rules.append((key, "*", value))
        else:
            rules.extend((key, pattern, action) for pattern, action in value.items())
    return rules


def deep_merge(base, overlay):
    merged = dict(base)
    for key, value in overlay.items():
        if isinstance(value, dict) and isinstance(merged.get(key), dict):
            merged[key] = deep_merge(merged[key], value)
        else:
            merged[key] = value
    return merged


def user_global_config():
    config = Path(os.environ.get("XDG_CONFIG_HOME", "/nonexistent")) / "kilo" / "kilo.json"
    return json.loads(config.read_text()) if config.is_file() else {}


def user_agent_permission():
    """Regra por agente na config global (`agent.build.permission`): vem DEPOIS da regra geral (agent.ts)."""
    permission = user_global_config().get("agent", {}).get("build", {}).get("permission", {})
    return {"*": permission} if isinstance(permission, str) else permission


def user_global_permission():
    """Config global do usuário (`$XDG_CONFIG_HOME/kilo/kilo.json`), mesclada por baixo de KILO_PERMISSION (config.ts)."""
    config = Path(os.environ.get("XDG_CONFIG_HOME", "/nonexistent")) / "kilo" / "kilo.json"
    permission = user_global_config().get("permission", {})
    return {"*": permission} if isinstance(permission, str) else permission


def kilo_rules():
    defaults = from_config({
        "*": "allow", "doom_loop": "ask", "external_directory": {"*": "ask"},
        "suggest": "deny", "question": "deny", "plan_enter": "deny", "plan_exit": "deny",
        "repo_clone": "deny", "repo_overview": "deny",
        "read": {"*": "allow", "*.env": "ask", "*.env.*": "ask", "*.env.example": "allow"},
    })
    patch = from_config({"bash": {"*": "ask", "cat *": "allow", "ls *": "allow"}, "recall": "ask"})
    build = from_config({"question": "allow", "suggest": "allow", "plan_enter": "allow"})
    user = from_config(deep_merge(user_global_permission(), json.loads(os.environ.get("KILO_PERMISSION") or "{}")))
    return defaults + patch + build + user + from_config(user_agent_permission())


def kilo_decision(permission, target):
    last = "ask"
    for rule_permission, pattern, action in kilo_rules():
        if wild(rule_permission, permission) and wild(pattern, target):
            last = action
    return last


log("lancado.json", {"engine": engine, "args": args,
                     "env": {k: os.environ.get(k) for k in ("XDG_CONFIG_HOME", "KILO_TEST_HOME", "XDG_DATA_HOME", "KILO_PERMISSION")}})
HUB_SERVERS = []
SESSIONS = cwd / "sessions.json"


def sessions():
    return json.loads(SESSIONS.read_text()) if SESSIONS.is_file() else {}


def call_hub(server, tool, arguments):
    """Chama uma ferramenta do Hub pelo servidor MCP que o Hub entregou na sessão, como o motor real faria."""
    definition = next((d for d in HUB_SERVERS if d.get("name") == server), None)
    if definition is None:
        return {"error": f"servidor {server} não entregue"}
    env = dict(os.environ)
    env.update({item["name"]: item["value"] for item in definition.get("env", [])})
    process = subprocess.Popen([definition["command"], *definition.get("args", [])], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, text=True)

    def rpc(number, method, params):
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": number, "method": method, "params": params}) + chr(10))
        process.stdin.flush()
        return json.loads(process.stdout.readline())

    rpc(1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "fake", "version": "1"}})
    answer = rpc(2, "tools/call", {"name": tool, "arguments": arguments})
    process.stdin.close()
    process.wait(timeout=10)
    return answer.get("result", answer)


def run_hub_tools():
    """Cada chamada vai ao Hub; o resultado fica em hubcalls.jsonl para o teste comparar com o que a pessoa decidiu."""
    for server, tool, arguments in HUB_CALLS:
        result = call_hub(server, tool, arguments)
        texto = (result.get("content") or [{}])[0].get("text", "") if isinstance(result, dict) else ""
        log("hubcalls.jsonl", {"server": server, "tool": tool, "isError": result.get("isError", False) or "error" in result,
                               "text": texto[:300]})


HUB_CALLS = [
    ("memory", "search_memory", {"query": "x"}),
    ("memory", "remember", {"text": "lembrete do motor"}),
    ("chat", "set_name", {"name": "Renomeado pelo motor"}),
    ("routines", "save_routine", {"name": "r", "instruction": "i", "triggers": [{"type": "interval", "seconds": 3600}]}),
    ("routines", "ferramenta_nova_que_ninguem_classificou", {}),
]


def launched_with_auto_approve():
    if "--yolo" in args:
        return True
    return "--auto-approve" in args and args[args.index("--auto-approve") + 1:][:1] == ["true"]


auto_approve = launched_with_auto_approve()
model = "m1"


def cline_options():
    options = [{"id": "model", "category": "model", "type": "select", "currentValue": model,
                "options": [{"value": "m1", "name": "m1"}]}]
    if "--sem-opcao" not in args:
        options.append({"id": "auto_approve", "type": "boolean", "currentValue": auto_approve})
    return options


TOOLS = [
    ("read", "README.md", "read", "Ler README.md", {"filepath": "README.md"}),
    ("edit", "a.txt", "edit", "Editar a.txt", {"filepath": "a.txt"}),
    ("bash", "rm -rf build", "execute", "rm -rf build", {"command": "rm -rf build"}),
    ("manage_update_bot", "*", "other", "manage_update_bot", {"botId": "outro"}),
]
OPTIONS = {
    "kilo": [{"optionId": "once", "kind": "allow_once", "name": "Allow once"},
             {"optionId": "always", "kind": "allow_always", "name": "Always allow"},
             {"optionId": "reject", "kind": "reject_once", "name": "Reject"}],
    "cline": [{"optionId": "allow_once", "kind": "allow_once", "name": "Allow once"},
              {"optionId": "allow_always", "kind": "allow_always", "name": "Allow always"},
              {"optionId": "reject_once", "kind": "reject_once", "name": "Reject"}],
}


def decide(permission, target, session=None):
    if engine == "kilo":
        # Uma sessão carregada traz as permissões que já tinha (allowEverything, kilocode/permission/allow-everything.ts).
        if session and sessions().get(session, {}).get("allow_everything"):
            return "allow"
        return kilo_decision(permission, target)
    return "allow" if auto_approve else "ask"


def update(session, value):
    send({"method": "session/update", "params": {"sessionId": session, "update": value}})


def run_tool(session, number, tool):
    permission, target, kind, title, raw = tool
    call = f"t{number}"
    update(session, {"sessionUpdate": "tool_call", "toolCallId": call, "title": title, "kind": kind,
                     "status": "pending", "rawInput": raw})
    decision = decide(permission, target, session)
    entry = {"tool": permission, "decision": decision, "options": None, "executed": False}
    if decision == "ask":
        send({"id": 900 + number, "method": "session/request_permission", "params": {
            "sessionId": session,
            "toolCall": {"toolCallId": call, "title": title, "kind": kind, "rawInput": raw},
            "options": OPTIONS[engine]}})
        entry["options"] = [option["optionId"] for option in OPTIONS[engine]]
        answer = json.loads(sys.stdin.readline())
        outcome = answer.get("result", {}).get("outcome", {})
        allowed = outcome.get("outcome") == "selected" and outcome.get("optionId") in ("once", "always", "allow_once", "allow_always")
        entry["answer"] = outcome.get("optionId", outcome.get("outcome"))
    else:
        allowed = decision == "allow"
    if allowed:
        (cwd / f"executou-{permission}").write_text("sim", encoding="utf-8")
        entry["executed"] = True
    log("decisions.jsonl", entry)
    update(session, {"sessionUpdate": "tool_call_update", "toolCallId": call,
                     "status": "completed" if allowed else "failed"})


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    params = request.get("params", {})
    log("requests.jsonl", {"method": method, "params": params})
    if "id" not in request:
        continue
    result = {}
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": engine == "kilo"}}
    elif method == "session/new":
        result = {"sessionId": "s1"}
        HUB_SERVERS[:] = params.get("mcpServers", [])
        if engine == "kilo":
            todas = sessions()
            result = {"sessionId": f"k{len(todas) + 1}"}
            todas[result["sessionId"]] = {"allow_everything": False}
            SESSIONS.write_text(json.dumps(todas))
        if engine == "cline":
            result["configOptions"] = cline_options()
    elif method == "session/load":
        result = {}
    elif method == "session/set_config_option":
        if engine == "cline" and params.get("configId") == "auto_approve":
            value = params["value"]
            if "--stubborn" not in args:
                auto_approve = value is True or value == "true"
        elif engine == "cline" and params.get("configId") == "model":
            model = params["value"]
        if engine == "cline":
            result["configOptions"] = cline_options()
    elif method == "session/prompt":
        if "--hub-tools" in args:
            run_hub_tools()
        elif "--sem-ferramentas" not in args:
            for number, tool in enumerate(TOOLS):
                run_tool(params["sessionId"], number, tool)
        update(params["sessionId"], {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "fim"}})
        result = {"stopReason": "end_turn"}
    send({"id": request["id"], "result": result})
