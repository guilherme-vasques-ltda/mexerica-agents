#!/usr/bin/env python3
"""Adaptador ACP fictício: prova ambiente selecionado, sem CLI/modelo real."""
import json
import os
from pathlib import Path
import sys
import time
import uuid

def emit(value):
    print(json.dumps(value), flush=True)

profile = Path(os.environ.get('CODEX_HOME', '/fixture-missing'))
prefix = 'fixture-' + profile.parent.parent.name + ':'
session = ''

for line in sys.stdin:
    request = json.loads(line)
    method = request.get('method')
    if 'id' not in request:
        continue
    if method == 'initialize':
        result = {'protocolVersion': 1, 'agentInfo': {'name': 'codex-acp', 'version': 'fixture'}, 'agentCapabilities': {'loadSession': True}}
    elif method in ('session/new', 'session/load'):
        with open('acp-sessions.log', 'a') as log:
            log.write(method + '\n')
        # Sessões PR5: segura a abertura enquanto o teste derruba o Hub (reserva feita, permit ainda não).
        while os.path.exists('hold-session-new'):
            time.sleep(0.05)
        # Sessões PR5: a abertura falha enquanto existir o arquivo (falha de preparo, sem prompt).
        if method == 'session/new' and os.path.exists('fail-session-new'):
            emit({'id': request['id'], 'error': {'code': -32000, 'message': 'fixture: abertura recusada'}})
            continue
        if method == 'session/load':
            session = request['params']['sessionId']
            assert session.startswith(prefix), 'sessao cruzou contas'
        else:
            session = prefix + str(uuid.uuid4())
        result = {'sessionId': session, 'configOptions': [
            {'id': 'mode', 'category': 'mode', 'currentValue': 'workspace-write', 'options': [{'value': 'workspace-write'}]},
            {'id': 'model', 'category': 'model', 'currentValue': 'gpt-6.1-sol', 'options': [{'value': 'gpt-6.1-sol'}]}]}
    elif method == 'session/prompt':
        session = request['params']['sessionId']
        assert session.startswith(prefix), 'sessao cruzou contas'
        profile = Path(os.environ['CODEX_HOME'])
        assert Path(os.environ['HOME']) == profile.parent
        assert (profile / '.fixture-auth').is_file()
        assert not os.environ.get('OPENAI_API_KEY') and not os.environ.get('ANTHROPIC_API_KEY')
        account = profile.parent.parent.name
        # Sessões PR5: o texto que o Hub mandou, para provar o que o primeiro prompt de uma sessão leva.
        text = ''.join(part.get('text', '') for part in request['params'].get('prompt', []))
        with open('acp-prompts.jsonl', 'a') as log:
            log.write(json.dumps({'sessionId': session, 'text': text}) + '\n')
        emit({'method': 'session/update', 'params': {'sessionId': session, 'update': {'sessionUpdate': 'agent_message_chunk', 'content': {'type': 'text', 'text': 'PROFILE:' + account}}}})
        result = {'stopReason': 'end_turn'}
    elif method in ('session/set_config_option', 'session/set_model'):
        result = {'configOptions': [{'id': 'mode', 'category': 'mode', 'currentValue': 'workspace-write'}]}
    elif method == 'session/cancel':
        result = {}
    else:
        result = {}
    emit({'id': request['id'], 'result': result})
