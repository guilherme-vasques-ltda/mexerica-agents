#!/usr/bin/env python3
"""CLI falso de motor (MEX-1291, F3). Nenhuma rede, modelo ou credencial real.

Uso (pelo script de cada CLI, escrito pelo teste): motor_cli.py <pasta> <nome> [argumentos do CLI].
O cenário vem do arquivo `<pasta>/scenario-<nome>`: logged_in (padrão), logged_out, unknown, timeout.
Quando o Hub lança o motor pelo caminho real (`backends::launch_candidates`), este arquivo vira o agente ACP:
registra quem foi lançado (binário, argumentos, modo) e passa a ser o agente de teste do modo daquele motor.
"""
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

base = Path(sys.argv[1])
name = sys.argv[2]
args = sys.argv[3:]


def scenario():
    path = base / f"scenario-{name}"
    return path.read_text().strip() if path.is_file() else "logged_in"


def say(text, code=0):
    print(text, flush=True)
    sys.exit(code)


# Comando de conferência sem cota, por CLI (plano contas-por-motor, tabela "Testar").
STATUS = {
    "claude": ["auth", "status"],
    "codex": ["login", "status"],
    "cursor-agent": ["status"],
    "agy": ["models"],
    "grok": ["models"],
    "opencode": ["auth", "list"],
}
# Modo ACP próprio de cada CLI (o `local` de cada motor em backends.rs) e o modo do agente falso que o imita.
ACP_LOCAL = {
    "cursor-agent": (["acp"], "standard"),
    "opencode": (["acp"], "standard"),
    "grok": (["agent", "stdio"], "grok"),
    "devin": (["acp"], "standard"),
    "kimi": (["acp"], "standard"),
    "cline": (["--acp"], "cline"),
}
# Motores sem modo ACP próprio entram pelo adaptador do registro (`npx -y <pacote>`): pacote -> (agente, modo).
ACP_NPX = {
    "fake-claude-acp@1.0.0": ("instructions_agent.py", "claude"),
    "fake-codex-acp@1.0.0": ("instructions_agent.py", "codex"),
    "fake-antigravity-acp@1.0.0": ("instructions_agent.py", "standard"),
}
# F6: contas gerenciadas lançam o adaptador pelo nome; cada conta responde conforme a pasta que recebeu.
ACP_ADAPTADOR = {"claude-agent-acp": ("cota_acp.py", "claude")}


def serve_acp(agent, mode):
    """Registra o lançamento e vira o agente ACP (mesma pasta de trabalho do bot)."""
    launch = {"binario": name, "args": args, "modo": mode, "caminho": os.environ.get("MEXERICA_FALSO_CAMINHO", "")}
    with (base / "launches.jsonl").open("a", encoding="utf-8") as log:
        log.write(json.dumps(launch) + "\n")
    Path("lancado-por.json").write_text(json.dumps(launch), encoding="utf-8")
    here = Path(__file__).resolve().parent
    os.execv(sys.executable, [sys.executable, "-X", "utf8", "-u", str(here / agent), mode])


def parent_name():
    """Nome do processo pai, de forma portátil; `None` quando não dá para identificar (nunca presume)."""
    ppid = os.getppid()
    comm = Path(f"/proc/{ppid}/comm")
    if comm.is_file():
        return comm.read_text().strip() or None
    ps = shutil.which("ps", path="/bin:/usr/bin")
    if ps:
        done = subprocess.run([ps, "-o", "comm=", "-p", str(ppid)], capture_output=True, text=True)
        out = done.stdout.strip()
        if done.returncode == 0 and out:
            return os.path.basename(out).lstrip("-")
    return None


# Comando de login por CLI: o Hub roda o binário com estes argumentos fixos.
LOGIN = {
    "claude": ["auth", "login"],
    "codex": ["login", "--device-auth"],
    "cursor-agent": ["login"],
    "agy": [],
    "grok": ["login", "--device-auth"],
    "opencode": ["auth", "login"],
    "devin": ["auth", "login"],
    "kimi": ["login"],
    "cline": ["auth"],
}

if args == ["--version"]:
    say(f"{name} 1.0.0")

