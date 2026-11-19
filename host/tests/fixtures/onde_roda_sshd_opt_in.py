#!/usr/bin/python3
"""Prova separada, SOMENTE opt-in no servidor. Nunca chamada pela suíte padrão."""
import json
import os
import pathlib
import pwd
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time

assert sys.argv[1:] == ['--run'], 'exige --run explícito'
assert os.geteuid() != 0, 'execute sem root/sudo'
for exe in ['/usr/sbin/sshd', '/usr/bin/ssh', '/usr/bin/ssh-keygen']:
    assert pathlib.Path(exe).is_file(), 'dependência ausente: ' + exe
account = pwd.getpwuid(os.geteuid())
username = account.pw_name
home_candidate = pathlib.Path(account.pw_dir)
assert home_candidate.is_absolute(), 'home NSS precisa ser absoluta'
test_home = home_candidate.resolve(strict=True)
assert test_home.is_dir(), 'home NSS precisa ser diretório existente'
for parent in [test_home, *test_home.parents]:
    metadata = parent.stat()
    assert metadata.st_uid in [0, os.geteuid()] and not stat.S_IMODE(metadata.st_mode) & 0o022, \
        'pai inseguro para StrictModes: ' + str(parent)
root = pathlib.Path(tempfile.mkdtemp(prefix='.dwp04-sshd-', dir=test_home))
root.chmod(0o700)
children = []
listener = None
own_logs = []
last_ssh = None

def stop_children():
    for child in children:
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()

def safe_text(value):
    if isinstance(value, bytes):
        value = value.decode('utf8', errors='replace')
    return str(value).replace(str(root), '<fixture>').replace(str(test_home), '<nss-home>').replace(username, '<fixture-user>')

def failure_diagnostics(error):
    # Somente stderr/stdout dos processos próprios. Nunca ler chaves privadas,
    # configuração pessoal, journal do sistema ou logs de outro sshd.
    report = {'error': safe_text(error), 'client': None, 'own_sshd': []}
    if last_ssh is not None:
        report['client'] = {key: safe_text(value) if isinstance(value, (str, bytes)) else value
                            for key, value in last_ssh.items()}
    for path in own_logs:
        try:
            with path.open('rb') as source:
                source.seek(0, os.SEEK_END)
                size = source.tell()
                source.seek(max(0, size - 32768))
                report['own_sshd'].append({'name': path.name, 'bytes': size, 'tail': safe_text(source.read(32768))})
        except OSError as read_error:
            report['own_sshd'].append({'name': path.name, 'error': safe_text(read_error)})
    print('DONNA_SSHD_OPT_IN_DIAGNOSTIC ' + json.dumps(report, ensure_ascii=True), file=sys.stderr, flush=True)

