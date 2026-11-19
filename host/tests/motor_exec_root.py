#!/usr/bin/env python3
"""D330 F2: sudo python3 SCRIPT AJUDANTE --vm-descartavel [--hub BIN_HUB [--tests BIN_TESTES]].
Somente VM descartável com root, setcap, systemd e filesystem sem nosuid.
"""
import json
import os
from pathlib import Path
import pwd
import shutil
import signal
import subprocess
import sys
import tempfile
import time

assert os.geteuid() == 0 and sys.argv[2:3] == ['--vm-descartavel']
assert len(sys.argv) in [3, 5, 7]
assert len(sys.argv) == 3 or sys.argv[3] == '--hub'
assert len(sys.argv) != 7 or sys.argv[5] == '--tests'
hub_binary = Path(sys.argv[4]).resolve() if len(sys.argv) >= 5 else None
test_binary = Path(sys.argv[6]).resolve() if len(sys.argv) == 7 else None
service_started = False
config = Path('/etc/donnabot/motor.conf')
assert not config.exists(), 'VM já configurada; recuso alterar'
root = Path(tempfile.mkdtemp(prefix='d330-root-', dir=Path(sys.argv[1]).resolve().parent))
root.chmod(0o755)
users = ['d330-hub-test', 'd330-motor-test']
created = []
helper = Path('/opt/donnabot/bin/donnabot-motor-exec') if hub_binary else root / 'donnabot-motor-exec'
assert not helper.exists(), 'ajudante já existe; recuso alterar'
helper.parent.mkdir(parents=True, exist_ok=True)
try:
    for name in users:
        subprocess.run(['useradd', '--system', '--no-create-home', name], check=True)
        created.append(name)
    hub, motor = (pwd.getpwnam(name) for name in users)
    shutil.copyfile(sys.argv[1], helper)
    os.chown(helper, 0, hub.pw_gid)
    helper.chmod(0o750)
    subprocess.run(['setcap', 'cap_setuid,cap_setgid+ep', str(helper)], check=True)
    print(subprocess.check_output(['getcap', str(helper)], text=True).strip())
    config.parent.mkdir(exist_ok=True)
    def configure(user=users[1], mode=0o644, work_gid=motor.pw_gid):
        config.write_text(f'motor_user={user}\nhub_uid={hub.pw_uid}\nwork_gid={work_gid}\n')
        config.chmod(mode)
    def as_hub():
        os.setgroups([])
        os.setresgid(hub.pw_gid, hub.pw_gid, hub.pw_gid)
        os.setresuid(hub.pw_uid, hub.pw_uid, hub.pw_uid)
    def run(args, caller=True):
        return subprocess.run([str(helper), *args], preexec_fn=as_hub if caller else None,
                              capture_output=True, text=True, timeout=10)
    configure()
    result = run(['--cwd', '/tmp', '--', '/bin/cat', '/proc/self/status'])
    assert result.returncode == 0, result.stderr
    status = dict(line.split(':', 1) for line in result.stdout.splitlines() if ':' in line)
    assert status['Uid'].split() == [str(motor.pw_uid)] * 4
    assert status['Gid'].split() == [str(motor.pw_gid)] * 4
    assert status['Groups'].split() == [str(motor.pw_gid)]
    for key in ['CapPrm', 'CapEff', 'CapInh', 'CapAmb']:
        assert int(status[key].strip(), 16) == 0
    assert status['NoNewPrivs'].strip() == '1'
    for key in ['Uid', 'Gid', 'Groups', 'CapPrm', 'CapEff', 'CapInh', 'CapAmb', 'NoNewPrivs']:
        print(key + ':' + status[key])
    assert run(['--cwd', '/', '--', '/bin/true'], caller=False).returncode != 0
    for user, mode in [('root', 0o644), (users[0], 0o644), (users[1], 0o666)]:
        configure(user, mode)
        assert run(['--cwd', '/', '--', '/bin/true']).returncode != 0
    configure()
    os.chown(config, hub.pw_uid, hub.pw_gid)
    assert run(['--cwd', '/', '--', '/bin/true']).returncode != 0
    os.chown(config, 0, 0)
    config.unlink()
    assert run(['--cwd', '/', '--', '/bin/true']).returncode != 0
    configure()
    print('PASS chamador errado, root, alvo Hub, modo/dono inseguros, ausente')
    # C1: cada negativo mantém a outra configuração válida e exige o motivo exato.
    gid_error = 'grupo primário do motor ou de trabalho igual ao grupo primário do Hub'
    assert hub.pw_gid != 0 and motor.pw_gid != hub.pw_gid
    try:
        subprocess.run(['usermod', '--gid', str(hub.pw_gid), users[1]], check=True)
        assert pwd.getpwnam(users[1]).pw_gid == hub.pw_gid
        configure()
        result = run(['--cwd', '/', '--', '/bin/true'])
        assert result.returncode != 0 and gid_error in result.stderr, result
        print('PASS C1 motor_gid=Hub primary: ' + gid_error)
    finally:
        subprocess.run(['usermod', '--gid', str(motor.pw_gid), users[1]], check=True)
        configure()
    assert pwd.getpwnam(users[1]).pw_gid == motor.pw_gid
    try:
        configure(work_gid=hub.pw_gid)
        result = run(['--cwd', '/', '--', '/bin/true'])
        assert result.returncode != 0 and gid_error in result.stderr, result
        print('PASS C1 work_gid=Hub primary: ' + gid_error)
    finally:
        configure()
    # O cwd privado é atravessado só após a troca.
    private = root / 'cwd-motor'
    private.mkdir(mode=0o700)
    os.chown(private, motor.pw_uid, motor.pw_gid)
    assert run(['--cwd', str(private), '--', '/bin/pwd']).stdout.strip() == str(private)
    # Protocolo usado pelo PTY de login/_meta.terminal-auth: ajudante, cwd e argv.
    import pty
    master, slave = pty.openpty()
    child = subprocess.Popen([str(helper), '--cwd', '/tmp', '--', '/usr/bin/id', '-u'],
                             preexec_fn=as_hub, stdin=slave, stdout=slave, stderr=slave)
    os.close(slave)
    assert child.wait(timeout=10) == 0
    assert os.read(master, 4096).strip() == str(motor.pw_uid).encode()
    os.close(master)
    print('PASS PTY executa somente com UID do motor')
    def alive(pid):
        try:
            return '\nState:\tZ' not in Path(f'/proc/{pid}/status').read_text()
        except (FileNotFoundError, ProcessLookupError):
            return False
    def stopped(pid):
        deadline = time.monotonic() + 5
        while alive(pid) and time.monotonic() < deadline:
            time.sleep(0.02)
        assert not alive(pid), f'processo {pid} sobreviveu'
    child = subprocess.Popen([str(helper), '--cwd', '/tmp', '--', '/bin/sh', '-c',
                              'sleep 120 & echo $!; wait'], preexec_fn=as_hub,
                             stdout=subprocess.PIPE, text=True, start_new_session=True)
    grandchild = int(child.stdout.readline())
    assert run(['kill-group', str(child.pid)]).returncode == 0
    child.wait(timeout=10)
    stopped(grandchild)
    print('PASS kill-group encerra neto')
    # Pai simulando o Hub: confere PDEATHSIG armado no exec e limpeza pós-saída.
    child_code = 'import ctypes,time; s=ctypes.c_int(); assert ctypes.CDLL(None).prctl(2,ctypes.byref(s),0,0,0)==0; print(s.value,flush=True); time.sleep(120)'
    parent_code = (
        'import os,subprocess,sys,time; '
        f'os.setgroups([]); os.setgid({hub.pw_gid}); os.setuid({hub.pw_uid}); '
        f'p=subprocess.Popen([{str(helper)!r},"--cwd","/tmp","--",{sys.executable!r},"-c",{child_code!r}],stdout=subprocess.PIPE); '
        'assert p.stdout.readline()==b"9\\n"; print(p.pid,flush=True); time.sleep(120)'
    )
    parent = subprocess.Popen([sys.executable, '-c', parent_code], stdout=subprocess.PIPE, text=True)
    motor_pid = int(parent.stdout.readline())
    parent.kill()
    parent.wait(timeout=10)
    print(f'OBS filho vivo após morte do pai de outro UID: {alive(motor_pid)}')
    assert run(['cleanup']).returncode == 0
    stopped(motor_pid)
    print('PASS PDEATHSIG=9 no exec; cleanup após morte do pai encerra filho')
    orphan = subprocess.Popen([str(helper), '--cwd', '/tmp', '--', '/bin/sh', '-c',
                               'sleep 120 </dev/null >/dev/null 2>&1 & echo $!'],
                              preexec_fn=as_hub, stdout=subprocess.PIPE, text=True)
    orphan_pid = int(orphan.stdout.readline())
    orphan.wait(timeout=10)
    assert run(['cleanup']).returncode == 0
    stopped(orphan_pid)
    print('PASS cleanup após saída do pai encerra motor órfão')
    if test_binary:
        # Os mesmos chamadores dos testes fictícios, agora com UID real separado.
        names = [
            'market::verify::cancellation_tests::motor_verify_persistente_e_timeout_encerram_grupo',
            'fork::engine_accounts::probe::cancellation_tests::motor_codex_persistente_e_timeout_encerram_grupo',
            'agent::term::cancellation_tests::motor_fechar_pty_legado_encerra_filho_e_neto',
            'fork::motor_exec::test_support::motor_timeout_output_encerra_grupo',
            'agent::term::linger_tests::motor_linger_nao_repete_kill_group',
        ]
        for index, name in enumerate(names):
            case_dir = root / f'case-{index}'
            case_dir.mkdir(mode=0o777)
            case_dir.chmod(0o777)
            env = dict(os.environ, DONNABOT_MOTOR_CASE_DIR=str(case_dir),
                       DONNABOT_MOTOR_EXPECT_UID=str(motor.pw_uid), HOME='/tmp', MEXERICA_HOME='/tmp')
            if name.endswith('motor_linger_nao_repete_kill_group'):
                # Só instrumentação do teste: registra e encaminha ao ajudante real.
                log_dir = root / 'helper-log'
                log_dir.mkdir(mode=0o700)
                os.chown(log_dir, hub.pw_uid, hub.pw_gid)
                forwarding = log_dir / 'helper'
                forwarding.write_text('#!/bin/sh\nprintf \'%s\\n\' "$*" >> "$0.calls"\nexec ' + str(helper) + ' "$@"\n')
                forwarding.chmod(0o755)
                env['DONNABOT_MOTOR_TEST_HELPER'] = str(forwarding)
            subprocess.run([str(test_binary), '--exact', name, '--nocapture'], env=env,
                           preexec_fn=as_hub, check=True, timeout=30)
            for pid_file in case_dir.glob('*.pid'):
                values = json.loads(pid_file.read_text())
                assert values[2] == motor.pw_uid
                for pid in values[:2]:
                    stopped(pid)
            print('PASS chamador real root: ' + name)
    if hub_binary:
        observation = root / 'observation'
        observation.mkdir(mode=0o700)
        os.chown(observation, hub.pw_uid, hub.pw_gid)
        data = observation / 'data'
        wrapper = root / 'start-hub.py'
        # Mesmo pai/PID: inicia motor e substitui o lançador pelo Hub real.
        wrapper.write_text(
            'import os,subprocess,json\n'
            f'p=subprocess.Popen([{str(helper)!r},"--cwd","/tmp","--","/bin/sh","-c","sleep 120 & echo $!; wait"],stdout=subprocess.PIPE,text=True,start_new_session=True)\n'
            'neto=int(p.stdout.readline())\n'
            f'open({str(observation / "pids")!r},"w").write(json.dumps([p.pid,neto]))\n'
            f'os.execv({str(hub_binary)!r},[{str(hub_binary)!r},"serve","--bind","127.0.0.1"])\n'
        )
        subprocess.run(['systemctl', 'start', f'user@{hub.pw_uid}.service'], check=True)
        service_started = True
        unit_dir = Path(f'/run/user/{hub.pw_uid}/systemd/user')
        unit_dir.mkdir(parents=True, exist_ok=True)
        (unit_dir / 'd330-f2-test.service').write_text(
            '[Service]\nUMask=0077\n'
            f'ExecStartPre={helper} cleanup\n'
            f'ExecStart={sys.executable} {wrapper}\n'
            f'Environment=MEXERICA_HOME={data}\n'
            f'ExecStopPost={helper} cleanup\n'
            'TimeoutStopSec=10s\n'
        )
        def unit(*args, check=True):
            return subprocess.run(['runuser', '-u', users[0], '--', 'env',
                                   f'XDG_RUNTIME_DIR=/run/user/{hub.pw_uid}', 'systemctl', '--user', *args],
                                  check=check, capture_output=True, text=True, timeout=30)
        unit('daemon-reload')
        unit('start', 'd330-f2-test.service')
        deadline = time.monotonic() + 15
        while not (data / 'token').exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        assert (data / 'token').exists(), 'Hub não abriu; conferir journal da unidade'
        assert unit('is-active', 'd330-f2-test.service').stdout.strip() == 'active'
        pids = json.loads((observation / 'pids').read_text())
        assert all(alive(pid) for pid in pids)
        main_pid = unit('show', 'd330-f2-test.service', '-p', 'MainPID', '--value').stdout.strip()
        hub_status = dict(line.split(':', 1) for line in Path(f'/proc/{main_pid}/status').read_text().splitlines() if ':' in line)
        assert hub_status['Uid'].split() == [str(hub.pw_uid)] * 4
        assert int(hub_status['CapEff'].strip(), 16) == 0
        print('PASS Hub real sem root e CapEff zero')
        unit('stop', 'd330-f2-test.service')
        for pid in pids:
            stopped(pid)
        assert unit('show', 'd330-f2-test.service', '-p', 'Result', '--value').stdout.strip() == 'success'
        print('PASS unidade systemd --user para Hub real com motor e neto ativos; ExecStopPost sem sobras')
    else:
        print('LIMITAÇÃO: sem --hub BIN, parada real do Hub não foi testada')
finally:
    if service_started:
        subprocess.run(['runuser', '-u', users[0], '--', 'env',
                        f'XDG_RUNTIME_DIR=/run/user/{hub.pw_uid}', 'systemctl', '--user',
                        'stop', 'd330-f2-test.service'], check=False, timeout=30)
        subprocess.run(['systemctl', 'stop', f'user@{hub.pw_uid}.service'], check=False, timeout=30)
    config.unlink(missing_ok=True)
    for name in reversed(created):
        # Somente PIDs dos UIDs criados nesta prova descartável.
        uid = pwd.getpwnam(name).pw_uid
        for entry in Path('/proc').iterdir():
            try:
                if entry.name.isdigit() and entry.stat().st_uid == uid:
                    os.kill(int(entry.name), signal.SIGKILL)
            except (FileNotFoundError, ProcessLookupError):
                pass
        subprocess.run(['userdel', name], check=True)
    helper.unlink(missing_ok=True)
    shutil.rmtree(root)
