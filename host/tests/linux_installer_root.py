#!/usr/bin/env python3
"""Sonda F4, só VM descartável: sudo python3 SCRIPT HUB AJUDANTE --vm-descartavel.
Python é ferramenta da prova, não requisito nem código do instalador.
"""
import hashlib
import fcntl
import json
import os
from pathlib import Path
import pwd
import re
import signal
import shutil
import sqlite3
import subprocess
import sys
import time
import urllib.request

SAFE_ROOT_ENV = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin'}

assert os.geteuid() == 0 and sys.argv[3:] in [
    ['--vm-descartavel'], ['--vm-descartavel', '--integrado']]
integrated = '--integrado' in sys.argv[3:]
host, helper = (Path(p).resolve() for p in sys.argv[1:3])
base = Path('/var/lib/donnabot')
bin_dir = Path('/opt/donnabot')
config = Path('/etc/donnabot/motor.conf')
home = Path('/home/d330-f4-hub')
backup = Path('/var/backups/donnabot-f4-prova')
record = Path(f'{backup}.manifesto.json')
forged = Path('/var/backups/donnabot-f4-forjado')
for path in [base, bin_dir, config, home, backup, record, forged, Path('/etc/donnabot-f4-forjado')]:
    assert not path.exists(), f'VM já contém {path}; recuso alterar'
for name in ['d330-f4-hub', 'donnabot-motor']:
    assert subprocess.run(['getent', 'passwd', name], env=SAFE_ROOT_ENV, stdout=subprocess.DEVNULL).returncode == 2
assert subprocess.run(['getent', 'group', 'donnabot-trabalho'], env=SAFE_ROOT_ENV, stdout=subprocess.DEVNULL).returncode == 2
created = False
manager = False
backup_tmpfs = False

def run(*args, check=True, process_env=None, **kwargs):
    result = subprocess.run([str(a) for a in args], env=SAFE_ROOT_ENV if process_env is None else process_env,
                            check=check, capture_output=True,
                            text=True, timeout=90, **kwargs)
    if result.returncode and check:
        print(result.stdout, result.stderr)
    return result

def stage(action, *args, check=True):
    result = run(host, 'install-motor', action, '--hub-uid', hub.pw_uid, *args, check=False)
    if check:
        assert result.returncode == 0, result.stdout + result.stderr
    return result

def as_hub(*args, check=True, motor_env=False, **kwargs):
    hub_env = {
        'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'HOME': str(base / 'motor' if motor_env else home), 'USER': hub.pw_name,
        'XDG_RUNTIME_DIR': f'/run/user/{hub.pw_uid}',
        'DBUS_SESSION_BUS_ADDRESS': f'unix:path=/run/user/{hub.pw_uid}/bus',
    }
    return run('runuser', *(['--preserve-environment'] if motor_env else []), '-u', hub.pw_name,
               '--', *args, check=check, process_env=hub_env, **kwargs)


def service(*args, check=True):
    return as_hub('systemctl', '--user', *args, check=check)

def health():
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen('http://127.0.0.1:19422/health', timeout=1) as response:
                return json.load(response)
        except OSError:
            time.sleep(0.05)
    raise AssertionError('Hub não respondeu em 20 s')

def mode(path):
    return path.stat().st_mode & 0o7777

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def service_logs():
    # O journal inclui stdout e stderr do serviço; nunca consultar Environment.
    chunks = []
    for command in [
        ['journalctl', '--user', '-u', 'donnabot-host.service', '--no-pager', '-n', '200', '-o', 'cat'],
        ['systemctl', '--user', 'show', 'donnabot-host.service', '-p', 'Result',
         '-p', 'ExecMainCode', '-p', 'ExecMainStatus', '-p', 'StandardError', '-p', 'StandardOutput'],
    ]:
        result = as_hub(*command, check=False)
        chunks.append(' '.join(command) + '\n' + result.stdout + result.stderr)
    # UID de sistema pode ter journal somente no armazenamento do sistema.
    # VM exclusiva: filtrar a unidade fictícia inclui também o stderr do ajudante.
    command = ['journalctl', '_SYSTEMD_USER_UNIT=donnabot-host.service',
               '--no-pager', '-n', '200', '-o', 'cat']
    result = run(*command, check=False)
    chunks.append(' '.join(command) + '\n' + result.stdout + result.stderr)
    text = '\n'.join(chunks)
    text = re.sub(r'(?i)(bearer\s+)[^\s"\']+', r'\1[MASCARADO]', text)
    text = re.sub(r'(?i)((?:token|secret|password|senha|api[_-]?key|authorization)\s*[=:]\s*)[^\s,;]+',
                  r'\1[MASCARADO]', text)
    text = re.sub(r'[A-Za-z0-9_+/=-]{32,}', '[MASCARADO]', text)
    Path('/data/f4/hub-sanitized.log').write_text(text)
    print('LOG HUB (stdout/stderr mascarados):\n' + text)

