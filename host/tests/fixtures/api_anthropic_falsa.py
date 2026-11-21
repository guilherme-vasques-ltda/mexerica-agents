#!/usr/bin/env python3
"""Servidor falso da API de mensagens da Anthropic, só para testes.

Nenhuma rede externa, nenhuma conta, nenhuma chamada paga: escuta em 127.0.0.1
numa porta livre e responde `/v1/messages` em streaming SSE no formato
documentado. A primeira chamada de uma conversa devolve um bloco `tool_use`
(Bash, Write ou Read, conforme o marcador no texto do pedido); a chamada
seguinte, que já traz `tool_result`, devolve texto final. Os caminhos usados
pelas ferramentas vêm do ambiente, definido por quem sobe o servidor:

  API_FALSA_TOQUE    arquivo que o Bash deve criar
  API_FALSA_ARQUIVO  arquivo que o Write deve gravar
  API_FALSA_LER       arquivo que o Read deve ler (existe antes do teste)

O servidor conta as chamadas que recebeu em `/v1/messages` e publica o total em
`GET /__chamadas`, para a prova confirmar que o programa de fato falou com ele.

Marcadores no texto do pedido do usuário:
  [[bash]]   → tool_use Bash com {"command": "touch <API_FALSA_TOQUE>"}
  [[write]]  → tool_use Write com {"file_path": <API_FALSA_ARQUIVO>, "content": ...}
  [[read]]   → tool_use Read com {"file_path": <API_FALSA_LER>}
  [[texto]]  → resposta em texto puro, sem ferramenta
"""
import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOQUE = os.environ.get("API_FALSA_TOQUE", "")
ARQUIVO = os.environ.get("API_FALSA_ARQUIVO", "")
LER = os.environ.get("API_FALSA_LER", "")
CONTADOR = 0
TRAVA = threading.Lock()


def bloco_tool_use(marcador):
    if marcador == "[[bash]]":
        return "Bash", {"command": "touch '%s'" % TOQUE.replace("'", "'\\''"), "description": "cria o arquivo de teste"}
    if marcador == "[[write]]":
        return "Write", {"file_path": ARQUIVO, "content": "conteúdo de teste da API falsa\n"}
    if marcador == "[[read]]":
        return "Read", {"file_path": LER}
    return None


def texto_do_pedido(corpo):
    textos = []
    for msg in corpo.get("messages", []):
        if msg.get("role") != "user":
            continue
        conteudo = msg.get("content")
        if isinstance(conteudo, str):
            textos.append(conteudo)
        elif isinstance(conteudo, list):
            textos.extend(b.get("text", "") for b in conteudo if isinstance(b, dict) and b.get("type") == "text")
    return " ".join(textos)


def tem_tool_result(corpo):
    for msg in corpo.get("messages", []):
        conteudo = msg.get("content")
        if isinstance(conteudo, list) and any(isinstance(b, dict) and b.get("type") == "tool_result" for b in conteudo):
            return True
    return False


class Pedido(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        pass

    def _responder(self, status, corpo, tipo="application/json"):
        dados = corpo if isinstance(corpo, bytes) else corpo.encode()
        self.send_response(status)
        self.send_header("Content-Type", tipo)
        self.send_header("Content-Length", str(len(dados)))
        self.end_headers()
        self.wfile.write(dados)

    def do_GET(self):
        if self.path.split("?")[0] == "/__chamadas":
            with TRAVA:
                chamadas = CONTADOR
            self._responder(200, json.dumps({"chamadas": chamadas}))
            return
        self._responder(404, json.dumps({"type": "error", "error": {"type": "not_found_error"}}))

    def do_POST(self):
        global CONTADOR
        tamanho = int(self.headers.get("Content-Length", "0"))
        bruto = self.rfile.read(tamanho)
        if self.path.split("?")[0] == "/v1/messages/count_tokens":
            self._responder(200, json.dumps({"input_tokens": 10}))
            return
        if self.path.split("?")[0] != "/v1/messages":
            self._responder(404, json.dumps({"type": "error", "error": {"type": "not_found_error"}}))
            return
        corpo = json.loads(bruto or b"{}")
        with TRAVA:
            CONTADOR += 1
            chamada = CONTADOR
        if tem_tool_result(corpo):
            self._stream_texto("resultado recebido, conversa encerrada", chamada)
            return
        texto = texto_do_pedido(corpo)
        marcadores = [m for m in ("[[bash]]", "[[write]]", "[[read]]", "[[texto]]") if m in texto]
        marcador = marcadores[0] if marcadores else None
        bloco = bloco_tool_use(marcador) if marcador not in (None, "[[texto]]") else None
        if bloco is None:
            self._stream_texto("resposta da API falsa sem ferramenta", chamada)
            return
        nome, entrada = bloco
        self._stream_ferramenta(nome, entrada, chamada)

    def _sse(self, eventos):
        dados = "".join("event: %s\ndata: %s\n\n" % (tipo, json.dumps(corpo)) for tipo, corpo in eventos)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Content-Length", str(len(dados)))
        self.end_headers()
        self.wfile.write(dados.encode())

    def _stream_texto(self, texto, chamada):
        self._sse([
            ("message_start", {"type": "message_start", "message": {"id": "msg_falso_%d" % chamada, "type": "message",
                "role": "assistant", "model": "claude-falso", "content": [], "stop_reason": None, "stop_sequence": None,
                "usage": {"input_tokens": 10, "output_tokens": 1}}}),
            ("content_block_start", {"type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}}),
            ("content_block_delta", {"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": texto}}),
            ("content_block_stop", {"type": "content_block_stop", "index": 0}),
            ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                "usage": {"output_tokens": 5}}),
            ("message_stop", {"type": "message_stop"}),
        ])

    def _stream_ferramenta(self, nome, entrada, chamada):
        eventos = [
            ("message_start", {"type": "message_start", "message": {"id": "msg_falso_%d" % chamada, "type": "message",
                "role": "assistant", "model": "claude-falso", "content": [], "stop_reason": None, "stop_sequence": None,
                "usage": {"input_tokens": 10, "output_tokens": 1}}}),
            ("content_block_start", {"type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}}),
            ("content_block_delta", {"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "vou usar uma ferramenta"}}),
            ("content_block_stop", {"type": "content_block_stop", "index": 0}),
            ("content_block_start", {"type": "content_block_start", "index": 1,
                "content_block": {"type": "tool_use", "id": "toolu_falso_%d" % chamada, "name": nome, "input": {}}}),
            ("content_block_delta", {"type": "content_block_delta", "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": json.dumps(entrada)}}),
            ("content_block_stop", {"type": "content_block_stop", "index": 1}),
            ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "tool_use", "stop_sequence": None},
                "usage": {"output_tokens": 5}}),
            ("message_stop", {"type": "message_stop"}),
        ]
        self._sse(eventos)


def main():
    porta = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    servidor = ThreadingHTTPServer(("127.0.0.1", porta), Pedido)
    print(json.dumps({"porta": servidor.server_address[1]}), flush=True)
    servidor.serve_forever()


if __name__ == "__main__":
    main()
