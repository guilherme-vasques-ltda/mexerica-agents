#!/usr/bin/env python3
"""Fixture first-party: somente API de conta; qualquer turno/login/modelo é erro."""
import json
import os
from pathlib import Path
import sys
import socket
import subprocess

profile = Path(os.environ['CODEX_HOME']) if 'CODEX_HOME' in os.environ else Path(os.environ['HOME']) / '.codex'
assert sys.argv[1:] == ['app-server']
assert os.environ['HOME'] == str(profile.parent)
assert set(os.environ) <= {'PATH', 'LANG', 'LC_ALL', 'SYSTEMROOT', 'WINDIR', 'HOME', 'CODEX_HOME', 'DISABLE_AUTOUPDATER', 'CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC', 'BROWSER', 'TERM', 'LC_CTYPE'}
expected = iter(['initialize', 'initialized', 'account/rateLimits/read'])
for line in sys.stdin:
    request = json.loads(line)
    method = request['method']
    assert method == next(expected), method
    with (profile / 'requests').open('a') as log:
        log.write(method + '\n')
    mode = (profile / 'mode').read_text() if (profile / 'mode').exists() else 'ready'
    if method == 'initialized':
        continue
    if method == 'initialize':
        result = {}
    else:
        assert request['params'] == {}
        if mode in ('error', 'signed-out'):
            print(json.dumps({'id': request['id'], 'error': {'code': -1, 'message': 'fixture'}}), flush=True)
            continue
        if mode == 'malformed':
            print('invalid-json', flush=True)
            continue
        if mode.startswith('controlled'):
            child = subprocess.Popen(['/usr/bin/sleep', '600']) if mode == 'controlled-timeout' else None
            with socket.socket(socket.AF_UNIX) as barrier:
                barrier.connect(str(profile / 'ready.sock'))
                barrier.sendall((json.dumps([os.getpid()] + ([child.pid] if child else [])) + '\n').encode())
                barrier.recv(1)
        result = json.loads((profile / 'rates.json').read_text())
    print(json.dumps({'id': request['id'], 'result': result}), flush=True)
