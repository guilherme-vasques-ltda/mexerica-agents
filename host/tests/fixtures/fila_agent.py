"""Agente ACP para a fila: o pedido com "segura" nunca termina (deixa o bot ocupado); os outros respondem "ok".

Cada pedido recebido vai, numa linha, para o arquivo do primeiro argumento.
"""

import json
import sys
import threading
import time

LOG = sys.argv[1]
SESSION = "fila"


def send(value):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", **value}) + "\n")
    sys.stdout.flush()


def answer(request, text):
    time.sleep(0.05)
    update = {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "ok"}}
    send({"method": "session/update", "params": {"sessionId": SESSION, "update": update}})
    send({"id": request, "result": {"stopReason": "end_turn"}})


for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        send({"id": message["id"], "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": False}}})
    elif method == "session/new":
        send({"id": message["id"], "result": {"sessionId": SESSION}})
    elif method == "session/prompt":
        text = " ".join(part.get("text", "") for part in message["params"]["prompt"])
        with open(LOG, "a", encoding="utf-8") as log:
            log.write(text.replace("\n", " ") + "\n")
        if "segura" not in text:
            threading.Thread(target=answer, args=(message["id"], text), daemon=True).start()
