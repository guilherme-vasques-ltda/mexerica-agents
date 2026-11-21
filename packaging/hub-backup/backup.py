#!/usr/bin/env python3
"""Backup do Hub. Sem dependências novas: Python + cryptography + curl existentes.

Formato v1: magic(8), RSA-OAEP-SHA256(512), nonce GCM(12), ciphertext, tag(16).
O cabeçalho inteiro é AAD. Chave AES-256 e nonce novos para cada objeto.
"""

import argparse
import contextlib
import datetime as dt
import errno
import fcntl
import getpass
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import signal
import sqlite3
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from urllib.parse import quote, urlencode
import uuid

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding, rsa
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

MAGIC = b'DNBKP001'
HEADER = 8 + 512 + 12
CHUNK = 1024 * 1024
MAX_BYTES = 256 * 1024 * 1024
OBJECT = re.compile(r'(hub|vault)-\d{8}T\d{6}Z-[0-9a-f]{32}\.dbkp')
# Logins/caches não são dados de trabalho. Não seguir links para fora da árvore.
EXCLUDED = {'.codex', '.claude', '.anthropic', '.cursor', '.continue', '.aider',
            '.azure', '.gcloud', '.config', '.local', '.ssh', '.aws',
            '.gnupg', '.grok', '.gemini', '.opencode', '.claude.json', 'engine-setup', '.git', 'node_modules', 'target', '.venv', '__pycache__',
            'token', 'identity.json', 'auth.json', 'credentials.json', '.netrc',
            '.docker', '.kube', '.npmrc', '.pypirc', '.bash_history', '.zsh_history'}


class BackupError(Exception):
    pass


def require(condition, code):
    if not condition:
        raise BackupError(code)


def control_path(path, field):
    """Resolve aliases de pastas de controle; o último componente nunca é link."""
    path = Path(path)
    require(path.is_absolute(), field + '_exige_absoluto')
    try:
        resolved = path.parent.resolve(strict=False) / path.name
        require(not resolved.is_symlink(), field + '_link_recusado')
        return resolved
    except (OSError, RuntimeError):
        raise BackupError(field + '_caminho_invalido') from None


def private_file(path, field='arquivo'):
    path = control_path(path, field)
    fd = safe_open(path, os.O_RDONLY | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                and stat.S_IMODE(info.st_mode) == 0o600, 'arquivo_exige_modo_600')
        return stream.read()


def config(path):
    cfg = json.loads(private_file(path, 'config'))
    require(cfg['transport'] in ('local', 'r2'), 'transporte_invalido')
    for key in ('state_dir', 'public_key', 'credentials', 'local_store', 'motor_work'):
        if key in cfg and cfg[key] is not None:
            cfg[key] = str(control_path(cfg[key], key))
    if cfg['transport'] == 'r2':
        require(re.fullmatch(r'[0-9a-f]{32}', cfg['account_id']), 'conta_invalida')
        require(re.fullmatch(r'[a-z0-9][a-z0-9-]{1,61}[a-z0-9]', cfg['bucket']), 'bucket_invalido')
        require(cfg.get('jurisdiction', 'default') in ('default', 'eu', 'us'), 'jurisdicao_invalida')
        retention = cfg.get('retention')
        require(retention in ('bucket-lock-indefinite', 'bucket-lock-age'), 'trava_r2_exige_confirmacao_a1')
        if retention == 'bucket-lock-age':
            require(isinstance(cfg.get('lock_days'), int) and cfg['lock_days'] >= 7
                    and isinstance(cfg.get('expiration_days'), int)
                    and cfg['expiration_days'] >= cfg['lock_days'], 'prazos_retencao_invalidos')
        private_file(cfg['credentials'], 'credentials')
    else:
        require(Path(cfg['local_store']).is_absolute(), 'caminho_exige_absoluto')
    return cfg


def digest(path):
    with os.fdopen(safe_open(path, os.O_RDONLY | os.O_NONBLOCK), 'rb') as stream:
        require(stat.S_ISREG(os.fstat(stream.fileno()).st_mode), 'arquivo_especial_recusado')
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def oaep():
    return padding.OAEP(mgf=padding.MGF1(hashes.SHA256()), algorithm=hashes.SHA256(), label=MAGIC)


def seal(source, target, public_key):
    with os.fdopen(safe_open(control_path(public_key, 'public_key'), os.O_RDONLY | os.O_NONBLOCK), 'rb') as stream:
        require(stat.S_ISREG(os.fstat(stream.fileno()).st_mode), 'publica_exige_arquivo_regular')
        key = serialization.load_pem_public_key(stream.read())
    require(isinstance(key, rsa.RSAPublicKey) and key.key_size == 4096, 'publica_exige_rsa_4096')
    aes = os.urandom(32)
    nonce = os.urandom(12)
    header = MAGIC + key.encrypt(aes, oaep()) + nonce
    encryptor = Cipher(algorithms.AES(aes), modes.GCM(nonce)).encryptor()
    encryptor.authenticate_additional_data(header)
    with source.open('rb') as src, target.open('xb') as dst:
        dst.write(header)
        while chunk := src.read(CHUNK):
            dst.write(encryptor.update(chunk))
        dst.write(encryptor.finalize())
        dst.write(encryptor.tag)


def unseal(source, target, private_key):
    pem = private_file(private_key, 'private_key')
    password = getpass.getpass('Senha da chave privada: ').encode() if b'ENCRYPTED' in pem else None
    key = serialization.load_pem_private_key(pem, password=password)
    require(isinstance(key, rsa.RSAPrivateKey) and key.key_size == 4096, 'privada_exige_rsa_4096')
    size = source.stat().st_size
    require(HEADER + 16 < size <= MAX_BYTES + HEADER + 16, 'envelope_invalido')
    with source.open('rb') as src, target.open('xb') as dst:
        header = src.read(HEADER)
        require(header[:8] == MAGIC, 'formato_invalido')
        aes = key.decrypt(header[8:520], oaep())
        src.seek(-16, 2)
        tag = src.read(16)
        src.seek(HEADER)
        decryptor = Cipher(algorithms.AES(aes), modes.GCM(header[520:], tag)).decryptor()
        decryptor.authenticate_additional_data(header)
        remaining = size - HEADER - 16
        while remaining:
            chunk = src.read(min(CHUNK, remaining))
            require(chunk, 'envelope_truncado')
            remaining -= len(chunk)
            dst.write(decryptor.update(chunk))
        # Nenhum tar é aberto antes de autenticar todos os bytes.
        dst.write(decryptor.finalize())


