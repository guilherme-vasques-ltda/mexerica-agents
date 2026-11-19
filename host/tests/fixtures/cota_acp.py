"""ACP sintética de cota (F6, MEX-1294): a conta decide se o turno responde "ok" ou esgota a cota.

`erro-comum` falha com um erro que não é de cota; `ferramenta-antes-do-limite` (junto de `cota-esgotada`) mostra uma
ferramenta antes do limite. A conta é a pasta de configuração que o launcher entregou (`CLAUDE_CONFIG_DIR`, ou `CODEX_HOME`). Se ela tiver o
arquivo `cota-esgotada`, o turno falha com "usage limit reached". Cada tentativa vai para
`$MEXERICA_HOME/cota-uso.jsonl` com o nome da pasta da conta e se havia chave de API paga no ambiente.
"""
import json
import os
import pathlib
import sys

config_dir = pathlib.Path(os.environ.get("CLAUDE_CONFIG_DIR") or os.environ.get("CODEX_HOME") or ".")
sessions = 0


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def record(outcome):
    entry = {"conta": config_dir.name, "resultado": outcome,
             "chave_api_paga": any(os.environ.get(k) for k in ("ANTHROPIC_API_KEY", "OPENAI_API_KEY"))}
    with open(pathlib.Path(os.environ["MEXERICA_HOME"]) / "cota-uso.jsonl", "a", encoding="utf-8") as log:
        log.write(json.dumps(entry) + "\n")


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if "id" not in request or method is None:
        continue
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": False, "sessionCapabilities": {}}}
    elif method == "session/new":
        sessions += 1
        result = {"sessionId": f"s{sessions}"}
    elif method == "session/prompt":
        if (config_dir / "erro-comum").exists():
            # Erro qualquer, que não é cota nem login: nunca troca de conta.
            record("erro")
            send({"id": request["id"], "error": {"code": -32603, "message": "boom"}})
            continue
        if (config_dir / "cota-esgotada").exists():
            if (config_dir / "aprovacao-aberta").exists():
                # Um cartão de aprovação ficou aberto na conta que esgotou: nunca migra para a outra conta.
                send({"id": 900, "method": "session/request_permission", "params": {
                    "sessionId": request["params"]["sessionId"],
                    "toolCall": {"toolCallId": "p1", "title": "Run", "kind": "execute", "status": "pending"},
                    "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}]}})
            if (config_dir / "ferramenta-antes-do-limite").exists():
                # A conta fez algo (uma ferramenta) antes de a cota acabar: o efeito pode ter ocorrido.
                send({"method": "session/update", "params": {"sessionId": request["params"]["sessionId"], "update": {
                    "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Run", "kind": "execute",
                    "status": "in_progress"}}})
            record("cota_esgotada")
            send({"id": request["id"], "error": {"code": -32000, "message": "usage limit reached"}})
            continue
        record("ok")
        send({"method": "session/update", "params": {"sessionId": request["params"]["sessionId"], "update": {
            "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "ok"}}}})
        result = {"stopReason": "end_turn"}
    else:
        result = {}
    send({"id": request["id"], "result": result})
