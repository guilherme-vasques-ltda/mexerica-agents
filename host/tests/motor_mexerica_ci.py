#!/usr/bin/env python3
"""Prova de dois UIDs do Motor Mexerica NO CI (MEX-1367, P3 da revisão da fatia 1).

Uso (root, VM descartável do CI): sudo -n python3 motor_mexerica_ci.py AJUDANTE BIN_TESTES --vm-descartavel

Configura sozinha dois usuários (Hub e motor), a configuração root do ajudante e
as pastas do motor; roda o teste `identidade_real_nega_sentinela_e_isola_descritores`
como o UID do Hub, com a sentinela do Hub, um descritor herdável (fd 9) e um
ambiente com segredo sintético; e desfaz tudo no fim. Falha fechada em TODO pré-requisito
ausente (root, VM já configurada, Node acessível ao motor, teste não executado):
nunca pula e nunca devolve verde sem rodar a prova.
"""
import os
import pwd
import shutil
import subprocess
import sys
from pathlib import Path

assert os.geteuid() == 0, 'a prova de dois UIDs exige root (sudo -n); sem ele o CI REPROVA, não pula'
assert len(sys.argv) == 4 and sys.argv[3] == '--vm-descartavel', 'uso: SCRIPT AJUDANTE BIN_TESTES --vm-descartavel'
helper_src, tests_src = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
helper = Path('/opt/donnabot/bin/donnabot-motor-exec')
config = Path('/etc/donnabot/motor.conf')
base = Path('/var/lib/donnabot')
proof = Path('/var/lib/donnabot-prova-mexerica')
assert not helper.exists() and not config.exists() and not base.exists() and not proof.exists(), \
    'VM já configurada para o motor; recuso alterar'
node = shutil.which('node')
assert node, 'Node.js ausente no PATH da VM de CI'
node = str(Path(node).resolve())
os.environ['PATH'] = '/usr/sbin:/sbin:' + os.environ.get('PATH', '')
created = []
try:
    for name in ['mx-ci-hub', 'mx-ci-motor']:
        subprocess.run(['useradd', '--system', '--no-create-home', name], check=True)
        created.append(name)
    hub, motor = (pwd.getpwnam(name) for name in created)
    assert hub.pw_uid != motor.pw_uid and hub.pw_gid != motor.pw_gid
    # O motor também precisa executar o Node: a prova só vale com o caminho aberto a ele.
    subprocess.run(['setpriv', '--reuid', str(motor.pw_uid), '--regid', str(motor.pw_gid), '--clear-groups', node, '--version'],
                   check=True, capture_output=True)
    base.mkdir(mode=0o755)
    work = base / 'trabalho'
    work.mkdir()
    os.chown(work, hub.pw_uid, motor.pw_gid)
    work.chmod(0o2770)
    home = base / 'motor'
    home.mkdir()
    os.chown(home, motor.pw_uid, motor.pw_gid)
    home.chmod(0o700)
    helper.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(helper_src, helper)
    os.chown(helper, 0, hub.pw_gid)
    helper.chmod(0o750)
    if shutil.which('setcap'):
        subprocess.run(['setcap', 'cap_setuid,cap_setgid+ep', str(helper)], check=True)
    else:  # sem libcap: setuid root dá ao ajudante a mesma troca de identidade
        helper.chmod(0o4750)
    config.parent.mkdir(exist_ok=True)
    config.write_text(f'motor_user={motor.pw_name}\nhub_uid={hub.pw_uid}\nwork_gid={motor.pw_gid}\n')
    config.chmod(0o644)

    proof.mkdir(mode=0o755)
    private = proof / 'hub-privado'
    private.mkdir(mode=0o700)
    os.chown(private, hub.pw_uid, hub.pw_gid)
    secret = private / 'sentinela'
    secret.write_text('sentinela-exclusiva-hub')
    os.chown(secret, hub.pw_uid, hub.pw_gid)
    secret.chmod(0o600)
    tests = proof / 'testes'
    shutil.copyfile(tests_src, tests)
    tests.chmod(0o755)
    probe = proof / 'sonda.cjs'
    probe.write_text(rf'''const fs = require('fs');
let negada = false;
try {{ fs.readFileSync('{secret}'); }} catch (e) {{ negada = e.code === 'EACCES'; }}
let fechado = false;
try {{ fechado = fs.readlinkSync('/proc/self/fd/9') !== '{secret}'; }} catch {{ fechado = true; }}
const h = process.env.HOME;
const dirs = [h, process.env.XDG_CONFIG_HOME, process.env.XDG_DATA_HOME, process.env.XDG_CACHE_HOME, process.env.XDG_STATE_HOME];
const privateDirs = dirs.every(d => {{ const st = fs.statSync(d); return (st.mode & 0o777) === 0o700 && st.uid === process.getuid(); }});
let gravavel = false;
try {{ fs.writeFileSync(h + '/prova', 'ok'); gravavel = true; }} catch {{}}
const cwd = process.cwd();
const cst = fs.statSync(cwd);
const cwdPrivado = cwd === h + '/work' && (cst.mode & 0o777) === 0o700 && cst.uid === process.getuid();
console.log(JSON.stringify({{sentinela_negada: negada, descritor_fechado: fechado, home_privado: privateDirs, home_gravavel: gravavel, cwd_privado: cwdPrivado, ambiente_minimo: process.env.PROVA_HUB_SECRET === undefined}}));
process.stdin.resume(); process.stdin.on('end', () => process.exit(0));
''')
    probe.chmod(0o644)

    fd = os.open(secret, os.O_RDONLY)
    os.dup2(fd, 9, inheritable=True)
    if fd != 9:
        os.close(fd)

    def as_hub():
        os.setgroups([motor.pw_gid])
        os.setresgid(hub.pw_gid, hub.pw_gid, hub.pw_gid)
        os.setresuid(hub.pw_uid, hub.pw_uid, hub.pw_uid)
        os.umask(0o077)

    result = subprocess.run(
        [str(tests), '--exact', 'fork::motor_mexerica::tests::identidade_real_nega_sentinela_e_isola_descritores',
         '--ignored', '--nocapture'],
        env={'PATH': '/usr/local/bin:/usr/bin:/bin', 'HOME': str(private), 'MEXERICA_MOTOR_ENTRY': str(probe),
             'MEXERICA_MOTOR': '1', 'DONNABOT_MEX_NODE_TEST': node, 'PROVA_HUB_SECRET': 'valor-sintetico'},
        preexec_fn=as_hub, pass_fds=(9,), timeout=120, capture_output=True, text=True)
    print(result.stdout, end='')
    print(result.stderr, end='', file=sys.stderr)
    assert result.returncode == 0 and '1 passed; 0 failed' in result.stdout, 'a prova de dois UIDs não foi executada'
    print('PASS prova de dois UIDs do Motor Mexerica (sentinela negada, fd 9 fechado, HOME 0700, ambiente mínimo)')
finally:
    config.unlink(missing_ok=True)
    helper.unlink(missing_ok=True)
    for path in (base, proof):
        if path.exists():
            shutil.rmtree(path)
    for name in reversed(created):
        subprocess.run(['userdel', name], check=False)
