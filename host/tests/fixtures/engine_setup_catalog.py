#!/usr/bin/python3
"""Fixture própria de catálogo: pausa só na etapa pedida, sem CLI/modelo/rede."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

profile = Path(os.environ['CLAUDE_CONFIG_DIR'])
assert Path(os.environ['HOME']) == profile.parent
stage = (profile / 'fixture-hang-stage').read_text()
for line in sys.stdin:
    request = json.loads(line)
    method = request.get('method')
    if method == ('initialize' if stage == 'initialize' else 'session/new'):
        child = subprocess.Popen(['/usr/bin/sleep', '600'])
        marker = profile / 'fixture-catalog-pids'
        pending = marker.with_suffix('.tmp')
        pending.write_text(str(os.getpid()) + '\n' + str(child.pid) + '\n')
        pending.replace(marker)
        time.sleep(600)
    if method == 'initialize':
        result = {'protocolVersion': 1, 'agentInfo': {'name': 'claude-agent-acp', 'version': 'fixture'}, 'agentCapabilities': {}}
    else:
        result = {}
    if 'id' in request:
        print(json.dumps({'id': request['id'], 'result': result}), flush=True)
