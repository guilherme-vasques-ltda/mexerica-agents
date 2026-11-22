#!/usr/bin/env python3
"""HTTPS local de TESTE; atraso opcional no ajudante (arquivo /srv/prova/atraso) para simular crash no download."""
import http.server, os, ssl, time
RAIZ = "/srv/prova/www"
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(s, *a, **k): super().__init__(*a, directory=RAIZ, **k)
    def do_GET(s):
        if s.path.endswith("donnabot-motor-exec") and os.path.exists("/srv/prova/atraso"): time.sleep(30)
        super().do_GET()
srv = http.server.ThreadingHTTPServer(("127.0.0.1", 443), H)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); ctx.load_cert_chain("/srv/prova/tls/servidor.crt", "/srv/prova/tls/servidor.key")
srv.socket = ctx.wrap_socket(srv.socket, server_side=True); srv.serve_forever()
