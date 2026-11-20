# Agente ACP mínimo para o teste de computadores: aceita o pedido e segura o turno aberto
# (sem responder) até receber cancelamento, para que o Hub trate o turno como vivo.
import json,sys
def send(m): sys.stdout.write(json.dumps(m)+"\n"); sys.stdout.flush()
for line in sys.stdin:
    m=json.loads(line)
    if "method" not in m: continue
    meth=m["method"]
    if meth=="initialize": send({"jsonrpc":"2.0","id":m["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":False}}})
    elif meth=="session/new": send({"jsonrpc":"2.0","id":m["id"],"result":{"sessionId":"s1"}})
    elif meth=="session/prompt": pass
    elif meth=="session/cancel": pass