def curl(cfg, method, target, query=None, upload=None, output=None):
    creds = json.loads(private_file(cfg['credentials']))
    require(re.fullmatch(r'[A-Za-z0-9]+', creds['access_key_id'])
            and re.fullmatch(r'[A-Za-z0-9/+=]+', creds['secret_access_key']), 'credenciais_invalidas')
    jurisdiction = cfg.get('jurisdiction', 'default')
    suffix = '' if jurisdiction == 'default' else '.' + jurisdiction
    url = f"https://{cfg['account_id']}{suffix}.r2.cloudflarestorage.com/{cfg['bucket']}"
    if target:
        url += '/' + quote(target, safe='/')
    if query:
        url += '?' + urlencode(query)
    # Configuração pela entrada padrão: nenhum segredo em argv, ambiente ou log.
    options = {'url': url, 'request': method, 'aws-sigv4': 'aws:amz:auto:s3',
               'user': creds['access_key_id'] + ':' + creds['secret_access_key']}
    if upload:
        options['upload-file'] = str(upload)
        options['header'] = 'Content-Type: application/octet-stream'
    if output:
        options['output'] = str(output)
        options['max-filesize'] = str(MAX_BYTES + HEADER + 16)
    payload = ''.join(k + ' = ' + json.dumps(v) + '\n' for k, v in options.items())
    if upload:
        payload += 'header = ' + json.dumps('x-amz-content-sha256: ' + digest(upload)) + '\n'
    result = subprocess.run(['curl', '--disable', '--silent', '--fail', '--proto', '=https',
                             '--connect-timeout', '20', '--max-time', '900',
                             '--config', '-'], input=payload.encode(), stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, timeout=910)
    require(result.returncode == 0, 'r2_falhou')
    return result.stdout


class Storage:
    def __init__(self, cfg):
        self.cfg = cfg

    def transfer(self, key, path, upload=False):
        require(key.startswith('backups/') and OBJECT.fullmatch(key[8:]), 'objeto_invalido')
        if self.cfg['transport'] == 'local':
            root = Path(self.cfg['local_store']) / 'backups'
            root.mkdir(parents=True, exist_ok=True, mode=0o700)
            obj = root / key[8:]
            require(not obj.is_symlink(), 'link_recusado')
            if upload:
                with path.open('rb') as src, obj.open('xb') as dst:
                    shutil.copyfileobj(src, dst)
            else:
                require(obj.stat().st_size <= MAX_BYTES + HEADER + 16, 'objeto_grande')
                shutil.copyfile(obj, path)
        else:
            curl(self.cfg, 'PUT' if upload else 'GET', key,
                 upload=path if upload else None, output=None if upload else path)



