#!/usr/bin/python3
"""Codex fictício: só initialize/initialized/account/read; nunca executa IA."""
import json
import os
import sys

assert sys.argv[1:] == ['app-server']
profile = os.environ['CODEX_HOME']
assert os.environ['HOME'] == profile
expected = ['initialize', 'initialized', 'account/read']
for method in expected:
    line = sys.stdin.readline()
    request = json.loads(line)
    assert request['method'] == method
    with open(os.path.join(profile, 'requests.jsonl'), 'a', encoding='utf8') as transcript:
        transcript.write(json.dumps(request) + '\n')
    if method == 'initialize':
        print(json.dumps({'id': 1, 'result': {}}), flush=True)
    elif method == 'account/read':
        assert request['params'] == {'refreshToken': False}
        with open(os.path.join(profile, 'fixture-state.json'), encoding='utf8') as state:
            result = json.load(state)
        print(json.dumps({'id': 2, 'result': result}), flush=True)
