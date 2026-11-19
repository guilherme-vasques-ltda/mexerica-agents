#!/usr/bin/env python3
"""D330 F3: prova de teste, só em VM descartável. Não é código do produto.
Uso root: SCRIPT HELPER BIN_TESTES --vm-descartavel [--d520-only].
"""
import os
from pathlib import Path
import pwd
import shutil
import subprocess
import sys

assert os.geteuid() == 0 and len(sys.argv) in (4, 5) and sys.argv[3] == '--vm-descartavel'
assert len(sys.argv) == 4 or sys.argv[4] == '--d520-only'
d520_only = len(sys.argv) == 5
helper = Path('/opt/donnabot/bin/donnabot-motor-exec')
config = Path('/etc/donnabot/motor.conf')
base = Path('/var/lib/donnabot')
assert not helper.exists() and not config.exists() and not base.exists(), 'VM já configurada; recuso alterar'
created = []
policies = []
os.environ["PATH"] = "/usr/sbin:/sbin:" + os.environ.get("PATH", "")
try:
    for path in ([] if d520_only else [Path('/etc/codex'), Path('/etc/claude-code')]):
        if path.exists():
            saved = path.with_name(path.name + '.d330-f3-proof')
            assert not saved.exists()
            path.rename(saved)
            policies.append((path,saved))
    for name in ['d330-f3-hub', 'd330-f3-motor']:
        subprocess.run(['useradd', '--system', '--no-create-home', name], check=True)
        created.append(name)
    hub, motor = (pwd.getpwnam(name) for name in created)
    # O grupo primário do motor serve de grupo de trabalho descartável nesta prova.
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
    shutil.copyfile(sys.argv[1], helper)
    os.chown(helper, 0, hub.pw_gid)
    helper.chmod(0o750)
    subprocess.run(['setcap', 'cap_setuid,cap_setgid+ep', str(helper)], check=True)
    config.parent.mkdir(exist_ok=True)
    config.write_text(f'motor_user={motor.pw_name}\nhub_uid={hub.pw_uid}\nwork_gid={motor.pw_gid}\n')
    config.chmod(0o644)
    def as_hub():
        os.setgroups([motor.pw_gid])
        os.setresgid(hub.pw_gid, hub.pw_gid, hub.pw_gid)
        os.setresuid(hub.pw_uid, hub.pw_uid, hub.pw_uid)
        os.umask(0o077)
    status = subprocess.run([str(helper),'--cwd','/','--','/bin/cat','/proc/self/status'], preexec_fn=as_hub, capture_output=True,text=True,check=True).stdout
    identity = dict(line.split(':',1) for line in status.splitlines() if ':' in line)
    assert identity['Uid'].split() == [str(motor.pw_uid)]*4
    assert hub.pw_uid != motor.pw_uid
    assert identity['NoNewPrivs'].strip() == '1'
    assert all(int(identity[k].strip(),16) == 0 for k in ['CapPrm','CapEff','CapInh','CapAmb'])
    print(f'PASS Hub UID={hub.pw_uid}, Motor UID={motor.pw_uid}; capacidades zero, NoNewPrivs=1', flush=True)
    os.environ['DONNABOT_MOTOR_TEST_AUDIT_EXE'] = str(Path(sys.argv[1]).resolve().with_name('donnabot-host'))
    cases = ['d520_negacoes_novas_e_antigas_uid_real']
    if not d520_only:
        cases.insert(0, 'raizes_prova_uid_real')
    for case in cases:
        subprocess.run([sys.argv[2], '--ignored', '--exact', 'fork::motor_roots_tests::' + case, '--nocapture'],
                       preexec_fn=as_hub, check=True, timeout=120)
finally:
    for path,saved in policies:
        saved.rename(path)
    config.unlink(missing_ok=True)
    helper.unlink(missing_ok=True)
    if base.exists():
        shutil.rmtree(base)
    for name in reversed(created):
        subprocess.run(['userdel',name],check=True)