def fingerprint(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def excluded(name):
    name = name.lower()
    return (name in EXCLUDED or name == '.env' or name.startswith('.env.')
            or name.endswith(('.pem', '.key')))


# Componentes inteiros, sem combinar substrings em nomes de trabalho.
SENSITIVE_NAME = re.compile(
    r'(?i)^(?:\.mcp\.json|\.git-credentials|\.pgpass|\.vault-token|'
    r'hosts\.ya?ml|cookies(?:\..*)?|id_(?:rsa|dsa|ecdsa|ed25519)(?:\..*)?|'
    r'login data|local state|(?:credentials?|secrets?|token)(?:\.json)?|'
    r'client[_-]secret(?:[_-][a-z0-9._-]+)?\.json|client_secrets\.json|'
    r'service_account[a-z0-9._-]*\.json|application_default_credentials\.json|'
    r'secrets\.toml|credentials\.tfrc\.json|'
    r'private[_-]?key(?:\.json)?|oauth[_-]?creds(?:\.json)?)$')

# Cabeçalho PEM completo, inclusive quando dividido entre blocos de leitura.
PRIVATE_KEY_HEADER = re.compile(rb'-----BEGIN (?:[A-Z0-9][A-Z0-9 -]{0,47} )?PRIVATE KEY-----')


def sensitive_name(name):
    return bool(SENSITIVE_NAME.fullmatch(name) or name.lower().endswith(('.p12', '.pfx')))


def canonical(value):
    if not isinstance(value, str):
        return False
    try:
        return str(uuid.UUID(value)) == value
    except ValueError:
        return False


def upload_bases(bot, workspaces=None):
    """O Hub não move uploads antigos ao trocar cwd, nem os apaga com o bot."""
    bases = ['bots/' + bot + '/workspace', 'bots/' + bot]
    if bot in (workspaces or {}):
        bases.insert(0, workspaces[bot])
    # No modo de motor separado os uploads do bot ficam em work/bots/<id>/uploads,
    # irmãos do workspace em /var/lib/donnabot/trabalho. No legado o caminho não existe.
    bases.append('work/bots/' + bot)
    return list(dict.fromkeys(bases))


def payload_path(logical, workspaces=None):
    """Anexos do dono e publicados não são credenciais de CLI."""
    parts = PurePosixPath(logical).parts
    bases = set((workspaces or {}).values())
    if len(parts) >= 3 and parts[0] == 'bots' and canonical(parts[1]):
        bases.update(upload_bases(parts[1], workspaces))
        if parts[2] == 'files':
            return len(parts) == 3 or (len(parts) == 4 and canonical(parts[3]))
    for base in bases:
        prefix = PurePosixPath(base, 'uploads').parts
        if parts[:len(prefix)] == prefix:
            rest = parts[len(prefix):]
            return not rest or (len(rest) in (1, 2) and canonical(rest[0]))
    # uploads de bots e de conversas de Space no modo de motor separado
    # (work/) e de conversas de Space no layout legado (spaces/).
    uploads_at = []
    if len(parts) >= 4 and parts[:2] == ('work', 'bots') and canonical(parts[2]):
        uploads_at.append(3)  # work/bots/<id>/uploads
    if (len(parts) >= 6 and parts[:2] == ('work', 'spaces') and canonical(parts[2])
            and parts[3] == 'conversations' and canonical(parts[4])):
        uploads_at.append(5)  # work/spaces/<espaco>/conversations/<chat>/uploads
    if (len(parts) >= 6 and parts[0] == 'spaces' and canonical(parts[1])
            and parts[2] == 'conversations' and canonical(parts[3])
            and parts[4] == 'workspace'):
        uploads_at.append(5)  # spaces/<espaco>/conversations/<chat>/workspace/uploads
    for at in uploads_at:
        if parts[at] == 'uploads':
            rest = parts[at + 1:]
            return not rest or (len(rest) in (1, 2) and canonical(rest[0]))
    return False


@contextlib.contextmanager
def safe_parent(path):
    """Abre cada componente sem seguir links, inclusive durante troca concorrente."""
    require(path.is_absolute(), 'caminho_exige_absoluto')
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parent.parts[1:]:
            try:
                child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            except OSError as error:
                if error.errno in (errno.ENOTDIR, errno.ELOOP):
                    raise BackupError('componente_pai_link_ou_nao_diretorio') from None
                raise
            os.close(fd)
            fd = child
        yield fd
    finally:
        os.close(fd)


def safe_open(path, flags):
    path = Path(path)
    with safe_parent(path) as parent:
        return os.open(path.name, flags | os.O_NOFOLLOW, 0o600, dir_fd=parent)


def safe_lstat(path):
    with safe_parent(path) as parent:
        return os.stat(path.name, dir_fd=parent, follow_symlinks=False)


def safe_readlink(path):
    with safe_parent(path) as parent:
        return os.readlink(path.name, dir_fd=parent)


def safe_children(path):
    fd = safe_open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        return [path / name for name in sorted(os.listdir(fd))]
    finally:
        os.close(fd)


def inventory(roots, workspaces=None, not_read=None):
    result = {}
    # Uma raiz contida em outra já tem representação; nunca percorrer duas vezes.
    unique = []
    for source, logical in sorted(roots, key=lambda r: len(r[0].parts)):
        if not any(source == parent or parent in source.parents for parent, _ in unique):
            unique.append((source, logical))
    for source, logical in unique:
        def visit(path, relative):
            try:
                info = safe_lstat(path)
                # Perfis do motor nunca entram como árvores, mesmo sob uploads.
                if (path != source and stat.S_ISDIR(info.st_mode)
                        and path.name.lower() in ('.codex', '.claude', '.grok', '.anthropic',
                                                  '.gemini', '.opencode', 'engine-setup')):
                    if not_read is not None:
                        not_read.append({'path': str(relative), 'reason': 'excluded_profile_tree'})
                    return
                # Só árvores aninhadas; arquivos/links e raízes explícitas mantêm a política anterior.
                if (path != source and stat.S_ISDIR(info.st_mode)
                        and path.name.lower().startswith(('.mexerica', '.donnabot', '.codync'))
                        and not payload_path(relative, workspaces)):
                    if not_read is not None:
                        not_read.append({'path': str(relative), 'reason': 'excluded_private_tree'})
                    return
                require(str(relative) not in result, 'arquivo_duplicado')
                require(stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode)
                        or stat.S_ISLNK(info.st_mode), 'arquivo_especial_recusado')
                result[str(relative)] = (path, fingerprint(info))
                if stat.S_ISDIR(info.st_mode):
                    for child in safe_children(path):
                        if (payload_path(relative / child.name, workspaces)
                                or not (excluded(child.name) or sensitive_name(child.name))):
                            visit(child, relative / child.name)
                        elif not_read is not None:
                            not_read.append({'path': str(relative / child.name), 'reason': 'excluded_name'})
            except FileNotFoundError:
                if not payload_path(relative, workspaces):
                    raise
                result.pop(str(relative), None)
                if not_read is not None:
                    not_read.append({'path': str(relative), 'reason': 'disappeared_during_inventory'})
        try:
            if (PurePosixPath(logical).parts[0] == 'external'
                    and (excluded(source.name) or sensitive_name(source.name))):
                if not_read is not None:
                    not_read.append({'path': logical, 'reason': 'excluded_root_name'})
                continue
            try:
                safe_lstat(source)
            except FileNotFoundError:
                continue
            visit(source, PurePosixPath(logical))
        except PermissionError:
            parts = PurePosixPath(logical).parts
            label = '/'.join(parts[:2]) if parts[0] == 'work' else parts[0]
            raise BackupError('raiz_sem_permissao:' + label) from None
    require(sum(v[1][3] for v in result.values() if stat.S_ISREG(v[1][2])) <= MAX_BYTES, 'dados_grandes')
    return result


def stable_inventory(items, workspaces=None):
    # mtime de diretório muda ao apagar um anexo. Os demais arquivos ainda são
    # conferidos integralmente; anexos têm a versão efetivamente copiada no manifesto.
    return {name: (path, fp[:3] if stat.S_ISDIR(fp[2]) else fp)
            for name, (path, fp) in items.items() if not payload_path(name, workspaces)}


