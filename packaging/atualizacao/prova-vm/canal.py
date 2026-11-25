#!/usr/bin/env python3
"""Conecta ao canal do motor sem enviar nada e confere a recusa: uso `canal.py TRECHO_ESPERADO`.
Use "sem-configuracao" para conferir o fechamento sem resposta no modo local."""
import socket, sys
esperado = sys.argv[1] if len(sys.argv) > 1 else "UID do Hub"
s = socket.socket(socket.AF_UNIX)
s.settimeout(5)
s.connect("/run/donnabot-motor-copia.sock")
# Nada é enviado: a recusa ocorre antes de qualquer leitura (dados não lidos
# no fechamento fariam o kernel responder ECONNRESET).
s.shutdown(socket.SHUT_WR)
partes = []
while (bloco := s.recv(4096)):
    partes.append(bloco)
resposta = b"".join(partes).decode()
print(resposta.strip())
if esperado == "sem-configuracao":
    sys.exit(0 if not resposta else 1)
sys.exit(0 if esperado in resposta and '"ok":false' in resposta else 1)
