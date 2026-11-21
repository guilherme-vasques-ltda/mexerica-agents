"""Agente ACP que é cortado no meio: abre uma ferramenta e escreve a resposta sem parar até o Hub cair.

Cada pedido recebido é anotado no arquivo do primeiro argumento, para o teste conferir que nada foi repetido.
"""

import json
import sys
import threading
import time

LOG = sys.argv[1]
SESSION = "cut"


def send(value):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", **value}) + "\n")
    sys.stdout.flush()


def update(value):
    send({"method": "session/update", "params": {"sessionId": SESSION, "update": value}})


def turn():
    tool = {"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Long task", "kind": "execute"}
    update({**tool, "status": "in_progress"})
    part = 0
    try:
        while True:
            part += 1
            update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": f"p{part} "}})
            time.sleep(0.02)
    except (BrokenPipeError, OSError):
        return


for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        send({"id": message["id"], "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": False}}})
    elif method == "session/new":
        send({"id": message["id"], "result": {"sessionId": SESSION}})
    elif method == "session/prompt":
        with open(LOG, "a", encoding="utf-8") as log:
            log.write("prompt\n")
        threading.Thread(target=turn, daemon=True).start()