def copy_inventory(items, stage, workspaces=None, not_read=None, filter_private_keys=False):
    total = 0
    for logical, (path, before) in items.items():
        target = stage / logical
        volatile = payload_path(logical, workspaces)
        try:
            target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            if stat.S_ISDIR(before[2]):
                require(fingerprint(safe_lstat(path))[:3] == before[:3] or volatile, 'dados_mudaram')
                target.mkdir(exist_ok=True, mode=0o700)
                continue
            if stat.S_ISLNK(before[2]):
                # Guarda o texto do link, inclusive venv com alvo absoluto. Nunca o abre.
                target.symlink_to(safe_readlink(path))
                if not volatile:
                    require(fingerprint(safe_lstat(path)) == before, 'dados_mudaram')
                continue
            fd = safe_open(path, os.O_RDONLY | os.O_NONBLOCK)
            with os.fdopen(fd, 'rb') as src, target.open('xb') as dst:
                info = os.fstat(src.fileno())
                require(stat.S_ISREG(info.st_mode), 'arquivo_especial_recusado')
                require(volatile or fingerprint(info) == before, 'dados_mudaram')
                tail = b''
                private_key = False
                while chunk := src.read(CHUNK):
                    total += len(chunk)
                    require(total <= MAX_BYTES, 'dados_grandes')
                    if filter_private_keys and not volatile:
                        private_key = private_key or bool(PRIVATE_KEY_HEADER.search(tail + chunk))
                        tail = chunk[-96:]
                    if not private_key:
                        dst.write(chunk)
                require(volatile or fingerprint(os.fstat(src.fileno())) == before, 'dados_mudaram')
            if private_key:
                target.unlink()
                if not_read is not None:
                    not_read.append({'path': logical, 'reason': 'excluded_private_key'})
                continue
            # Somente rwx; nunca reintroduzir setuid/setgid/sticky do backup.
            target.chmod(stat.S_IMODE(info.st_mode) & 0o777)
        except OSError as error:
            if error.errno in (errno.EACCES, errno.EPERM):
                parts = PurePosixPath(logical).parts
                label = '/'.join(parts[:2]) if parts[0] == 'work' else parts[0]
                raise BackupError('raiz_sem_permissao:' + label) from None
            if not volatile or error.errno != errno.ENOENT:
                raise
            if target.is_symlink() or target.is_file():
                target.unlink()
            if not_read is not None:
                not_read.append({'path': logical, 'reason': 'disappeared_during_copy'})
    # Modos das pastas são aplicados após a escrita dos filhos, no pack/unpack.
    return {name: fp[2] & 0o777 for name, (_, fp) in items.items()
            if stat.S_ISDIR(fp[2]) and (stage / name).is_dir()}


def connect(path):
    require(path.is_file() and not path.is_symlink(), 'banco_ausente')
    return sqlite3.connect(path.as_uri() + '?mode=ro', uri=True, timeout=10)


def host_lock_holders(path):
    """None: trava ausente ou livre; senão, argv dos detentores do flock."""
    try:
        info = safe_lstat(path)
    except FileNotFoundError:
        return None
    require(stat.S_ISREG(info.st_mode), 'host_lock_invalido')
    fd = None
    for flags in (os.O_RDWR | os.O_NONBLOCK, os.O_RDONLY | os.O_NONBLOCK):
        try:
            fd = safe_open(path, flags)
            break
        except OSError:
            continue
    if fd is not None:
        try:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                pass  # Contenção ou erro indeterminado: /proc/locks decide.
            else:
                return None
        finally:
            os.close(fd)
    # Travado ou indeterminado: /proc/locks liga o inode aos pids; fora do Linux
    # fica indeterminado e a decisão recai sobre o classificador dos detentores.
    try:
        locks = Path('/proc/locks').read_text()
    except OSError:
        raise BackupError('host_lock_verificacao_indisponivel') from None
    device = (os.major(info.st_dev), os.minor(info.st_dev), info.st_ino)
    pids = []
    for line in locks.splitlines():
        fields = line.split()
        if len(fields) < 6 or fields[1] != 'FLOCK':
            continue
        try:
            major, minor, inode = fields[5].split(':')
            pid = int(fields[4])
        except ValueError:
            continue
        # /proc/locks: dispositivo em hex, inode em decimal.
        if (int(major, 16), int(minor, 16), int(inode)) == device:
            pids.append(pid)
    require(pids, 'host_lock_detentor_indeterminado')
    require(fingerprint(safe_lstat(path))[:2] == fingerprint(info)[:2], 'host_lock_mudou')
    holders = []
    for pid in dict.fromkeys(pids):
        try:
            raw = Path('/proc/%d/cmdline' % pid).read_bytes()
        except OSError:
            holders.append(['<pid-desconhecido:%d>' % pid])
            continue
        args = [arg for arg in raw.decode('utf-8', 'replace').split('\0') if arg]
        holders.append(args or ['<pid-desconhecido:%d>' % pid])
    return holders


def hub_lock_allows_backup(holders):
    """O daemon serve pode coexistir com a cópia estável; manutenção não."""
    if not holders:
        return True
    return all('serve' in args[1:] and any(Path(arg).name == 'donnabot-host' for arg in args)
               for args in holders)


def stats(db):
    require(db.execute('PRAGMA integrity_check').fetchall() == [('ok',)], 'integridade_falhou')
    return {'bots': db.execute('SELECT count(*) FROM bots WHERE deleted=0').fetchone()[0],
            # Cada bot tem seu chat principal. Entradas preservam turnos e conversas.
            'conversas': db.execute('SELECT count(DISTINCT bot_id) FROM entries').fetchone()[0],
            'entradas': db.execute('SELECT count(*) FROM entries').fetchone()[0]}


def check_snapshot_guard(home, db_identity=None):
    require(hub_lock_allows_backup(host_lock_holders(home / 'host.lock')), 'hub_em_manutencao')
    info = safe_lstat(home / 'codync.db')
    require(stat.S_ISREG(info.st_mode), 'banco_invalido')
    identity = (info.st_dev, info.st_ino)
    require(db_identity is None or identity == db_identity, 'banco_substituido_durante_backup')
    return identity


def allowed_root(path, home):
    # Nem uma raiz explícita pode apontar para/abranger os perfis do motor.
    forbidden = (Path('/var/lib/donnabot/motor'), home / 'engine-setup' / 'accounts')
    require(not any(path == item or item in path.parents or path in item.parents
                    for item in forbidden), 'raiz_sobrepoe_perfil_motor')
    require(not any(part.lower() in ('.codex', '.claude', '.grok', '.anthropic',
                                    '.gemini', '.opencode', 'engine-setup')
                    for part in path.parts), 'raiz_perfil_motor_recusada')


