"""Prova manual do adaptador ACP real; não imprime mensagens nem credenciais."""
import json
import os
from pathlib import Path
import select
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time


with tempfile.TemporaryDirectory(prefix="donnabot-acp-") as root:
    path = Path(root)
    mcp_marker = path / "mcp-iniciado"
    tool_marker = path / "ferramenta-executada"
    hook_marker = path / "hook-executado"
    mcp_program = f"open({str(mcp_marker)!r}, 'w').write('x')"
    (path / ".mcp.json").write_text(json.dumps({"mcpServers": {"fixture": {
        "command": "python3", "args": ["-c", mcp_program]}}}))
    (path / ".claude").mkdir()
    (path / ".claude" / "settings.local.json").write_text(json.dumps({"hooks": {
        "SessionStart": [{"hooks": [{"type": "command", "command": "touch " + shlex.quote(str(hook_marker))}]}]}}))
    allowed = ("PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "CLAUDE_CONFIG_DIR")
    env = {name: os.environ[name] for name in allowed if name in os.environ}
    env.update({"npm_config_cache": str(path / "npm"), "DISABLE_AUTOUPDATER": "1",
                "CLAUDE_CODE_DISABLE_CLAUDE_MDS": "1", "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1",
                "CLAUDE_CODE_EXECUTABLE": shutil.which("claude") or "claude"})
    process = subprocess.Popen(
        ["npx", "--yes", "@agentclientprotocol/claude-agent-acp@0.94.0"],
        cwd=path, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, start_new_session=True, bufsize=0,
    )
    deadline = time.monotonic() + 120
    notifications = []

    def request(number, method, params):
        process.stdin.write((json.dumps({"jsonrpc": "2.0", "id": number,
                                         "method": method, "params": params}) + "\n").encode())
        process.stdin.flush()
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([process.stdout], [], [], remaining)[0]:
                raise TimeoutError("adaptador não concluiu a prova")
            line = process.stdout.readline()
            if not line:
                raise RuntimeError("adaptador encerrou antes da resposta")
            message = json.loads(line)
            if message.get("id") == number:
                if "error" in message:
                    raise RuntimeError("adaptador recusou a prova")
                return message["result"]
            notifications.append(message)

    try:
        initialized = request(1, "initialize", {"protocolVersion": 1,
                                   "clientCapabilities": {"fs": {"readTextFile": False,
                                                                  "writeTextFile": False},
                                                          "terminal": False}})
        print("conector:", initialized.get("agentInfo", {}).get("name"),
              initialized.get("agentInfo", {}).get("version"))
        session = request(2, "session/new", {"cwd": root, "mcpServers": [], "_meta": {
            "systemPrompt": "Responda brevemente.", "claudeCode": {"options": {
                "tools": [], "disallowedTools": ["*"], "strictMcpConfig": True,
                "persistSession": False, "settingSources": [], "model": "haiku",
                "permissionMode": "default", "allowDangerouslySkipPermissions": False,
                "settings": {"autoMemoryEnabled": False, "autoDreamEnabled": False,
                             "disableAllHooks": True}}}}})
        print("seleção anunciada:", [(o.get("id"), o.get("currentValue"))
                                     for o in session.get("configOptions", [])
                                     if o.get("category") == "model" or o.get("id") in ("model", "effort", "reasoning_effort")])
        selected = request(3, "session/set_config_option", {"sessionId": session["sessionId"],
                                                              "configId": "model", "value": "haiku"})
        selected = request(4, "session/set_config_option", {"sessionId": session["sessionId"],
                                                              "configId": "effort", "value": "low"})
        print("seleção confirmada:", [(o.get("id"), o.get("currentValue"))
                                      for o in selected.get("configOptions", [])
                                      if o.get("id") in ("model", "effort")])
        print("esforços anunciados:", [value.get("value") for option in selected.get("configOptions", [])
                                         if option.get("id") == "effort" for value in option.get("options", [])])
        if "--catalog-only" in sys.argv:
            raise SystemExit(0)
        result = request(5, "session/prompt", {"sessionId": session["sessionId"], "prompt": [
            {"type": "text", "text": f"Crie o arquivo {tool_marker} com o texto teste e responda ok."}]})
        tool_events = [m for m in notifications if m.get("params", {}).get("update", {}).get("sessionUpdate")
                       in ("tool_call", "tool_call_update")]
        models = result.get("_meta", {}).get("quota", {}).get("model_usage", [])
        print("MCP iniciado:", mcp_marker.exists())
        print("hook executado:", hook_marker.exists())
        print("marcador de ferramenta criado:", tool_marker.exists())
        print("eventos de ferramenta:", len(tool_events))
        print("conclusão:", result.get("stopReason"))
        print("modelos efetivos:", [row.get("model") for row in models])
        if mcp_marker.exists() or hook_marker.exists() or tool_marker.exists() or tool_events or result.get("stopReason") != "end_turn":
            raise RuntimeError("fronteira da prova falhou")
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
