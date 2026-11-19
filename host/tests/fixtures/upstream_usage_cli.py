#!/usr/bin/python3
"""CLI controlado: modela /usage e MCP, não certifica o Claude Code real."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

home = Path(os.environ["HOME"])
profile = Path(os.environ["CLAUDE_CONFIG_DIR"])
args = sys.argv[1:]
(home / "invocation.json").write_text(json.dumps({
    "args": args, "env": dict(os.environ), "cwd": os.getcwd(),
    "stdin": sys.stdin.read(), "pid": os.getpid(),
    "account": json.loads((profile / "auth.json").read_text()),
}))
config = json.loads((profile / "mcp.json").read_text())
if "--mcp-config" in args:
    override = json.loads(args[args.index("--mcp-config") + 1])
    if "--strict-mcp-config" in args:
        config = override
    else:
        config["mcpServers"].update(override["mcpServers"])
for server in config["mcpServers"].values():
    subprocess.run([server["command"], *server["args"]], check=True)
if args[:2] != ["-p", "/usage"]:
    (home / "model-turn").touch()
if "--no-session-persistence" not in args:
    (profile / "session.json").write_text("conversa de fixture")
scenario = (home / "scenario").read_text()
if scenario == "timeout":
    # O teste só vence seu deadline depois deste sinal; não há saída natural por tempo.
    ready = home / "timeout-ready.json"
    pending = home / "timeout-ready.tmp"
    pending.write_text(json.dumps({"pid": os.getpid()}))
    pending.replace(ready)
    while not (home / "timeout-release").is_file():
        time.sleep(.01)
elif scenario == "invalid":
    print('{"result":"relatório indisponível"}')
else:
    print(json.dumps({"result": "Current session: 23% used · resets Nov 4 at 10am (UTC)\nCurrent week (all models): 41% used"}))
