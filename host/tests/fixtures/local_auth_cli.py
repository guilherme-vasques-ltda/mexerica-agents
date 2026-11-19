#!/usr/bin/python3
"""ACP local para conferir descoberta, chave e authenticate; sem rede ou conta real."""
import json
import os
import sys
from pathlib import Path

marker = Path(sys.argv[1])
for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    method = request['method']
    result = {}
    error = None
    if method == 'initialize':
        result = {'protocolVersion': 1, 'agentCapabilities': {}, 'authMethods': [
            {'id': 'fixture-key', 'name': 'Chave fictícia', 'type': 'env_var',
             'vars': [{'name': 'FIXTURE_API_KEY', 'label': 'Chave fictícia'}]},
            {'id': 'fixture-login', 'name': 'Login fictício'},
        ]}
    elif method == 'authenticate':
        assert request['params']['methodId'] == 'fixture-login'
        marker.write_text('autorizado')
    elif method == 'session/new':
        if marker.exists() or os.environ.get('FIXTURE_API_KEY') == 'fixture-value':
            result = {'sessionId': 'fixture-session'}
        else:
            error = {'code': -32000, 'message': 'Login necessário'}
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'],
                      **({'error': error} if error else {'result': result})}), flush=True)