def validate_hub_home(home):
    motor = Path('/var/lib/donnabot/motor')
    require(home != motor and motor not in home.parents, 'hub_home_perfil_motor_recusado')
    require(not any(part.lower() in ('.codex', '.claude', '.grok', '.anthropic',
                                    '.gemini', '.opencode', 'engine-setup')
                    for part in home.parts), 'hub_home_perfil_motor_recusado')


def motor_workspace_allowed(relative, bot_id):
    parts = relative.parts
    return (len(parts) >= 3 and parts[:3] == ('bots', bot_id, 'workspace')
            or len(parts) >= 5 and parts[0] == 'spaces' and canonical(parts[1])
            and parts[2] == 'conversations' and canonical(parts[3]) and parts[4] == 'workspace')


def hub_snapshot(cfg, stage):
    require(Path(cfg['hub_home']).is_absolute(), 'caminho_exige_absoluto')
    home = Path(cfg['hub_home']).resolve()
    validate_hub_home(home)
    # uma janela de manutenção (migração/instalação) segura host.lock e
    # reescreve as árvores; o daemon serve pode coexistir com a cópia estável.
    db_identity = check_snapshot_guard(home)
    with contextlib.closing(connect(home / 'codync.db')) as src:
        check_snapshot_guard(home, db_identity)
        version = src.execute('PRAGMA data_version').fetchone()[0]
        with contextlib.closing(sqlite3.connect(stage / 'codync.db')) as db:
            deadline = time.monotonic() + 120
            page_size = src.execute('PRAGMA page_size').fetchone()[0]
            def progress(status, remaining, total):
                require(time.monotonic() < deadline, 'snapshot_timeout')
                require(total * page_size <= MAX_BYTES, 'banco_grande')
                check_snapshot_guard(home, db_identity)
            src.backup(db, pages=256, progress=progress)
            check_snapshot_guard(home, db_identity)
            counts = stats(db)
            roots = [(home / 'shared-memory', 'shared-memory'), (home / 'skills', 'skills'),
                     (home / 'harness-context.json', 'harness-context.json'),
                     (home / 'spaces', 'spaces'), (home / 'grupos', 'grupos'),
                     (home / 'mexeriquinhas' / 'gerados', 'mexeriquinhas/gerados')]
            # modo de motor separado (/etc/donnabot/motor.conf). Os dados
            # do usuário ficam em motor_work; motor/ (perfis e credenciais) e
            # /opt/donnabot (binários) ficam fora por decisão de projeto.
            work = None
            if cfg.get('motor_work'):
                require(Path(cfg['motor_work']).is_absolute(), 'motor_work_exige_absoluto')
                work = Path(cfg['motor_work']).resolve()
                require(work != home and home not in work.parents and work not in home.parents,
                        'motor_work_sobrepoe_hub')
                require(work.is_dir(), 'motor_work_ausente')
                allowed_root(work, home)
                for name in ('bots', 'spaces', 'skills'):
                    subtree = work / name
                    if subtree.exists() or subtree.is_symlink():
                        require(not subtree.is_symlink() and subtree.is_dir(), 'motor_work_layout_invalido')
                roots += [(work / 'bots', 'work/bots'), (work / 'spaces', 'work/spaces'),
                          (work / 'skills', 'work/skills')]
            external = []
            workspaces = {}
            workspace_paths = {}
            allowed = {str(Path(p).resolve()) for p in cfg.get('external_workspaces', [])}
            for bot_id, raw, deleted in db.execute('SELECT id,config,deleted FROM bots'):
                require(str(uuid.UUID(bot_id)) == bot_id, 'id_bot_invalido')
                roots.append((home / 'bots' / bot_id, 'bots/' + bot_id))
                bot = json.loads(raw)
                cwd = bot.get('cwd', '')
                if cwd and not deleted:
                    path = Path(cwd)
                    require(path.is_absolute() and not path.is_symlink(), 'workspace_invalido')
                    bot_root = home / 'bots' / bot_id
                    require(path.resolve() == path, 'workspace_invalido')
                    allowed_root(path, home)
                    if path == bot_root or bot_root in path.parents:
                        workspaces[bot_id] = str(path.relative_to(home))
                    elif work is not None and (path == work or work in path.parents):
                        relative = path.relative_to(work)
                        require(motor_workspace_allowed(relative, bot_id), 'motor_work_workspace_recusado')
                    else:
                        require(str(path.resolve()) in allowed and path.resolve() != home
                                and path.resolve() != Path.home(), 'workspace_externo_exige_lista')
                        allowed_root(path, home)
                        roots.append((path, 'external/' + bot_id))
                        external.append(bot_id)
                        workspaces[bot_id] = 'external/' + bot_id
                    require(path.is_dir(), 'workspace_ausente')
                    workspace_paths[bot_id] = path
            # conversas de Space publicam arquivos em bots/<chat>/files e
            # camadas de memória de Space são UUIDs em kv → bots/<uuid>/bot-memory.
            has_conversations = db.execute("SELECT 1 FROM sqlite_master WHERE type='table'"
                                           " AND name='donna_space_conversations'").fetchone()
            if has_conversations:
                for (chat,) in db.execute('SELECT id FROM donna_space_conversations'):
                    require(canonical(chat), 'id_conversa_invalido')
                    roots.append((home / 'bots' / chat, 'bots/' + chat))
            for (layer,) in db.execute("SELECT v FROM kv WHERE k GLOB 'donna.space.memory.*'"):
                require(canonical(layer), 'camada_memoria_invalida')
                roots.append((home / 'bots' / layer, 'bots/' + layer))
            for bot_id, path in workspace_paths.items():
                source, logical = next((source, logical) for source, logical in sorted(roots, key=lambda r: len(r[0].parts))
                                       if source == path or source in path.parents)
                workspaces[bot_id] = str(PurePosixPath(logical) / path.relative_to(source))
            not_read = []
            before = inventory(roots, workspaces, not_read)
            directory_modes = copy_inventory(before, stage, workspaces, not_read, True)
            require(stable_inventory(before, workspaces) == stable_inventory(inventory(roots, workspaces), workspaces)
                    and version == src.execute('PRAGMA data_version').fetchone()[0], 'dados_mudaram')
            check_snapshot_guard(home, db_identity)
            excluded_count = sum(item['reason'].startswith('excluded_') for item in not_read)
            if excluded_count:
                print(json.dumps({'warning': 'itens_excluidos_do_backup', 'count': excluded_count}),
                      file=sys.stderr, flush=True)
            references = reference_manifest(db, stage, workspaces)
            # Os grants antigos não viram credenciais de um computador restaurado.
            db.execute('PRAGMA secure_delete=ON')
            db.execute("DELETE FROM kv WHERE k='token'")
            for table in ('devices', 'push_tickets', 'activity_tickets'):
                if db.execute('SELECT 1 FROM sqlite_master WHERE type=\'table\' AND name=?', (table,)).fetchone():
                    db.execute('DELETE FROM ' + table)
            db.commit()
            db.execute('PRAGMA journal_mode=DELETE')
            db.execute('VACUUM')
        return {'counts': counts, 'external': external, 'workspaces': workspaces,
                'references': references, 'directory_modes': directory_modes, 'not_read': not_read}


