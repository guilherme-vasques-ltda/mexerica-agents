#!/usr/bin/env python3
"""D164: processos próprios de fixture. Nenhuma rede, modelo ou credencial real."""
import json
import os
from pathlib import Path
import sys
import threading
import subprocess
import time

backend = 'claude' if Path(sys.argv[0]).name.startswith('claude') else 'codex'
base = Path(sys.argv[0]).parent
profile = Path(os.environ.get('CLAUDE_CONFIG_DIR' if backend == 'claude' else 'CODEX_HOME', '/fixture-missing'))

def setting(name, default=''):
    p = base / name
    return p.read_text().strip() if p.is_file() else default

def emit(value):
    print(json.dumps(value), flush=True)

def identity():
    return setting('identity', 'owner@example.invalid')

def signed_in():
    return (profile / '.fixture-auth').is_file()

if sys.argv[-1:] == ['--version']:
    if setting('scenario') == 'version_hang':
        child = subprocess.Popen(['/usr/bin/sleep', '600'])
        marker = base / 'fixture-version-pids'
        pending = marker.with_suffix('.tmp')
        pending.write_text(str(os.getpid()) + '\n' + str(child.pid) + '\n')
        pending.replace(marker)
        time.sleep(600)
    print('claude 2.1.309' if backend == 'claude' else 'codex 0.164.0')
    sys.exit(0)

if backend == 'claude':
    if sys.argv[1:3] == ['auth', 'status']:
        if setting('scenario').startswith('pty_eof_'):
            with (base / 'pty-status-calls').open('a') as calls:
                calls.write('status\n')
        if not signed_in() or setting('scenario') == 'pty_eof_unverified':
            emit({'loggedIn': False})
        else:
            emit({'loggedIn': True, 'email': (profile / '.fixture-auth').read_text(), 'orgId': 'fixture-org', 'authMethod': 'claude.ai', 'apiProvider': 'firstParty', 'subscriptionType': 'max'})
        sys.exit(0)
    if sys.argv[1:] == ['auth', 'logout']:
        with (base / 'logout-calls').open('a') as calls:
            calls.write('logout\n')
        if setting('scenario') != 'logout_ignored':
            (profile / '.fixture-auth').unlink(missing_ok=True)
        sys.exit(0)
    assert sys.argv[1:] == ['auth', 'login']
    assert os.environ.get('HOME') == str(profile.parent)
    if setting('scenario').startswith('pty_eof_'):
        with (base / 'pty-login-pids').open('a') as pids:
            pids.write(str(os.getpid()) + '\n')
    print('\x1b]52;c;TEST_SECRET_SENTINEL\x07https://cla', end='', flush=True)
    time.sleep(.03)
    print('ude.ai/oauth/authorize?state=fixture', flush=True)
    code = sys.stdin.readline().strip()
    assert code == 'FIXTURE-CODE'
    (profile / '.fixture-auth').write_text(identity())
    if setting('scenario').startswith('pty_eof_'):
        # Controle first-party: transporte fechado, filho vivo até liberação explícita.
        for fd in (0, 1, 2):
            os.close(fd)
        (base / 'pty-closed.json').write_text(json.dumps({'pid': os.getpid(), 'profile': str(profile)}))
        while not (base / 'pty-release').is_file():
            time.sleep(.01)
        sys.exit(7 if setting('scenario') == 'pty_eof_nonzero' else 0)
    sys.exit(0)

assert sys.argv[1:] == ['app-server']
assert os.environ.get('HOME') == str(profile.parent)
helpers = None
if setting('scenario').startswith('codex_arg0_'):
    # Codex 0.164.0 keeps these aliases until its stdin/EOF shutdown finishes.
    helpers = profile / 'tmp' / 'arg0-linux' / 'codex-arg0fixture'
    helpers.mkdir(parents=True)
    for name in ['apply_patch', 'applypatch', 'codex-linux-sandbox']:
        (helpers / name).symlink_to(Path(sys.argv[0]).resolve())
for line in sys.stdin:
    request = json.loads(line)
    method = request.get('method')
    if method == 'initialized':
        continue
    if method == 'initialize':
        result = {'userAgent': 'd164-fixture'}
    elif method == 'account/login/start':
        assert request['params'] == {'type': 'chatgptDeviceCode'}
        result = {'type': 'chatgptDeviceCode', 'loginId': 'fixture-login', 'verificationUrl': 'https://auth.openai.com/codex/device', 'userCode': 'FIXTURE-CODE'}
        def complete():
            mode = setting('scenario')
            if mode == 'controlled_login':
                # Só o teste libera esta fase; o Host encerra o grupo em cancelamento/TTL.
                while not (base / 'login-release').is_file():
                    time.sleep(.01)
            else:
                time.sleep(.3)
            if mode == 'hang':
                return
            (profile / '.fixture-auth').write_text(identity())
            emit({'method': 'account/login/completed', 'params': {'loginId': 'wrong-login' if mode == 'stale' else 'fixture-login', 'success': True}})
        threading.Thread(target=complete, daemon=True).start()
    elif method == 'account/read':
        result = {'account': {'type': 'chatgpt', 'email': (profile / '.fixture-auth').read_text(), 'planType': 'pro'}} if signed_in() else {'account': None}
    elif method == 'model/list':
        result = {'data': [{'id': 'gpt-6.1-sol', 'defaultReasoningEffort': 'high', 'supportedReasoningEfforts': [{'reasoningEffort': 'high'}, {'reasoningEffort': 'xhigh'}]}]}
    elif method == 'account/rateLimits/read':
        # F6-A: cada leitura deixa um traço; o arquivo `rates` ao lado do executável troca a resposta e `rate-delay` a atrasa.
        with (base / 'rate-reads').open('a') as reads:
            reads.write('x\n')
        if setting('rate-delay'):
            time.sleep(float(setting('rate-delay')))
        result = json.loads(setting('rates')) if setting('rates') else {'rateLimits': {'primary': {'usedPercent': 23, 'windowDurationMins': 300, 'resetsAt': 1800000000}}}
    else:
        raise AssertionError(method)
    emit({'id': request['id'], 'result': result})
if helpers is not None:
    for helper in helpers.iterdir():
        helper.unlink()
    helpers.rmdir()
    if setting('scenario') == 'codex_arg0_nonzero':
        sys.exit(7)
    (profile.parent / '.fixture-eof-complete').write_text('done')
