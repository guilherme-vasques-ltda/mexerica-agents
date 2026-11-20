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


def kilo_rules():
    defaults = from_config({
        "*": "allow", "doom_loop": "ask", "external_directory": {"*": "ask"},
        "suggest": "deny", "question": "deny", "plan_enter": "deny", "plan_exit": "deny",
        "repo_clone": "deny", "repo_overview": "deny",
        "read": {"*": "allow", "*.env": "ask", "*.env.*": "ask", "*.env.example": "allow"},
    })
    patch = from_config({"bash": {"*": "ask", "cat *": "allow", "ls *": "allow"}, "recall": "ask"})
    build = from_config({"question": "allow", "suggest": "allow", "plan_enter": "allow"})
    user = from_config(json.loads(os.environ.get("KILO_PERMISSION") or "{}"))
    return defaults + patch + build + user


def kilo_decision(permission, target):
    last = "ask"
    for rule_permission, pattern, action in kilo_rules():
        if wild(rule_permission, permission) and wild(pattern, target):
            last = action
    return last


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


def decide(permission, target):
    if engine == "kilo":
        return kilo_decision(permission, target)
    return "allow" if auto_approve else "ask"


def update(session, value):
    send({"method": "session/update", "params": {"sessionId": session, "update": value}})


def run_tool(session, number, tool):
    permission, target, kind, title, raw = tool
    call = f"t{number}"
    update(session, {"sessionUpdate": "tool_call", "toolCallId": call, "title": title, "kind": kind,
                     "status": "pending", "rawInput": raw})
    decision = decide(permission, target)
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
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": False}}
    elif method == "session/new":
        result = {"sessionId": "s1"}
        if engine == "cline":
            result["configOptions"] = cline_options()
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
        for number, tool in enumerate(TOOLS):
            run_tool(params["sessionId"], number, tool)
        update(params["sessionId"], {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "fim"}})
        result = {"stopReason": "end_turn"}
    send({"id": request["id"], "result": result})
