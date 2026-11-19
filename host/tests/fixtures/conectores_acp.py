#!/usr/bin/env python3
"""ACP sintético 04: sem modelo, sandbox, segredo, autenticação ou rede."""
import json
from pathlib import Path
import sys

for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    method = request['method']
    mode = Path('scenario').read_text() if Path('scenario').is_file() else ''
    if method == 'initialize':
        result = {'agentInfo': {'name': 'fixture-acp', 'version': '1'}, 'agentCapabilities': {'loadSession': True, 'sessionCapabilities': {'close': {}}}}
        if mode == 'manual_error':
            result['agentCapabilities']['_meta'] = {'claudeCode': {}}
    elif method in ('session/new', 'session/load'):
        with Path('session-requests.jsonl').open('a') as log:
            log.write(json.dumps(request) + '\n')
        if mode == 'new_error' or (mode == 'load_error' and method == 'session/load'):
            print(json.dumps({'id': request['id'], 'error': {'code': -32603, 'message': 'fixture'}}), flush=True)
            continue
        result = {'sessionId': request['params'].get('sessionId', 'fixture-acp')}
        if mode == 'manual_error':
            result['configOptions'] = [{'id': 'mode', 'category': 'mode', 'currentValue': 'blocked'}]
    elif (method == 'session/set_model' and mode == 'model_error') or (method == 'session/set_config_option' and mode == 'manual_error') or (method == 'session/close' and mode == 'close_error'):
        print(json.dumps({'id': request['id'], 'error': {'code': -32603, 'message': 'fixture'}}), flush=True)
        continue
    else:
        result = {}
    print(json.dumps({'id': request['id'], 'result': result}), flush=True)
