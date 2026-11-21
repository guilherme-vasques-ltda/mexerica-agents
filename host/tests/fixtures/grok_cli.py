#!/usr/bin/env python3
"""Programa falso que imita o Grok Build. Nenhuma rede, conta ou credencial real."""
import os
import sys
import time
from pathlib import Path

base = Path(sys.argv[0]).parent
home = Path(os.environ.get('GROK_HOME', '/fixture-missing'))
args = sys.argv[1:]


def setting(name, default=''):
    path = base / name
    return path.read_text().strip() if path.is_file() else default


if args == ['--version']:
    print(setting('version', 'grok 0.0.0-fixture'))
    sys.exit(0)

# Isolamento que o Hub promete: perfil da conta, sem chave de API, sem navegador.
assert 'XAI_API_KEY' not in os.environ
assert os.environ.get('HOME') == str(home.parent)
assert os.environ.get('BROWSER') == '/usr/bin/false'

auth = home / 'auth.json'

if args == ['models']:
    if auth.is_file():
        print('You are logged in with grok.com.')
        print('grok-4.7')
        sys.exit(0)
    print('You are not authenticated.')
    sys.exit(1)

assert args == ['login', '--device-auth'], args
with (base / 'login-pids').open('a') as pids:
    pids.write(str(os.getpid()) + '\n')

style = setting('style', 'plain')
if style == 'plain':
    print('Open this page on any device:', flush=True)
    print('  https://auth.x.ai/device', flush=True)
    print('Then enter this code: ABCD-EFGH', flush=True)
elif style == 'confirm':
    print('Confirm this code in your browser: WXYZ-2468', flush=True)
    print('https://accounts.x.ai/device', flush=True)
elif style == 'complete':
    print('Open https://auth.x.ai/device?user_code=QRST-1357 to continue.', flush=True)
elif style == 'ansi':
    print('\x1b]52;c;TEST_SECRET_SENTINEL\x07\x1b[1mhttps://auth.x.ai/de', end='', flush=True)
    time.sleep(.05)
    print('vice\x1b[0m', flush=True)
    print('\x1b[36mThen enter this code:\x1b[0m \x1b[1mABCD-', end='', flush=True)
    time.sleep(.05)
    print('EFGH\x1b[0m', flush=True)
elif style == 'foreign':
    print('https://evil.example.invalid/device', flush=True)
    print('Then enter this code: ABCD-EFGH', flush=True)
elif style == 'partial':
    print('Open this page on any device:', flush=True)
    print('  https://auth.x.ai/device', flush=True)
    print('Then enter this code: ABCD-EFGH', flush=True)
print('Waiting for authorization...', flush=True)

deadline = time.time() + 600
while time.time() < deadline:
    if (base / 'approve').is_file():
        home.mkdir(parents=True, exist_ok=True)
        auth.write_text('{"access_token":"FIXTURE-TOKEN-SECRET"}')
        auth.chmod(0o600)
        print('Login successful.', flush=True)
        if style == 'partial':
            print('Then enter this code: WXYZ-2468', end='', flush=True)
        sys.exit(0)
    if (base / 'deny').is_file():
        print('Authorization denied. The user rejected the request.', flush=True)
        sys.exit(1)
    if (base / 'expire').is_file():
        print('Device code expired. Run grok login --device-auth again.', flush=True)
        sys.exit(1)
    time.sleep(.05)
sys.exit(2)
