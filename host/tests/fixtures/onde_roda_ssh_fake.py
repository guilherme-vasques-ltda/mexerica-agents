#!/usr/bin/python3
"""Somente fixture. Não invoca SSH; gatilho usa listener loopback próprio."""
import os
import socket
import sys
import time

args = sys.argv[1:]
if args == ['-V']:
    sys.stderr.write('OpenSSH_9.2p1, fixture\n')
    sys.exit(0)
assert '-F' in args
config_path = args[args.index('-F') + 1]
with open(config_path, encoding='utf8') as config_file:
    config = config_file.read()
for required in ['StrictHostKeyChecking yes', 'IdentityAgent none', 'GlobalKnownHostsFile ',
                 'HostKeyAlgorithms ssh-ed25519', 'ProxyCommand none', 'ProxyJump none']:
    assert required in config
# bounded() envia a entrada e fecha stdin antes de colher o resultado.
# Aguarda esse fechamento; sair antes pode causar EPIPE no chamador.
# O listener -L mantém outro protocolo e não recebe entrada.
if '-L' not in args:
    sys.stdin.buffer.read()
sys.stderr.write('Authenticated to fixture using "publickey".\n')
if '-L' in args:
    host, port, remote = args[args.index('-L') + 1].split(':', 2)
    assert host == '127.0.0.1' and remote == '/nonexistent/donna-workplace-probe.sock'
    listener = socket.socket()
    listener.bind((host, int(port)))
    listener.listen(1)
    sys.stderr.write('Local forwarding listening on 127.0.0.1 port ' + port + '.\n')
    sys.stderr.flush()
    connection, _ = listener.accept()
    sys.stderr.write('channel 0: open failed: administratively prohibited: fixture\n')
    sys.stderr.flush()
    time.sleep(5)
    connection.close()
    listener.close()
elif '-R' in args:
    forward = args[args.index('-R') + 1]
    if forward.startswith('0:'):
        sys.stderr.write('remote forward failure for: listen 0\nremote port forwarding failed\n')
    else:
        path = forward.split(':', 1)[0]
        sys.stderr.write('remote forward failure for: listen ' + path + '\n')
        sys.stderr.write('remote port forwarding failed for listen path ' + path + '\n')
elif '-W' in args:
    assert args[args.index('-W') + 1] == '127.0.0.1:9'
    sys.stderr.write('channel 0: open failed: administratively prohibited: fixture\n')
elif '-tt' in args:
    sys.stderr.write('PTY allocation request failed on channel 0\n')
else:
    sys.stderr.write('channel 0: rcvd eof\nExit status 1\n')
sys.exit(1)
