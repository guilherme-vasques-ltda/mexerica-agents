#!/usr/bin/env python3
"""D169: ACP próprio de fixture. Sem rede, CLI, modelo ou credencial real."""
import json
import os
from pathlib import Path
import sys

session = 'same-adapter-session-id'
model = 'gpt-6.1-sol'
pending = None
index = Path(os.environ['CODEX_HOME']) / '.fixture-space-session.json'

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    method = request.get('method')
    params = request.get('params', {})
    with open('space-requests.jsonl', 'a', encoding='utf-8') as log:
        log.write(json.dumps({'method': method, 'params': params, 'cwd': os.getcwd(), 'profile': str(index.parent), 'selectedModel': model}) + '\n')
    if method == 'session/cancel' and pending is not None:
        emit({'method': 'session/update', 'params': {'sessionId': session, 'update': {'sessionUpdate': 'agent_message_chunk', 'content': {'type': 'text', 'text': 'LATE_A_ONLY'}}}})
        emit({'id': pending, 'result': {'stopReason': 'cancelled'}})
        pending = None
        continue
    if 'id' not in request:
        continue
    if method == 'initialize':
        result = {'protocolVersion': 1, 'agentInfo': {'name': 'codex-acp', 'version': 'fixture'}, 'agentCapabilities': {'loadSession': True}}
    elif method in ('session/new', 'session/load'):
        if method == 'session/load':
            assert json.loads(index.read_text())['cwd'] == os.getcwd(), 'SID raw resolveu contexto de outra conversa'
        else:
            index.write_text(json.dumps({'cwd': os.getcwd(), 'profile': str(index.parent), 'selectedModel': model}))
        assert params['cwd'] == os.getcwd()
        servers = params.get('mcpServers', [])
        personal = '00000000-0000-4000-8000-000000000001' in Path.cwd().parts
        assert {s['name'] for s in servers} == ({'connectors', 'chat', 'team', 'memory', 'tasks'} if personal else set()), 'MCPs divergiram do contexto da fixture'
        result = {'sessionId': session, 'configOptions': [
            {'id': 'mode', 'category': 'mode', 'currentValue': 'workspace-write', 'options': [{'value': 'workspace-write'}]},
            {'id': 'model', 'category': 'model', 'currentValue': 'gpt-6.1-sol', 'options': [{'value': 'gpt-6.1-sol'}, {'value': 'gpt-fixture-b'}]}]}
    elif method == 'session/prompt':
        text = params['prompt'][0]['text']
        assert 'R1_PERSONAL_PROMPT_334C' not in text
        files = []
        for item in text.splitlines():
            if item.startswith('- ') and '/uploads/' in item:
                path = Path(item[2:])
                assert path.resolve().is_relative_to(Path.cwd() / 'uploads')
                files.append({'path': str(path), 'text': path.read_text()})
        with open('space-requests.jsonl', 'a', encoding='utf-8') as log:
            log.write(json.dumps({'method': 'fixture/consumed', 'text': text, 'attachedFiles': files, 'selectedModel': model, 'profile': str(index.parent), 'cwd': os.getcwd()}) + '\n')
        if 'R1_RATE_LIMIT' in text and not Path('fixture-release-limit').exists():
            emit({'id': request['id'], 'error': {'code': -32603, 'message': 'Internal error', 'data': {'codex_error_info': 'UsageLimitExceeded', 'retry_after_seconds': 3600}}})
            continue
        if 'HOLD_A_ONLY' in text or ('O limite da assinatura interrompeu' in text and Path('fixture-hold-retry').exists()):
            pending = request['id']
            continue
        marker = 'CONTEXT_MARKER_B' if 'CONTEXT_MARKER_B' in text else 'CONTEXT_MARKER_A'
        emit({'method': 'session/update', 'params': {'sessionId': session, 'update': {'sessionUpdate': 'agent_message_chunk', 'content': {'type': 'text', 'text': marker}}}})
        result = {'stopReason': 'end_turn'}
    elif method in ('session/set_config_option', 'session/set_model'):
        if params.get('configId') == 'model':
            model = params['value']
        elif method == 'session/set_model':
            model = params['modelId']
        result = {'configOptions': [{'id': 'mode', 'category': 'mode', 'currentValue': 'workspace-write'}]}
    else:
        result = {}
    emit({'id': request['id'], 'result': result})
