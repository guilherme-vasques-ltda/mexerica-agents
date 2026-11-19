#!/usr/bin/env python3
"""Prova de dois UIDs: somente em contêiner descartável já configurado para motor_exec.

Requer /mnt/test, /usr/local/bin/node, usuário mxhub e a configuração real do
ajudante. A fixture cria arquivos sintéticos em /tmp; nunca usa dados reais.
"""
import os, pwd, subprocess, sys
assert os.geteuid() == 0 and os.environ.get('MEXERICA_PROOF_DESCARTAVEL') == '1'
hub = pwd.getpwnam('mxhub')
root = '/tmp/mxhub-private'
os.mkdir(root, 0o700)
os.chown(root, hub.pw_uid, hub.pw_gid)
secret = root + '/sentinela'
with open(secret, 'w') as f:
    f.write('sentinela-exclusiva-hub')
os.chown(secret, hub.pw_uid, hub.pw_gid)
os.chmod(secret, 0o600)
script = r'''const fs = require('fs');
let negada = false;
try { fs.readFileSync('/tmp/mxhub-private/sentinela'); } catch (e) { negada = e.code === 'EACCES'; }
let fechado = false;
try { fechado = fs.readlinkSync('/proc/self/fd/9') !== '/tmp/mxhub-private/sentinela'; } catch { fechado = true; }
const h = process.env.HOME;
const dirs = [h, process.env.XDG_CONFIG_HOME, process.env.XDG_DATA_HOME, process.env.XDG_CACHE_HOME, process.env.XDG_STATE_HOME];
const privateDirs = dirs.every(d => { const st = fs.statSync(d); return (st.mode & 0o777) === 0o700 && st.uid === process.getuid(); });
let gravavel = false;
try { fs.writeFileSync(h + '/prova', 'ok'); gravavel = true; } catch {}
console.log(JSON.stringify({sentinela_negada: negada, descritor_fechado: fechado, home_privado: privateDirs, home_gravavel: gravavel, ambiente_minimo: process.env.PROVA_HUB_SECRET === undefined}));
process.stdin.resume(); process.stdin.on('end', () => process.exit(0));
'''
with open('/tmp/mx-probe.cjs', 'w') as f: f.write(script)
os.chmod('/tmp/mx-probe.cjs', 0o644)
fd = os.open(secret, os.O_RDONLY)
os.dup2(fd, 9, inheritable=True)
if fd != 9: os.close(fd)
def as_hub():
    os.setgroups([])
    os.setresgid(hub.pw_gid, hub.pw_gid, hub.pw_gid)
    os.setresuid(hub.pw_uid, hub.pw_uid, hub.pw_uid)
result = subprocess.run(['/mnt/test', '--exact', 'fork::motor_mexerica::tests::identidade_real_nega_sentinela_e_isola_descritores', '--ignored', '--nocapture'],
    env={'PATH': '/usr/local/bin:/usr/bin:/bin', 'HOME': root, 'MEXERICA_MOTOR_ENTRY': '/tmp/mx-probe.cjs', 'MEXERICA_MOTOR': '1', 'DONNABOT_MEX_NODE_TEST': '/usr/local/bin/node', 'PROVA_HUB_SECRET': 'valor-sintetico'},
    preexec_fn=as_hub, pass_fds=(9,), timeout=90, capture_output=True, text=True)
print(result.stdout, end='')
print(result.stderr, end='', file=sys.stderr)
assert result.returncode == 0 and '1 passed; 0 failed' in result.stdout, 'prova não foi executada'