try:
    run('useradd', '--system', '--user-group', '--create-home', '--home-dir', home,
        '--shell', '/bin/bash', 'd330-f4-hub')
    created = True
    hub = pwd.getpwnam('d330-f4-hub')
    run('systemctl', 'start', f'user@{hub.pw_uid}.service')
    manager = True
    args = ['--host', host, '--ajudante', helper, '--data-dir', home / '.mexerica', '--port', '19422']
    stage('instalar', *args)
    assert health()['environment'] == 'main'
    motor = pwd.getpwnam('donnabot-motor')
    assert motor.pw_uid not in [0, hub.pw_uid]
    assert motor.pw_dir == str(base / 'motor') and motor.pw_shell.endswith('/nologin')
    assert mode(base / 'motor') == 0o700 and mode(base / 'trabalho') == 0o2770
    assert (base / 'motor').stat().st_uid == motor.pw_uid
    assert (base / 'trabalho').stat().st_uid == hub.pw_uid
    print('PASS 1 instalação limpa, usuário/grupo/raízes e Hub real main')
    exe = bin_dir / 'bin/donnabot-motor-exec'
    assert exe.stat().st_uid == 0 and exe.stat().st_gid == hub.pw_gid and mode(exe) == 0o750
    assert config.stat().st_uid == 0 and mode(config) == 0o644
    host_bin = bin_dir / 'bin/donnabot-host'
    assert (bin_dir / 'bin').stat().st_uid == 0 and (bin_dir / 'bin').stat().st_gid == 0 and mode(bin_dir / 'bin') == 0o755
    assert host_bin.stat().st_uid == 0 and host_bin.stat().st_gid == 0 and mode(host_bin) == 0o755
    assert os.getxattr(exe, 'security.capability')
    assert 'cap_setgid,cap_setuid=ep' in run('getcap', exe).stdout
    assert as_hub(exe, '--cwd', base / 'motor', '--', '/usr/bin/id', '-u').stdout.strip() == str(motor.pw_uid)
    print('PASS 2 dono/modo/config e capacidades efetivas pelo Hub')
    text = service('cat', 'donnabot-host.service').stdout
    assert 'UMask=0077' in text and 'ExecStartPre=' in text and 'ExecStopPost=' in text
    assert 'NoNewPrivileges' not in text
    pid = service('show', 'donnabot-host.service', '-p', 'MainPID', '--value').stdout.strip()
    status = Path(f'/proc/{pid}/status').read_text()
    assert f'Uid:\t{hub.pw_uid}\t{hub.pw_uid}' in status
    assert int(next(x.split(':')[1] for x in status.splitlines() if x.startswith('CapEff:')), 16) == 0
    print('PASS 3 unidade de usuário, UMask/limpezas, Hub sem root nem capacidades')
    link = home / '.local/bin/donnabot-host'
    assert link.is_symlink() and link.resolve() == bin_dir / 'bin/donnabot-host'
    # Mesma descoberta do ssh-command.ts: primeiro caminho absoluto da home.
    info = json.loads(as_hub(link, 'info', '--json', '--port', '19422').stdout)
    assert info
    print('PASS 4 comando info do app encontra binário pelo link estável (R5)')
    services = as_hub('busctl', '--user', 'list', check=False)
    if services.returncode != 0:
        print('LIMITAÇÃO R4: D-Bus da sessão sem consulta disponível; sentinela de cofre fica para F5')
    elif 'org.freedesktop.secrets' not in services.stdout:
        print('LIMITAÇÃO R4: VM sem Secret Service; sentinela de cofre fica para F5')
    else:
        raise RuntimeError('Secret Service presente: executar sentinela real antes de aceitar a prova')
    sentinel = base / 'trabalho/dado-ficticio'
    sentinel.write_text('preservar-este-dado-ficticio')
    before = digest(sentinel)
    ids = (motor.pw_uid, motor.pw_gid, (base / 'trabalho').stat().st_gid)
    exe.chmod(0o700)
    run('setcap', '-r', exe)
    stage('instalar', *args)
    assert digest(sentinel) == before
    assert ids == (pwd.getpwnam('donnabot-motor').pw_uid, pwd.getpwnam('donnabot-motor').pw_gid,
                   (base / 'trabalho').stat().st_gid)
    assert mode(exe) == 0o750 and os.getxattr(exe, 'security.capability')
    assert health()['ok']
    print('PASS 5 reinstalação idempotente, reaplica modo/cap e preserva dados/IDs')
    secret = Path('/root/f4-origem-privada')
    secret.write_text('origem-que-a-conta-do-hub-nao-le')
    secret.chmod(0o600)
    denied = stage('instalar', '--host', secret, *args[2:], check=False)
    secret.unlink()
    assert denied.returncode != 0 and host_bin.stat().st_uid == 0 and health()['ok']
    print('PASS 5b origem que a conta do Hub não lê é recusada antes de parar o Hub')
    os.chown(bin_dir / 'bin', hub.pw_uid, hub.pw_gid)
    os.chown(host_bin, hub.pw_uid, hub.pw_gid)
    planted = bin_dir / 'bin/plantado'
    planted.write_text('x')
    os.chown(planted, hub.pw_uid, hub.pw_gid)
    stage('instalar', *args)
    assert (bin_dir / 'bin').stat().st_uid == 0 and mode(bin_dir / 'bin') == 0o755 and not planted.exists()
    assert host_bin.stat().st_uid == 0 and mode(host_bin) == 0o755 and os.getxattr(exe, 'security.capability')
    assert not list(bin_dir.glob('.bin-*')) and health()['ok']
    print('PASS 5c pasta de binários da conta do Hub trocada por uma do root na reinstalação')
    outside = Path('/etc/donnabot-f4-fora-da-home')
    refused = stage('instalar', '--host', host, '--ajudante', helper, '--data-dir', outside, '--port', '19422', check=False)
    assert refused.returncode != 0 and not outside.exists() and health()['ok']
    print('PASS 5d pasta de dados fora da home do Hub recusada sem criar nada')
    config_before, exe_before, bin_inode = config.read_bytes(), digest(exe), (bin_dir / 'bin').stat().st_ino
    fake = Path('/tmp/f4bin/ajudante-falso')
    shutil.copy('/usr/bin/false', fake)
    fake.chmod(0o755)
    refused = stage('instalar', '--host', host, '--ajudante', fake, *args[4:], check=False)
    fake.unlink()
    assert refused.returncode != 0 and health()['ok'] and config.read_bytes() == config_before
    assert digest(exe) == exe_before and (bin_dir / 'bin').stat().st_ino == bin_inode
    assert not list(bin_dir.glob('.bin-*'))
    print('PASS 5e ajudante que não funciona é recusado antes de parar o Hub, sem trocar nada')
    fake = Path('/tmp/f4bin/ajudante-sem-troca')
    fake.write_text('#!/bin/sh\nshift 3\nexec "$@"\n')
    fake.chmod(0o755)
    refused = stage('instalar', '--host', host, '--ajudante', fake, *args[4:], check=False)
    fake.unlink()
    assert refused.returncode != 0 and 'conta do motor' in refused.stdout + refused.stderr
    assert health()['ok'] and config.read_bytes() == config_before and digest(exe) == exe_before
    assert (bin_dir / 'bin').stat().st_ino == bin_inode and not list(bin_dir.glob('.bin-*'))
    print('PASS 5e2 ajudante que termina sem trocar de conta é recusado antes de parar o Hub')
    local_bin = home / '.local/bin'
    local_bin.rename(home / '.local/bin-guardado')
    local_bin.write_text('bloqueio')
    os.chown(local_bin, hub.pw_uid, hub.pw_gid)
    failed = stage('instalar', *args, check=False)
    local_bin.unlink()
    (home / '.local/bin-guardado').rename(local_bin)
    assert failed.returncode != 0 and 'desfeita' in failed.stdout + failed.stderr
    assert (bin_dir / 'bin').stat().st_ino == bin_inode and digest(exe) == exe_before
    assert config.read_bytes() == config_before and not list(bin_dir.glob('.bin-*')) and health()['ok']
    print('PASS 5f falha depois da troca desfaz a troca e religa o Hub anterior')
    fake = Path('/tmp/f4bin/hub-que-nao-sobe')
    shutil.copy('/usr/bin/true', fake)
    fake.chmod(0o755)
    failed = stage('instalar', '--host', fake, *args[2:], check=False)
    fake.unlink()
    assert failed.returncode != 0 and 'desfeita' in failed.stdout + failed.stderr
    assert (bin_dir / 'bin').stat().st_ino == bin_inode and digest(exe) == exe_before
    assert not list(bin_dir.glob('.bin-*')) and health()['ok']
    print('PASS 5g Hub novo que não responde no /health é desfeito e o anterior volta')
    # UID da etapa é explícito, testado separadamente, sem modificar a instalação.
    wrong = run(host, 'install-motor', 'desinstalar', '--hub-uid', '0', check=False)
    assert wrong.returncode != 0 and config.exists()
    print('PASS 6 Hub root recusado sem remover instalação')
    stage('desinstalar')
    assert not config.exists() and not exe.exists()
    assert not (home / '.config/systemd/user/donnabot-host.service').exists()
    assert pwd.getpwnam('donnabot-motor') and digest(sentinel) == before and link.exists()
    print('PASS 7 desinstalação conserva dados, contas, binário e link')
    stage('desinstalar')
    print('PASS 8 desinstalação repetida é idempotente')
    stage('instalar', *args)
    assert health()['ok']
    stage('desinstalar', '--remover-usuario-motor')
    assert subprocess.run(['getent', 'passwd', 'donnabot-motor'], env=SAFE_ROOT_ENV, stdout=subprocess.DEVNULL).returncode == 2
    assert (base / 'motor').exists() and sentinel.exists()
    print('PASS 9 remoção explícita da conta conserva home/dados')
    if not integrated:
        print('ESCOPO: instalador puro aprovado; migração/volta aguardam F3 e F3b na main')
        sys.exit(0)
    # Só a VM própria: tenta um FS distinto para provar EXDEV na volta real.
    if shutil.which('mount', path=SAFE_ROOT_ENV['PATH']) and not any(Path('/var/backups').iterdir()):
        mounted = run('mount', '-t', 'tmpfs', '-o', 'size=2G,mode=0755', 'tmpfs', '/var/backups', check=False)
        backup_tmpfs = mounted.returncode == 0
    if backup_tmpfs:
        assert Path('/var/backups').stat().st_dev != Path('/etc/donnabot').stat().st_dev
        print('CENÁRIO EXDEV real: /var/backups em tmpfs distinto de /etc/donnabot')
    else:
        print('LIMITAÇÃO EXDEV: VM sem montagem disponível ou /var/backups não vazio; cobertura pelo teste D70 simulado')
    # Raízes novas vazias para a migração; só dados criados por esta sonda.
    shutil.rmtree(base)
    shutil.rmtree(bin_dir)
    link.unlink()
    shutil.copyfile(host, link)
    with link.open('ab') as legacy_binary:
        legacy_binary.write(b'\nDONNA_F4_BINARY_LEGACY_FICTICIO\n')
    legacy_binary_hash = digest(link)
    assert legacy_binary_hash != digest(host)
    link.chmod(0o755)
    os.chown(link, hub.pw_uid, hub.pw_gid)
    data = home / '.mexerica'
    old_unit = home / '.config/systemd/user/donnabot-host.service'
    old_unit.write_text('[Service]\n' + f'ExecStart={link} serve --port 19422\n' +
                        f'Environment=MEXERICA_HOME={data}\n[Install]\nWantedBy=default.target\n')
    os.chown(old_unit, hub.pw_uid, hub.pw_gid)
    service('daemon-reload')
    service('start', 'donnabot-host.service')
    assert health()['busy'] is False
    service('stop', 'donnabot-host.service')
    bid = 'd330f400-0000-4000-8000-000000000001'
    workspace = data / 'bots' / bid / 'workspace'
    (workspace / 'uploads').mkdir(parents=True)
    (workspace / 'arquivo.txt').write_text('workspace fictício')
    executable = workspace / 'script.sh'
    executable.write_text('#!/bin/sh\nexit 0\n')
    executable.chmod(0o700)
    (workspace / 'uploads/anexo.txt').write_text('anexo fictício')
    (data / 'skills').mkdir(exist_ok=True)
    (data / 'skills/ficticia.md').write_text('skill fictícia')
    profile = home / '.codex-donna-ficticio'
    profile.mkdir(mode=0o700)
    (profile / 'auth.json').write_text('{"ficticio":true}')
    personal = home / '.codex'
    personal.mkdir(mode=0o700)
    (personal / 'nao-copiar').write_text('perfil pessoal fictício sem referência')
    db = sqlite3.connect(data / 'codync.db')
    db.execute('INSERT INTO bots(id,rev,config) VALUES(?,?,?)', (bid, 1, json.dumps({
        'id': bid, 'name': 'Fixture F4', 'backend': 'codex', 'cwd': str(workspace)})))
    account = {'id':'ac_' + '1' * 64,'backend':'codex','billing':'unverified','state':'disconnected',
               'launch':{'kind':'cli_profile','executable':'/usr/bin/false','directory':str(profile)},
               'scope':'owner','quotaIdentity':None,'verifiedAt':None,'subscriptionProof':None,'models':[]}
    db.execute('INSERT OR REPLACE INTO kv(k,v) VALUES(?,?)', ('fork.engine-accounts.v1', json.dumps({
        'salt':'d330f400-0000-4000-8000-000000000099','accounts':[account, {**account, 'id':'ac_' + '2' * 64}]})))
    db.commit()
    db.close()
    run('chown', '-R', f'{hub.pw_uid}:{hub.pw_gid}', data, profile, personal)
    service('start', 'donnabot-host.service')
    assert health()['busy'] is False
    preflight = as_hub(host, 'migrar-raizes', '--data-dir', data, '--inventario', check=False)
    if preflight.returncode:
        refusal = stage('migrar', '--host', host, '--ajudante', helper, '--data-dir', data,
                        '--backup', backup, '--port', '19422', check=False)
        assert refusal.returncode != 0 and not backup.exists() and not config.exists()
        assert health()['ok']
        print('PASS 10 sem F3 integrada, migração recusa antes de parar/alterar')
        raise RuntimeError('F3 não integrada: prova de migração e volta pendente; não declarar entrega integral')
    inventory = json.loads(preflight.stdout)
    assert sum(i['kind'] == 'profile' and i['source'] == str(profile) for i in inventory) == 2
    stage('migrar', '--host', host, '--ajudante', helper, '--data-dir', data,
          '--backup', backup, '--port', '19422')
    assert health()['ok'] and link.is_symlink() and mode(backup) == 0o700
    assert all(mode(p) == 0o600 for p in backup.rglob('*') if p.is_file())
    assert record.stat().st_uid == 0 and mode(record) == 0o600 and not (backup / 'manifest.json').exists()
    print('PASS 10 migração fictícia: backup privado, cópias, F3 e Hub reiniciado')
    migrated_script = base / 'trabalho/bots' / bid / 'workspace/script.sh'
    assert mode(migrated_script) == 0o770
    as_hub(exe, '--cwd', migrated_script.parent, '--', migrated_script)
    print('PASS F4-REV2: script 0700 copiado executa como motor pelo grupo de trabalho')
    for item in inventory:
        src, dst = Path(item['source']), Path(item['destination'])
        assert dst.exists() and src.exists()
        if src.is_file():
            assert digest(src) == digest(dst)
        else:
            for file in src.rglob('*'):
                if file.is_file():
                    # uploads aninhados têm destino próprio só leitura.
                    if item['kind'] == 'workspace' and any(file.is_relative_to(Path(u['source']))
                            for u in inventory if u['kind'] == 'uploads'):
                        assert not (dst / file.relative_to(src)).exists()
                    else:
                        assert digest(file) == digest(dst / file.relative_to(src))
    assert not any(p.name == 'nao-copiar' for p in base.rglob('*'))
    print('PASS 11 perfis referidos copiados; pessoal ausente; anexos sem cópia gravável')
    db = sqlite3.connect(data / 'codync.db')
    migrated = json.loads(db.execute('SELECT v FROM kv WHERE k=?', ('fork.engine-accounts.v1',)).fetchone()[0])
    paths = [a['launch']['directory'] for a in migrated['accounts']]
    assert len(set(paths)) == 2
    for account_after in migrated['accounts']:
        assert account_after['id'] in account_after['launch']['directory']
    assert json.loads(db.execute('SELECT config FROM bots WHERE id=?', (bid,)).fetchone()[0])['cwd'] == str(base / 'trabalho/bots' / bid / 'workspace')
    snapshot = db.execute("SELECT v FROM kv WHERE k='d330.raizes.v1'").fetchone()[0]
    db.close()
    service('stop', 'donnabot-host.service')
    with (data / 'host.lock').open('r+') as migration_lock:
        fcntl.flock(migration_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        refused = as_hub(link, 'migrar-raizes', '--data-dir', data, check=False)
        assert refused.returncode != 0
    as_hub(link, 'migrar-raizes', '--data-dir', data)
    db = sqlite3.connect(data / 'codync.db')
    assert db.execute("SELECT v FROM kv WHERE k='d330.raizes.v1'").fetchone()[0] == snapshot
    db.close()
    print('PASS F3 real: referências por ID, origens compartilhadas e repetição sem trocar backup')
    # F3b: fronteira real de publicação/descoberta; sem baixar nem executar CLI de IA.
    motor = pwd.getpwnam('donnabot-motor')
    installed = base / 'motor/engine-setup/bin/fixture-f4'
    installed.parent.mkdir(parents=True, exist_ok=True)
    installed.write_text('#!/bin/sh\nexit 0\n')
    for directory in [base / 'motor/engine-setup', installed.parent]:
        os.chown(directory, motor.pw_uid, motor.pw_gid)
        directory.chmod(0o700)
    os.chown(installed, motor.pw_uid, motor.pw_gid)
    installed.chmod(0o700)
    def installer_request(request):
        # O worker real recebe HOME do motor; runuser normalmente o troca pelo HOME do Hub.
        response = as_hub(exe, '--cwd', base / 'motor', '--', bin_dir / 'bin/donnabot-host',
                          'motor-install', input=json.dumps(request), motor_env=True)
        return json.loads(response.stdout)
    assert installer_request({'operation':'published','path':str(installed)})['path'] == str(installed)
    assert installer_request({'operation':'locate','name':str(installed)})['path'] == str(installed)
    assert installer_request({'operation':'locate','name':str(link)})['path'] is None
    for protected in [data / 'token', data / 'codync.db']:
        assert as_hub(exe, '--cwd', base / 'motor', '--', '/usr/bin/test', '-r', protected, check=False).returncode != 0
    assert as_hub(exe, '--cwd', base / 'motor', '--', '/usr/bin/test', '-w', bin_dir / 'bin/donnabot-host', check=False).returncode != 0
    print('PASS F3b real: publicação e descoberta como motor; binário privado antigo recusado; token/DB/binário protegidos')
    service('start', 'donnabot-host.service')
    assert health()['ok']

    # A VM padrão não oferece Secret Service. Se houver, não fingir operação de cofre.
    services = as_hub('busctl', '--user', 'list', check=False)
    if services.returncode != 0:
        print('LIMITAÇÃO R4: D-Bus da sessão sem consulta disponível; sentinela de cofre fica para F5')
    elif 'org.freedesktop.secrets' not in services.stdout:
        print('LIMITAÇÃO R4: VM sem Secret Service; sentinela de cofre antes/depois fica para F5')
    else:
        raise RuntimeError('Secret Service presente: executar sentinela real antes de aceitar a prova')
    shutil.copytree(backup, forged)
    manifest = json.loads(record.read_text())
    manifest.update(binary='/etc/donnabot-f4-forjado', binary_mode=0o4755)
    (forged / 'manifest.json').write_text(json.dumps(manifest))
    (forged / 'manifest.json').chmod(0o600)
    run('chown', '-R', f'{hub.pw_uid}:{hub.pw_gid}', forged)
    refused = stage('voltar', '--backup', forged, check=False)
    assert refused.returncode != 0 and 'registro protegido' in refused.stderr, refused.stderr
    assert not Path('/etc/donnabot-f4-forjado').exists() and health()['ok']
    print('PASS F4-REV3: volta recusa cópia com manifesto do Hub sem registro protegido do root')
    post = data / 'mensagem-pos-troca-ficticia'
    post.write_text('não apagar')
    service('stop', 'donnabot-host.service')
    lock_inode = (data / 'host.lock').stat().st_ino
    rollback = subprocess.Popen([str(host), 'install-motor', 'voltar', '--hub-uid', str(hub.pw_uid),
                                 '--backup', str(backup)], env=SAFE_ROOT_ENV,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if any(p.is_dir() and any(p.iterdir()) for p in home.glob('donnabot-pos-troca-*')):
                rollback.send_signal(signal.SIGSTOP)
                _, stopped = os.waitpid(rollback.pid, os.WUNTRACED)
                assert os.WIFSTOPPED(stopped), 'volta terminou antes da observação'
                break
            assert rollback.poll() is None, 'volta terminou antes da restauração observável'
            time.sleep(0.001)
        else:
            raise AssertionError('volta não iniciou a restauração em 30 s')
        assert (data / 'host.lock').stat().st_ino == lock_inode
        with (data / 'host.lock').open('r+') as active_lock:
            try:
                fcntl.flock(active_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                pass
            else:
                raise AssertionError('host.lock ativo livre no meio da volta')
        contender = as_hub(host, 'serve', '--bind', '127.0.0.1', '--port', '0', check=False)
        assert contender.returncode != 0 and 'another donnabot-host is already running' in contender.stderr
        print('PASS F4-REV1: serve real concorrente recusado durante a volta real, inode do lock preservado')
    finally:
        if rollback.poll() is None:
            rollback.send_signal(signal.SIGCONT)
        rollback_stdout, rollback_stderr = rollback.communicate(timeout=90)
    assert rollback.returncode == 0, rollback_stdout + rollback_stderr
    assert (data / 'host.lock').stat().st_ino == lock_inode
    assert health()['ok'] and not link.is_symlink() and not config.exists()
    assert digest(link) == legacy_binary_hash
    assert not post.exists() and any((p / post.name).exists() for p in home.glob('donnabot-pos-troca-*'))
    assert (base / 'motor').exists() and (bin_dir / 'bin/donnabot-motor-exec').exists()
    assert mode(executable) == 0o700
    db = sqlite3.connect(data / 'codync.db')
    restored = json.loads(db.execute('SELECT v FROM kv WHERE k=?', ('fork.engine-accounts.v1',)).fetchone()[0])
    assert all(a['launch']['directory'] == str(profile) for a in restored['accounts'])
    assert db.execute("SELECT v FROM kv WHERE k='d330.raizes.v1'").fetchone() is None
    db.close()
    print('PASS 12 volta com hashes, unidade/binário/DB originais e estado novo preservado')
    if backup_tmpfs:
        inactive = backup.with_suffix('.motor-conf-inativo')
        assert inactive.is_file() and mode(inactive) == 0o644 and inactive.stat().st_uid == 0
        print('PASS EXDEV real: configuração desativada entre FS distintos, modo/root preservados e Hub antigo ativo')
finally:
    if manager:
        service_logs()
        service('stop', 'donnabot-host.service', check=False)
        run('systemctl', 'stop', f'user@{hub.pw_uid}.service', check=False)
    if created:
        # Somente UIDs/arquivos criados nesta prova, com ausência prévia conferida.
        for name in ['donnabot-motor', 'd330-f4-hub']:
            if subprocess.run(['getent', 'passwd', name], env=SAFE_ROOT_ENV, stdout=subprocess.DEVNULL).returncode == 0:
                run('userdel', name, check=False)
        for name in ['donnabot-trabalho', 'donnabot-motor', 'd330-f4-hub']:
            run('groupdel', name, check=False)
        for path in [base, bin_dir, home, backup, forged]:
            if path.exists():
                shutil.rmtree(path)
        config.unlink(missing_ok=True)
        record.unlink(missing_ok=True)
        backup.with_suffix('.motor-conf-inativo').unlink(missing_ok=True)
    if backup_tmpfs:
        run('umount', '/var/backups', check=False)