def reference_manifest(db, stage, workspaces):
    references = []
    for bot, raw in db.execute('SELECT bot_id,data FROM entries ORDER BY rowid'):
        require(str(uuid.UUID(bot)) == bot, 'id_bot_invalido')
        data = json.loads(raw)
        for field in ('files', 'attachments'):
            for item in data.get(field, []):
                file_id = str(uuid.UUID(item['id']))
                if field == 'files':
                    candidates = [PurePosixPath('bots', bot, 'files', file_id)]
                else:
                    name = item['name']
                    require(name and '/' not in name and '\\' not in name
                            and name not in ('.', '..'), 'anexo_invalido')
                    bases = upload_bases(bot, workspaces)
                    candidates = [PurePosixPath(base, 'uploads', file_id, name) for base in bases]
                    # anexos de conversas de Space ficam na pasta da
                    # conversa, não na do bot; os dois layouts são candidatos.
                    space, chat = item.get('spaceId'), item.get('conversationId')
                    if canonical(space) and canonical(chat):
                        candidates = [
                            PurePosixPath('spaces', space, 'conversations', chat,
                                          'workspace', 'uploads', file_id, name),
                            PurePosixPath('work', 'spaces', space, 'conversations', chat,
                                          'uploads', file_id, name),
                        ] + candidates
                # Não seguir nem um link de diretório intermediário para ler um anexo.
                selected = candidates[0]
                record = {'bot': bot, 'type': field, 'id': file_id, 'status': 'missing'}
                for relative in candidates:
                    path = stage / relative
                    if any(parent.is_symlink() for parent in (path, *path.parents) if parent != stage):
                        continue
                    if path.is_file():
                        selected = relative
                        record.update(status='copied', size=path.stat().st_size, sha256=digest(path))
                        record['matches_entry'] = (record['size'] == item['size']
                                                   and ('sha256' not in item or record['sha256'] == item['sha256']))
                        break
                record['path'] = str(selected)
                references.append(record)
    return references


def validate_references(db, stage, meta):
    require(reference_manifest(db, stage, meta['workspaces']) == meta['references'], 'referencias_divergentes')


def pack(stage, target, meta):
    paths = sorted(stage.rglob('*'))  # pathlib não percorre diretórios symlink.
    files = {str(p.relative_to(stage)): {'sha256': digest(p), 'mode': p.stat().st_mode & 0o777}
             for p in paths if not p.is_symlink() and p.is_file()}
    links = {str(p.relative_to(stage)): os.readlink(p) for p in paths if p.is_symlink()}
    directory_modes = meta.pop('directory_modes', {})
    directories = {str(p.relative_to(stage)): directory_modes.get(str(p.relative_to(stage)), 0o700)
                   for p in paths if not p.is_symlink() and p.is_dir()}
    require(sum(p.stat().st_size for p in paths if not p.is_symlink() and p.is_file()) <= MAX_BYTES, 'dados_grandes')
    meta.update({'format': 2, 'created_at': dt.datetime.now(dt.timezone.utc).isoformat(),
                 'files': files, 'directories': directories, 'links': links})
    (stage / 'manifest.json').write_text(json.dumps(meta, ensure_ascii=False))
    with tarfile.open(target, 'w:gz') as archive:
        for path in sorted(stage.rglob('*')):
            info = archive.gettarinfo(str(path), arcname=str(path.relative_to(stage)))
            if info.isdir():
                info.mode = directories[info.name]
            if info.isfile():
                with path.open('rb') as src:
                    archive.addfile(info, src)
            else:
                archive.addfile(info)
    require(target.stat().st_size <= MAX_BYTES, 'pacote_grande')


def unpack(source, stage):
    total = 0
    links = {}
    files = {}
    directories = {}
    with tarfile.open(source, 'r:gz') as archive:
        seen = set()
        for member in archive:
            name = PurePosixPath(member.name)
            require((member.isfile() or member.isdir() or member.issym()) and not name.is_absolute()
                    and '..' not in name.parts and str(name) == member.name
                    and member.name not in seen, 'tar_invalido')
            seen.add(member.name)
            total += member.size
            require(total <= MAX_BYTES + 16 * 1024 * 1024, 'tar_grande')
            target = stage / member.name
            if member.issym():
                links[member.name] = member.linkname
                continue  # Criar links só depois de todos os arquivos e verificações.
            target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            if member.isdir():
                target.mkdir(exist_ok=True, mode=0o700)
                directories[member.name] = member.mode & 0o777
                continue
            files[member.name] = member.mode & 0o777
            with archive.extractfile(member) as src, target.open('xb') as dst:
                shutil.copyfileobj(src, dst)
            target.chmod(member.mode & 0o777)
    # Recusa link com qualquer descendente, independentemente da ordem do tar.
    require(not any(PurePosixPath(link) in PurePosixPath(name).parents
                    for link in links for name in seen), 'tar_invalido')
    meta = json.loads((stage / 'manifest.json').read_text())
    require(meta['format'] == 2 and set(meta['files']) == set(files) - {'manifest.json'}
            and meta['directories'] == directories and meta['links'] == links, 'manifesto_invalido')
    for name, expected in meta['files'].items():
        require(digest(stage / name) == expected['sha256'], 'hash_falhou')
        require(files[name] == expected['mode'], 'modo_divergente')
    for name, link in links.items():
        target = stage / name
        target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        target.symlink_to(link)
    # A pasta temporária continua privada (700). Modos de diretório são
    # restabelecidos só na promoção; não atrapalham a conferência dos arquivos.
    return meta