try:
    for name in ['host', 'client']:
        subprocess.run(['/usr/bin/ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(root / name)],
                       env={}, stdin=subprocess.DEVNULL, check=True, timeout=10)
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen(1)
    target_port = listener.getsockname()[1]
    def echo():
        connection, _ = listener.accept()
        with connection:
            connection.sendall(connection.recv(32))
    worker = threading.Thread(target=echo, daemon=True)
    worker.start()
    host_key = (root / 'host.pub').read_text().split()
    (root / 'known_hosts').write_text('bx_fixture01 ' + ' '.join(host_key[:2]) + '\n')
    (root / 'global_known_hosts').write_text('')
    public = (root / 'client.pub').read_text().strip()
    (root / 'authorized_keys').write_text('restrict,port-forwarding,permitopen="127.0.0.1:' + str(target_port) + '",command="/bin/false" ' + public + '\n')
    (root / 'authorized_keys').chmod(0o600)
    cases = []
    for secure in [True, False]:
        reserve = socket.socket()
        reserve.bind(('127.0.0.1', 0))
        port = reserve.getsockname()[1]
        reserve.close()
        config = root / ('sshd-secure' if secure else 'sshd-permissive')
        config.write_text(f'''Port {port}
ListenAddress 127.0.0.1
HostKey {root / 'host'}
PidFile {root / 'pid'}
AuthorizedKeysFile {root / 'authorized_keys'}
StrictModes yes
AllowUsers {username}
PubkeyAuthentication yes
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
PermitRootLogin no
PermitTTY no
X11Forwarding no
AllowAgentForwarding no
PermitTunnel no
ForceCommand /bin/false
AllowTcpForwarding local
PermitOpen 127.0.0.1:{target_port}
PermitListen none
AllowStreamLocalForwarding {'no' if secure else 'yes'}
LogLevel DEBUG3
''')
        # A configuração permissiva isola exatamente o falso positivo Unix.
        if not secure:
            (root / 'authorized_keys').write_text('command="/bin/false" ' + public + '\n')
            config.write_text(config.read_text().replace('PermitListen none', 'PermitListen any'))
        log_path = root / ('secure.log' if secure else 'permissive.log')
        own_logs.append(log_path)
        log = open(log_path, 'wb')
        server = subprocess.Popen(['/usr/sbin/sshd', '-D', '-e', '-f', str(config)], env={},
                                  stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=log)
        children.append(server)
        for _ in range(100):
            assert server.poll() is None, 'sshd falhou: ' + str(root / 'secure.log')
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=0.1):
                    break
            except OSError:
                time.sleep(0.05)
        else:
            raise AssertionError('sshd não abriu a porta')
        client_config = root / 'ssh_config'
        client_config.write_text(f'''Host *
HostName 127.0.0.1
Port {port}
User {username}
HostKeyAlias bx_fixture01
IdentityFile {root / 'client'}
UserKnownHostsFile {root / 'known_hosts'}
GlobalKnownHostsFile {root / 'global_known_hosts'}
StrictHostKeyChecking yes
HostKeyAlgorithms ssh-ed25519
IdentityAgent none
IdentitiesOnly yes
BatchMode yes
ProxyCommand none
ProxyJump none
ControlPath none
LogLevel DEBUG3
''')
        def ssh(args, data=b''):
            global last_ssh
            last_ssh = {'args': [safe_text(arg) for arg in args]}
            try:
                result = subprocess.run(['/usr/bin/ssh', '-F', str(client_config)] + args,
                                        input=data, capture_output=True, env={}, timeout=6)
            except subprocess.TimeoutExpired as timeout:
                last_ssh.update({'timeout': True, 'stdout': (timeout.stdout or b'')[-32768:],
                                 'stderr': (timeout.stderr or b'')[-32768:]})
                raise
            last_ssh.update({'returncode': result.returncode, 'stdout': result.stdout[-32768:],
                             'stderr': result.stderr[-32768:]})
            assert len(result.stderr) <= 65536 and len(result.stdout) <= 65536
            return {'success': result.returncode == 0, 'stdout': result.stdout.decode(), 'stderr': result.stderr.decode()}
        if secure:
            positive = ssh(['-T', '-W', f'127.0.0.1:{target_port}', '--', 'bx_fixture01'], b'fixture-echo')
            assert positive['stdout'] == 'fixture-echo', 'controle positivo TCP falhou'
            for kind, args in [
                ('exec', ['-T', '--', 'bx_fixture01', 'printf DONNA_WORKPLACE_FORBIDDEN_V1']),
                ('shell', ['-T', '--', 'bx_fixture01']),
                ('pty', ['-tt', '--', 'bx_fixture01']),
                ('remote', ['-N', '-o', 'ExitOnForwardFailure=yes', '-R', '0:127.0.0.1:9', '--', 'bx_fixture01']),
                ('other_target', ['-T', '-W', '127.0.0.1:9', '--', 'bx_fixture01']),
            ]:
                cases.append({'kind': kind, **ssh(args, b'printf DONNA_WORKPLACE_FORBIDDEN_V1\nexit\n')})
        # Diretório existe, pertence ao mesmo usuário, e suporta bind/unlink.
        path = root / ('probe-secure.sock' if secure else 'probe-permissive.sock')
        test_socket = socket.socket(socket.AF_UNIX)
        test_socket.bind(str(path))
        test_socket.close()
        path.unlink()
        if secure:
            result = ssh(['-N', '-o', 'ExitOnForwardFailure=yes', '-R', str(path) + ':127.0.0.1:9', '--', 'bx_fixture01'])
            cases.append({'kind': 'remote_socket', 'listen': str(path), 'ready': True, **result})
        else:
            missing = root / 'does-not-exist' / 'listen.sock'
            result = ssh(['-N', '-o', 'ExitOnForwardFailure=yes', '-R', str(missing) + ':127.0.0.1:9', '--', 'bx_fixture01'])
            cases.append({'kind': 'remote_socket_false_positive', 'listen': str(missing), 'ready': False, **result})
        server.terminate()
        server.wait(timeout=5)
        log.close()
    print(json.dumps(cases))
except Exception as error:
    stop_children()
    failure_diagnostics(error)
    raise
finally:
    stop_children()
    if listener:
        listener.close()
    shutil.rmtree(root)
