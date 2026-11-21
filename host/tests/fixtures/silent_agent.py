"""Motor de mentira para o aviso de motor mudo: o modo vem do primeiro argumento.

mudo      um trecho de texto e depois silêncio total, até o cancelamento
lento     um trecho por segundo durante 6 s e depois termina (demora, mas dá sinal)
volta     um trecho, 5 s de silêncio, outro trecho e termina (volta depois do aviso)
ferramenta  uma ferramenta em execução, 6 s de silêncio e termina
cartao    pede permissão e espera a resposta, em silêncio, até o cancelamento
"""

import json
import sys
import threading

MODE = sys.argv[1] if len(sys.argv) > 1 else "mudo"
SID = "mudo-s1"
cancelled = threading.Event()
answers = {}


def send(value):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", **value}) + "\n")
    sys.stdout.flush()


def update(body):
    send({"method": "session/update", "params": {"sessionId": SID, "update": body}})


def chunk(text):
    update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})


def finish(rid, reason="end_turn"):
    send({"id": rid, "result": {"stopReason": reason}})


def turn(rid):
    chunk("Começando")
    if MODE == "mudo":
        cancelled.wait()
    elif MODE == "lento":
        for i in range(6):
            if cancelled.wait(1):
                return finish(rid, "cancelled")
            chunk(f" passo {i}")
    elif MODE == "volta":
        if cancelled.wait(5):
            return finish(rid, "cancelled")
        chunk(" voltei")
    elif MODE == "ferramenta":
        update({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Terminal", "kind": "execute", "status": "in_progress"})
        if cancelled.wait(6):
            return finish(rid, "cancelled")
        update({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"})
        chunk(" feito")
    elif MODE == "cartao":
        update({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit a.txt", "kind": "edit", "status": "pending"})
        send({
            "id": 900,
            "method": "session/request_permission",
            "params": {
                "sessionId": SID,
                "toolCall": {"toolCallId": "t1", "title": "Edit a.txt", "kind": "edit", "rawInput": {"command": "touch a.txt"}},
                "options": [
                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                ],
            },
        })
        cancelled.wait()
    if cancelled.is_set():
        return finish(rid, "cancelled")
    finish(rid)


for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method is None:
        continue
    if method == "initialize":
        send({"id": message["id"], "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": False}}})
    elif method == "session/new":
        send({"id": message["id"], "result": {"sessionId": SID}})
    elif method == "session/prompt":
        threading.Thread(target=turn, args=(message["id"],), daemon=True).start()
    elif method == "session/cancel":
        cancelled.set()