def durable_json(path, value):
    tmp = path.with_name(path.name + '-' + uuid.uuid4().hex)
    with tmp.open('x') as stream:
        json.dump(value, stream)
        stream.flush()
        os.fsync(stream.fileno())
    tmp.replace(path)
    fd = safe_open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def finish_pending(cfg, kind, started):
    pending = Path(cfg['state_dir']) / (kind + '-pending')
    record = json.loads(private_file(pending / 'point.json'))
    require(kind != 'hub' or record.get('credential_policy') == 1,
            'pendente_hub_politica_antiga')
    encrypted = pending / 'encrypted.dbkp'
    require(record['bytes'] == encrypted.stat().st_size and record['sha256'] == digest(encrypted), 'pendente_divergente')
    storage = Storage(cfg)
    if not record['put_started']:
        # Persistir ANTES da rede: uma resposta incerta nunca causa outro PUT.
        record['put_started'] = True
        durable_json(pending / 'point.json', record)
        storage.transfer(record['object'], encrypted, upload=True)
    downloaded = pending / 'readback.dbkp'
    downloaded.unlink(missing_ok=True)
    storage.transfer(record['object'], downloaded)
    require(digest(encrypted) == digest(downloaded), 'envio_divergente')
    result = {key: record[key] for key in ('object', 'bytes', 'point_at')}
    # Não perder o rastreio se faltar disco ao registrar sucesso após o GET.
    operation = 'backup' if kind == 'hub' else 'vault-backup'
    durable_json(Path(cfg['state_dir']) / (operation + '.json'),
                 {'ok': True, 'operation': operation, 'at': dt.datetime.now(dt.timezone.utc).isoformat(),
                  'duration_seconds': round(time.monotonic() - started, 3), **result})
    shutil.rmtree(pending)
    return result


