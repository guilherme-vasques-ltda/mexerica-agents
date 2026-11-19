#!/usr/bin/env python3
"""D169: ACP próprio de fixture. Sem rede, CLI, modelo ou credencial real."""
import json
import os
from pathlib import Path
import sys
import subprocess
import select

session = 'same-adapter-session-id'
model = 'gpt-6.1-sol'
pending = None
servers = []
permission_prompt = None
index = Path(os.environ['CODEX_HOME']) / '.fixture-space-session.json'

def emit(value):
    print(json.dumps(value), flush=True)

def tool(server, name, arguments, expected_error=None):
    definition = next(s for s in servers if s['name'] == server)
    env = os.environ.copy()
    env.update({e['name']: e['value'] for e in definition.get('env', [])})
    child = subprocess.Popen([definition['command']] + definition.get('args', []), env=env,
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    try:
        child.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call',
                                     'params': {'name': name, 'arguments': arguments}}) + '\n')
        child.stdin.flush()
        assert select.select([child.stdout], [], [], 25)[0], 'MCP fixture sem resposta em 25 s'
        result = json.loads(child.stdout.readline())['result']
        with open('space-requests.jsonl', 'a', encoding='utf-8') as log:
            log.write(json.dumps({'method': 'fixture/tool', 'server': server, 'name': name, 'result': result}) + '\n')
        if expected_error is not None:
            assert result.get('isError') and expected_error in result['content'][0]['text'], result
            return None
        assert not result.get('isError'), result
        return json.loads(result['content'][0]['text'])
    finally:
        child.stdin.close()
        child.terminate()
        child.wait(timeout=5)

for line in sys.stdin:
    request = json.loads(line)
    if request.get('id') == 'permission-fixture' and permission_prompt is not None:
        emit({'method': 'session/update', 'params': {'sessionId': session, 'update': {'sessionUpdate': 'agent_message_chunk', 'content': {'type': 'text', 'text': 'CONTEXT_MARKER_A'}}}})
        emit({'id': permission_prompt, 'result': {'stopReason': 'end_turn'}})
        permission_prompt = None
        continue
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
        if 'spaces' in Path.cwd().parts:
            assert not servers or {s['name'] for s in servers} in ({'connectors', 'memory', 'chat', 'team', 'tasks'}, {'connectors', 'memory', 'tasks'})
        result = {'sessionId': session, 'configOptions': [
            {'id': 'mode', 'category': 'mode', 'currentValue': 'workspace-write', 'options': [{'value': 'workspace-write'}]},
            {'id': 'model', 'category': 'model', 'currentValue': 'gpt-6.1-sol', 'options': [{'value': 'gpt-6.1-sol'}, {'value': 'gpt-fixture-b'}]}]}
    elif method == 'session/prompt':
        text = params['prompt'][0]['text']
        assert 'PERSONAL_PROMPT_SENTINEL' not in text
        files = []
        for item in text.splitlines():
            if item.startswith('- ') and '/uploads/' in item:
                path = Path(item[2:])
                assert path.resolve().is_relative_to(Path.cwd() / 'uploads')
                files.append({'path': str(path), 'text': path.read_text()})
        with open('space-requests.jsonl', 'a', encoding='utf-8') as log:
            log.write(json.dumps({'method': 'fixture/consumed', 'text': text, 'attachedFiles': files, 'selectedModel': model, 'profile': str(index.parent), 'cwd': os.getcwd()}) + '\n')
        if 'TOOL_READ' in text:
            tool('memory', 'search_memory', {'query': 'astronomia'})
        if 'TOOL_FLOW' in text:
            tool('chat', 'send_message', {'text': 'MENSAGEM_ADICIONAL'})
            tool('chat', 'react', {'emoji': '👍'})
            Path('arquivo-adicional.txt').write_text('ARQUIVO_ADICIONAL')
            tool('chat', 'send_file', {'path': 'arquivo-adicional.txt'})
            tool('memory', 'remember', {'text': 'A pessoa gosta de astronomia.'}, 'codex_memory_mutation_refused')
            tool('memory', 'search_memory', {'query': 'astronomia'})
            if 'TARGET_BOT=' in text:
                target = text.split('TARGET_BOT=')[-1].split()[0]
                tool('team', 'ask_bot', {'botId': target, 'message': 'CONTEXT_MARKER_B'})
                tool('team', 'message_bot', {'botId': target, 'message': 'CONTEXT_MARKER_B'})
            permission_prompt = request['id']
            emit({'id': 'permission-fixture', 'method': 'session/request_permission', 'params': {'sessionId': session,
                'toolCall': {'toolCallId': 'permission-1', 'title': 'Permissão da adicional', 'kind': 'execute', 'rawInput': {'command': 'fixture'}},
                'options': [{'optionId': 'allow', 'name': 'Permitir', 'kind': 'allow_once'}, {'optionId': 'deny', 'name': 'Negar', 'kind': 'reject_once'}]}})
            continue
        if 'HOLD_A_ONLY' in text:
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
