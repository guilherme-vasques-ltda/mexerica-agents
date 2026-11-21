#!/usr/bin/env python3
"""Repositório TUF + manifesto Ed25519 de TESTE (chaves geradas e mantidas só nesta VM).
uso: repo.py init DIR
     repo.py publicar DIR SEQ PISO HUB AJUDANTE [--hash-errado]
     repo.py rotacionar DIR VERSAO [--so-nova]   (troca a chave raiz e a de timestamp)"""
import base64, hashlib, json, os, subprocess, sys, time, datetime
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

DOMINIO = b"mexerica/atualizacao-privilegiada/manifesto/v1\0"
canon = lambda v: json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
sha = lambda b: hashlib.sha256(b).hexdigest()
expira = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=30)).strftime("%Y-%m-%dT%H:%M:%SZ")
cmd, d = sys.argv[1], sys.argv[2]
chaves = os.path.join(d, "chaves"); repo = os.path.join(d, "www", "repo")
os.makedirs(chaves, mode=0o700, exist_ok=True); os.makedirs(repo, exist_ok=True)
def priv(nome):
    path = os.path.join(chaves, nome)
    if not os.path.exists(path):
        k = Ed25519PrivateKey.generate()
        with open(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb") as f:
            f.write(k.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption()))
    return Ed25519PrivateKey.from_private_bytes(open(path, "rb").read())
pub = lambda k: k.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
kid = lambda k: sha(pub(k))
def envelope(signed, *ks):
    return json.dumps({"signed": signed, "signatures": [{"keyid": kid(k), "sig": k.sign(canon(signed)).hex()} for k in ks]}).encode()
estado_path = os.path.join(d, "estado-repo.json")
estado = json.load(open(estado_path)) if os.path.exists(estado_path) else {"root": "root1", "timestamp": "timestamp1", "versao_raiz": 1}
def salvar(): json.dump(estado, open(estado_path, "w"))
def raiz_signed(versao, root, ts):
    ks = {"root": priv(root), "timestamp": priv(ts), "snapshot": priv("snapshot"), "targets": priv("targets")}
    return {"_type": "root", "spec_version": "1.0.0", "version": versao, "expires": expira,
            "keys": {kid(k): {"keytype": "ed25519", "scheme": "ed25519", "keyval": {"public": pub(k).hex()}} for k in ks.values()},
            "roles": {n: {"keyids": [kid(k)], "threshold": 1} for n, k in ks.items()}}
if cmd == "init":
    b = envelope(raiz_signed(1, "root1", "timestamp1"), priv("root1"))
    open(os.path.join(repo, "1.root.json"), "wb").write(b); salvar()
    print(json.dumps({"raiz": "1.root.json", "chave_manifesto": base64.b64encode(pub(priv("manifesto"))).decode()}))
elif cmd == "rotacionar":
    versao = int(sys.argv[3]); nova_raiz, novo_ts = f"root{versao}", f"timestamp{versao}"
    assinantes = [priv(nova_raiz)] if "--so-nova" in sys.argv else [priv(estado["root"]), priv(nova_raiz)]
    b = envelope(raiz_signed(versao, nova_raiz, novo_ts), *assinantes)
    open(os.path.join(repo, f"{versao}.root.json"), "wb").write(b)
    if "--so-nova" not in sys.argv:
        estado.update({"root": nova_raiz, "timestamp": novo_ts, "versao_raiz": versao}); salvar()
    print(f"raiz {versao} publicada")
elif cmd == "publicar":
    seq, piso, hub, aj = int(sys.argv[3]), int(sys.argv[4]), open(sys.argv[5], "rb").read(), open(sys.argv[6], "rb").read()
    agora = int(time.time())
    versao = subprocess.check_output([sys.argv[5], "--version"], text=True).strip().split()[-1]
    m = {"produto": "mexerica", "canal": "estavel", "arquitetura": "x86_64", "versao": versao, "sequencia": seq,
         "inicio": agora - 60, "expiracao": agora + 7200,
         "hub": {"nome": "donnabot-host", "tamanho": len(hub), "sha256": sha(hub) if "--hash-errado" not in sys.argv else "0" * 64},
         "ajudante": {"nome": "donnabot-motor-exec", "tamanho": len(aj), "sha256": sha(aj)},
         "sequencia_minima_reversivel": piso, "wrapper_minimo": "0.1.0"}
    mb = json.dumps(m, separators=(",", ":")).encode()
    ref = lambda b, v=None: {**({"version": v} if v else {}), "length": len(b), "hashes": {"sha256": sha(b)}}
    targets = envelope({"_type": "targets", "spec_version": "1.0.0", "version": seq, "expires": expira, "targets": {"manifesto.json": ref(mb)}}, priv("targets"))
    snap = envelope({"_type": "snapshot", "spec_version": "1.0.0", "version": seq, "expires": expira, "meta": {"targets.json": ref(targets, seq)}}, priv("snapshot"))
    ts = envelope({"_type": "timestamp", "spec_version": "1.0.0", "version": seq, "expires": expira, "meta": {"snapshot.json": ref(snap, seq)}}, priv(estado["timestamp"]))
    for n, b in {"manifesto.json": mb, "manifesto.sig": priv("manifesto").sign(DOMINIO + mb), "donnabot-host": hub, "donnabot-motor-exec": aj,
                 "targets.json": targets, "snapshot.json": snap, "timestamp.json": ts}.items():
        open(os.path.join(repo, n), "wb").write(b)
    print(f"publicada sequência {seq}")