def backup(cfg, work, keyrings=None):
    started = time.monotonic()
    kind = 'vault' if keyrings else 'hub'
    if (Path(cfg['state_dir']) / (kind + '-pending')).exists():
        return finish_pending(cfg, kind, started)
    stage = work / 'stage'
    stage.mkdir(mode=0o700)
    if keyrings:
        require(Path(keyrings).is_absolute() and not Path(keyrings).is_symlink(), 'cofre_invalido')
        roots = [(Path(keyrings).resolve(), 'keyrings')]
        require(roots[0][0].is_dir(), 'cofre_ausente')
        before = inventory(roots)
        require(before, 'cofre_vazio')
        directory_modes = copy_inventory(before, stage)
        require(before == inventory(roots), 'dados_mudaram')
        meta = {'kind': kind, 'directory_modes': directory_modes}
    else:
        meta = {'kind': kind, **hub_snapshot(cfg, stage)}
    archive = work / 'package.tar.gz'
    pack(stage, archive, meta)
    encrypted = work / 'encrypted.dbkp'
    seal(archive, encrypted, cfg['public_key'])
    key = f"backups/{kind}-{dt.datetime.now(dt.timezone.utc):%Y%m%dT%H%M%SZ}-{uuid.uuid4().hex}.dbkp"
    # Só o envelope cifrado e metadados opacos sobrevivem à tentativa. A pasta
    # publicada atomicamente identifica o mesmo ponto em todas as repetições.
    state = Path(cfg['state_dir'])
    with tempfile.TemporaryDirectory(prefix='pending-stage-', dir=state) as directory:
        pending = Path(directory)
        shutil.copyfile(encrypted, pending / 'encrypted.dbkp')
        with (pending / 'encrypted.dbkp').open('rb') as stream:
            os.fsync(stream.fileno())
        durable_json(pending / 'point.json', {'object': key, 'bytes': encrypted.stat().st_size,
                     'sha256': digest(encrypted), 'point_at': meta['created_at'], 'put_started': False,
                     'credential_policy': 1 if kind == 'hub' else None})
        pending.rename(state / (kind + '-pending'))
        fd = safe_open(state, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    return finish_pending(cfg, kind, started)


def remove_private_tree(path):
    """Limpeza inclusive de diretórios já promovidos com modo 0555/0000."""
    if not path.exists():
        return
    path.chmod(0o700)
    for parent, directories, _ in os.walk(path, followlinks=False):
        for name in directories:
            child = Path(parent) / name
            if not child.is_symlink():
                child.chmod(0o700)
    shutil.rmtree(path)


def restore(cfg, work, key, private_key, destination):
    destination = Path(destination)
    require(destination.is_absolute() and not destination.exists() and not destination.is_symlink(), 'destino_exige_pasta_nova')
    parent = destination.parent.resolve(strict=True)
    destination = parent / destination.name
    live = Path(cfg.get('hub_home', str(Path.home() / '.mexerica'))).resolve()
    require(not (parent == live or live in parent.parents), 'destino_hub_recusado')
    encrypted = work / 'download.dbkp'
    Storage(cfg).transfer(key, encrypted)
    archive = work / 'package.tar.gz'
    unseal(encrypted, archive, private_key)
    stage = work / 'restored'
    stage.mkdir(mode=0o700)
    meta = unpack(archive, stage)
    require(meta['kind'] == key[8:].split('-')[0], 'tipo_divergente')
    result = {'files': len(meta['files']), 'kind': meta['kind'], 'object': key,
              'created_at': meta['created_at'], 'bytes': encrypted.stat().st_size}
    if meta['kind'] == 'hub':
        with contextlib.closing(connect(stage / 'codync.db')) as db:
            require(stats(db) == meta['counts'], 'contagens_divergentes')
            validate_references(db, stage, meta)
            require(db.execute("SELECT 1 FROM kv WHERE k='token'").fetchone() is None, 'token_antigo_recusado')
        result.update(meta['counts'])
    # Nenhum start, migração, remapeamento de cwd ou import no Secret Service.
    # A criação exclusiva não sobrescreve um destino concorrente.
    os.mkdir(destination, 0o700)
    try:
        for child in stage.iterdir():
            shutil.move(str(child), str(destination / child.name))
        # Mover entre discos pode virar copytree+rmtree. A origem permanece
        # gravável; só o destino recebe os modos finais, filhos antes dos pais.
        for name, mode in sorted(meta['directories'].items(), key=lambda x: -len(PurePosixPath(x[0]).parts)):
            (destination / name).chmod(mode)
    except BaseException:
        try:
            remove_private_tree(destination)
        except OSError:
            raise BackupError('promocao_falhou_limpeza_incompleta') from None
        raise BackupError('promocao_falhou_destino_removido') from None
    return result


def utc_date(value, code):
    try:
        at = dt.datetime.fromisoformat(value)
        return at.replace(tzinfo=dt.timezone.utc) if at.tzinfo is None else at.astimezone(dt.timezone.utc)
    except (ValueError, TypeError):
        raise BackupError(code) from None


def check_health(cfg):
    now = dt.datetime.now(dt.timezone.utc)
    alerts = []
    for operation in ('backup', 'vault-backup'):
        try:
            path = Path(cfg['state_dir']) / (operation + '.json')
            require(path.is_file(), operation + '_sem_sucesso')
            status = json.loads(path.read_text())
            at = utc_date(status['at'], operation + '_data_invalida')
            point_at = utc_date(status.get('point_at', status['at']), operation + '_data_invalida')
            if not (status['ok'] and dt.timedelta(0) <= now - point_at <= dt.timedelta(hours=26)):
                alerts.append(operation + '_atrasado_26h')
            failure = path.with_name(operation + '-failure.json')
            if failure.exists() and utc_date(json.loads(failure.read_text())['at'], operation + '_data_invalida') >= at:
                alerts.append(operation + '_ultima_tentativa_falhou')
        except BackupError as error:
            alerts.append(str(error))
        except (OSError, ValueError, KeyError, TypeError):
            alerts.append(operation + '_estado_invalido')
    if cfg.get('credential_expires_at'):
        try:
            expires = utc_date(cfg['credential_expires_at'], 'validade_r2_data_invalida')
            if expires - now <= dt.timedelta(days=14):
                alerts.append('token_r2_expira_em_14_dias')
        except BackupError as error:
            alerts.append(str(error))
    if alerts:
        error = BackupError(alerts[0])
        error.alerts = alerts
        raise error
    return {'status': 'em_dia'}


def interrupted(signum, frame):
    raise BackupError('interrompido_' + signal.Signals(signum).name)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('backup', 'vault-backup', 'restore', 'check-health'))
    parser.add_argument('--config', required=True, type=Path)
    parser.add_argument('--keyrings', type=Path)
    parser.add_argument('--object')
    parser.add_argument('--private-key', type=Path)
    parser.add_argument('--destination', type=Path)
    args = parser.parse_args()
    state = None
    started = time.monotonic()
    try:
        cfg = config(args.config)
        state = control_path(cfg['state_dir'], 'state_dir')
        try:
            state.mkdir(parents=True, exist_ok=True, mode=0o700)
        except OSError as error:
            raise BackupError('state_dir_criacao_falhou_errno_' + str(error.errno)) from None
        require(not state.is_symlink() and stat.S_IMODE(state.stat().st_mode) == 0o700
                and state.stat().st_uid == os.getuid(), 'estado_exige_modo_700')
        with os.fdopen(safe_open(state / (args.command + '-job.lock'), os.O_WRONLY | os.O_CREAT | os.O_APPEND), 'a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with tempfile.TemporaryDirectory(prefix='job-', dir=state) as directory:
                work = Path(directory)
                if args.command == 'check-health':
                    result = check_health(cfg)
                elif args.command == 'restore':
                    require(args.object and args.private_key and args.destination, 'argumentos_ausentes')
                    result = restore(cfg, work, args.object, args.private_key, args.destination)
                else:
                    require(args.command != 'vault-backup' or args.keyrings, 'argumentos_ausentes')
                    result = backup(cfg, work, args.keyrings if args.command == 'vault-backup' else None)
        status = {'ok': True, 'operation': args.command, 'at': dt.datetime.now(dt.timezone.utc).isoformat(),
                  'duration_seconds': round(time.monotonic() - started, 3), **result}
        # Status da operação não inclui caminhos, conteúdo, nomes ou contagens.
        health = {k: v for k, v in status.items() if k not in ('bots', 'conversas', 'entradas', 'files')}
        if args.command not in ('backup', 'vault-backup'):
            durable_json(state / (args.command + '.json'), health)
        print(json.dumps(health))
        return 0
    except Exception as error:
        code = str(error) if isinstance(error, BackupError) else type(error).__name__
        status = {'ok': False, 'operation': args.command, 'error': code,
                  'at': dt.datetime.now(dt.timezone.utc).isoformat()}
        if hasattr(error, 'alerts'):
            status['alerts'] = error.alerts
        if state and state.is_dir() and not state.is_symlink():
            # Não sobrescrever o último sucesso: a tentativa tem arquivo próprio.
            with contextlib.suppress(OSError):
                (state / (args.command + '-failure.json')).write_text(json.dumps(status))
        print(json.dumps(status), file=sys.stderr)
        return 1


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    sys.exit(main())