# F6 (MEX-1391): contas gerenciadas. Cada uma é a pasta de configuração que o Hub entrega (`CLAUDE_CONFIG_DIR`); o
# falso responde a identidade e o uso a partir dela. `auth status --json` dá a identidade `<pasta>@example.invalid`
# (o teste calcula a mesma identidade de cota); `-p /usage` lê `uso-sessao` e `uso-semana` (percentuais) da pasta.
if name == "claude" and args == ["auth", "status", "--json"]:
    conta = Path(os.environ.get("CLAUDE_CONFIG_DIR", ".")).name
    if (Path(os.environ.get("CLAUDE_CONFIG_DIR", ".")) / "identidade-negada").exists():
        say(json.dumps({"loggedIn": False}), 1)
    say(json.dumps({"loggedIn": True, "authMethod": "claude.ai", "apiProvider": "firstParty",
                    "subscriptionType": "max", "email": f"{conta}@example.invalid", "orgId": "org"}))

if name == "claude" and args[:2] == ["-p", "/usage"]:
    pasta = Path(os.environ.get("CLAUDE_CONFIG_DIR", "."))
    linhas = []
    for arquivo, rotulo in (("uso-sessao", "Current session"), ("uso-semana", "Current week (all models)")):
        if (pasta / arquivo).is_file():
            percentual = (pasta / arquivo).read_text().strip()
            linhas.append(f"{rotulo}: {percentual}% used · resets 11pm (America/Sao_Paulo)")
    say(json.dumps({"result": "\n".join(linhas) or "sem dados"}))

if name in ACP_LOCAL and args == ACP_LOCAL[name][0]:
    serve_acp("instructions_agent.py", ACP_LOCAL[name][1])

if name in ACP_ADAPTADOR and not args:
    serve_acp(*ACP_ADAPTADOR[name])

if name == "npx" and args[:1] == ["-y"] and len(args) >= 2 and args[1] in ACP_NPX:
    serve_acp(*ACP_NPX[args[1]])

if args and args == STATUS.get(name):
    mode = scenario()
    if mode == "timeout":
        time.sleep(60)
        sys.exit(0)
    if mode == "unknown":
        # Resposta que nenhum parser conhece; o código de saída nunca diz "logado".
        say("???", 2 if name == "codex" else 0)
    logged = mode == "logged_in"
    if name == "claude":
        say(json.dumps({"loggedIn": logged}), 0 if logged else 1)
    if name == "codex":
        say("Logged in using ChatGPT" if logged else "Not logged in", 0 if logged else 1)
    if name == "cursor-agent":
        say("Logged in as fake@example.invalid" if logged else "Not logged in", 0 if logged else 1)
    if name == "grok":
        say("You are logged in with grok.com.\nmodel-fake-1" if logged else "You are not authenticated.", 0 if logged else 1)
    if name == "agy":
        say("gemini-fixture\tGemini fixture" if logged else "Error: not authenticated", 0 if logged else 1)
    say("model-fake-1\nmodel-fake-2" if logged else "Error: not authenticated", 0 if logged else 1)

if args == LOGIN.get(name):
    # Marca quem é o processo pai: um shell no meio quebra "sem shell" (§3.1 do plano).
    # Sem como identificar o pai, grava DESCONHECIDO e o teste falha: nunca vira "não é shell".
    (base / f"parent-{name}").write_text(parent_name() or "DESCONHECIDO")
    urls = {"cursor-agent": "https://cursor.com/login?challenge=fixture", "grok": "https://auth.x.ai/device", "agy": "https://accounts.google.com/o/oauth2/v2/auth?fixture=1"}
    print("Abra " + urls.get(name, "https://auth.openai.com/oauth/authorize") + " e digite o código:", flush=True)
    for line in sys.stdin:
        line = line.rstrip("\r\n")
        if line == "CODIGO-OK":
            (base / f"scenario-{name}").write_text("logged_in")
            print("Login concluído", flush=True)
            for command in sys.stdin:
                (base / f"command-after-login-{name}").write_text(command)
            sys.exit(0)
        # O fake nunca executa nada: só registra o que recebeu, para o teste conferir.
        with (base / f"inputs-{name}.log").open("a") as inputs:
            inputs.write(line + "\n")
        print("código inválido", flush=True)
    sys.exit(1)

say(f"{name}: comando desconhecido: {' '.join(args)}", 2)
