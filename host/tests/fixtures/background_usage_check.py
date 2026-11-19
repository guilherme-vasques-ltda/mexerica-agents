"""Verificação manual, sem mostrar conta nem cota: cliente Claude do servidor."""
import json
import os
import subprocess
import tempfile
from pathlib import Path

allowed = ("PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "CLAUDE_CONFIG_DIR")
env = {name: os.environ[name] for name in allowed if name in os.environ}
env["CLAUDE_CODE_DISABLE_CLAUDE_MDS"] = "1"
env["CLAUDE_CODE_DISABLE_AUTO_MEMORY"] = "1"
env["DISABLE_AUTOUPDATER"] = "1"
with tempfile.TemporaryDirectory(prefix="donnabot-usage-") as directory:
    auth = subprocess.run(["claude", "auth", "status", "--json"], cwd=directory, env=env,
                          stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=20, check=True)
    auth_status = json.loads(auth.stdout)
    print("tipo de autenticação:", auth_status.get("authMethod"))
    print("tipo de plano:", auth_status.get("subscriptionType"))
    result = subprocess.run(
        ["claude", "-p", "/usage", "--output-format", "json", "--no-session-persistence",
         "--setting-sources", "", "--strict-mcp-config", "--tools", "", "--settings",
         '{"disableAllHooks":true}'],
        cwd=directory, env=env, stdin=subprocess.DEVNULL, capture_output=True,
        text=True, timeout=50, check=True,
    )
    text = json.loads(result.stdout).get("result", "")
    print("janela de sessão:", "Current session:" in text)
    print("janela semanal:", "Current week (all models):" in text)
    print("rótulos reconhecidos:", [line.split(":", 1)[0].strip() for line in text.splitlines()
                                    if "% used" in line and ":" in line])
    print("outros rótulos de janela:", [line.split(":", 1)[0].strip() for line in text.splitlines()
                                          if line.strip().lower().startswith("current ") and ":" in line])
    marker = Path(directory) / "marcador"
    attempt = subprocess.run(
        ["claude", "-p", f"Crie o arquivo {marker} com o texto teste e responda ok.",
         "--model", "haiku", "--output-format", "json", "--no-session-persistence",
         "--setting-sources", "", "--strict-mcp-config", "--tools", "", "--settings",
         '{"disableAllHooks":true}'],
        cwd=directory, env=env, stdin=subprocess.DEVNULL, capture_output=True,
        text=True, timeout=50, check=True,
    )
    answer = json.loads(attempt.stdout)
    print("marcador criado:", marker.exists())
    print("modelos efetivos informados:", sorted(answer.get("modelUsage", {}).keys()))
    after = subprocess.run(
        ["claude", "-p", "/usage", "--output-format", "json", "--no-session-persistence",
         "--setting-sources", "", "--strict-mcp-config", "--tools", "", "--settings",
         '{"disableAllHooks":true}'],
        cwd=directory, env=env, stdin=subprocess.DEVNULL, capture_output=True,
        text=True, timeout=50, check=True,
    )
    print("janela de sessão após chamada:", "Current session:" in json.loads(after.stdout).get("result", ""))
