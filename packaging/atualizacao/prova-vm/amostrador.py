#!/usr/bin/env python3
"""Amostra a cada 50 ms: cgroup da unidade do Hub, etapa do journal e seletor (root)."""
import json, os, sys, time
uid = int(sys.argv[1]); saida = open(sys.argv[2], "a", buffering=1)
base = f"/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service"
def populado():
    for rel in ("app.slice/donnabot-host.service", "donnabot-host.service"):
        try:
            if "populated 1" in open(f"{base}/{rel}/cgroup.events").read(): return 1
        except OSError: pass
    return 0
while True:
    try: etapa = json.load(open("/var/lib/donnabot/atualizacao/journal"))["etapa"]
    except Exception: etapa = None
    try: ativo = os.readlink("/opt/donnabot/releases/ativo")
    except OSError: ativo = None
    saida.write(json.dumps({"t": time.time(), "hub": populado(), "etapa": etapa, "ativo": ativo}) + "\n")
    time.sleep(0.05)
