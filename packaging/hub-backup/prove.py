#!/usr/bin/env python3
"""Prova apenas com fixtures; cria uma pasta nova e a remove ao terminar."""
import argparse
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import signal
import sys
import tarfile
import time
from unittest.mock import patch
import uuid

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import rsa

spec = importlib.util.spec_from_file_location('backup', Path(__file__).with_name('backup.py'))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)


def refused(fn, expected=b.BackupError, label=''):
    try:
        fn()
    except expected:
        return
    raise AssertionError('operação deveria ter sido recusada: ' + label)


def prove_private_roots(root):
    """D520: diretórios privados ficam fora; arquivos/recibos sintéticos são copiados."""
    root.mkdir()
    try:
        for family in ('.mexerica', '.donnabot', '.codync', '.MEXERICA', '.DONNABOT', '.CODYNC'):
            for suffix in ('', '-dev', '-dev-teste', '-backup'):
                private = root / (family + suffix)
                private.mkdir()
                (private / 'segredo-ficticio').write_text('FICTICIO_D520')
        (root / 'trabalho.txt').write_text('PUBLICO_FICTICIO')
        skill = root / 'skills' / 'skill-ficticia'
        skill.mkdir(parents=True)
        receipt = 'workspace/skills/skill-ficticia/.donnabot-receipt.json'
        receipt_bytes = b'{"fixture":"D520-R1-F1-publica"}\n'
        (skill / '.donnabot-receipt.json').write_bytes(receipt_bytes)
        regular_files = {
            'workspace/skills/skill-ficticia/.mexerica-publico.txt': b'PUBLICO_MEXERICA',
            'workspace/skills/skill-ficticia/.codync-publico.txt': b'PUBLICO_CODYNC',
        }
        for logical, data in regular_files.items():
            (skill / Path(logical).name).write_bytes(data)
        items = b.inventory([(root, 'workspace')])
        # Primeiro assert: no head 8e, o RED deve ser a ausência real do recibo.
        assert receipt in items, 'F1: recibo sintetico da skill ausente do inventario'
        assert set(items) == {
            'workspace', 'workspace/trabalho.txt', 'workspace/skills',
            'workspace/skills/skill-ficticia', receipt, *regular_files,
        }, set(items)
        # Desaparecimento fora de uploads não pode omitir a raiz silenciosamente.
        real_lstat = b.safe_lstat
        def removed_during_inventory(path):
            if path == root / 'trabalho.txt':
                raise FileNotFoundError('fixture')
            return real_lstat(path)
        with patch.object(b, 'safe_lstat', side_effect=removed_during_inventory):
            refused(lambda: b.inventory([(root, 'workspace')]), FileNotFoundError)
        stage = root / 'copia-ficticia'
        b.copy_inventory(items, stage)
        assert (stage / receipt).read_bytes() == receipt_bytes, 'F1: copia alterou o recibo sintetico'
        for logical, data in regular_files.items():
            assert (stage / logical).read_bytes() == data
        for old in ('.ssh', '.codex', '.claude', '.env', '.env.dev', 'chave.pem', 'chave.key'):
            assert b.excluded(old), old
    finally:
        shutil.rmtree(root)


def secret_service_phase(phase, root):
    """Só é chamado dentro de dbus-run-session com HOME/XDG fictícios."""
    control = root / 'control'
    control.mkdir(exist_ok=True)
    daemon = ['gnome-keyring-daemon', '--control-directory=' + str(control)]
    # A senha pertence exclusivamente ao cofre fictício, nunca ao usuário do Hub.
    result = subprocess.run(daemon + ['--unlock', '--components=secrets'], input=b'SENHA-FICTICIA\n',
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
    assert result.returncode == 0
    try:
        key = (root.parent / 'master-ficticia').read_bytes()
        attrs = ['service', 'app.donnabot.credentials', 'username', 'COFRE-FICTICIO']
        if phase == 'write':
            # O daemon recém-desbloqueado pode ainda não ter assumido o nome no
            # barramento; sem arquivo .service de ativação, a primeira chamada
            # ao secret-tool corre com a prontidão do serviço.
            result = None
            for _ in range(100):
                result = subprocess.run(['secret-tool', 'store', '--label=Cofre fictício', *attrs],
                                        input=key, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
                if result.returncode == 0:
                    break
                time.sleep(0.1)
            assert result.returncode == 0
        result = subprocess.run(['secret-tool', 'lookup', *attrs], stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=15)
        assert result.returncode == 0 and result.stdout.rstrip(b'\n') == key
    finally:
        subprocess.run(daemon + ['--quit'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)


def run_secret_service(phase, home):
    home.mkdir(exist_ok=True)
    env = {**os.environ, 'HOME': str(home), 'XDG_DATA_HOME': str(home / '.local/share'),
           'XDG_CONFIG_HOME': str(home / '.config'), 'GNOME_KEYRING_CONTROL': str(home / 'control')}
    env.pop('DBUS_SESSION_BUS_ADDRESS', None)
    env.pop('DBUS_STARTER_ADDRESS', None)
    result = subprocess.run(['dbus-run-session', '--', 'python3', str(Path(__file__).resolve()),
                             '--vault-phase', phase, '--root', str(home)], env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
    assert result.returncode == 0, 'fase do Secret Service fictício falhou; saída retida para não expor chave'


def prove_restore_guards(root, cfg, obj, key):
    """Pacotes cifrados/autenticados, mas semanticamente inválidos: GCM não basta."""
    pristine = obj.read_bytes()
    try:
        for label in ('hash', 'integrity', 'counts', 'references', 'files',
                      'mode', 'directories', 'links', 'kind', 'token', 'format'):
            with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
                work = Path(tmp)
                archive = work / 'original.tar.gz'
                b.unseal(obj, archive, root / 'private.pem')
                stage = work / 'stage'
                stage.mkdir()
                meta = b.unpack(archive, stage)
                manifest = stage / 'manifest.json'
                if label == 'hash':
                    target = next(name for name in meta['files'] if name.endswith('exemplo.txt'))
                    (stage / target).write_text('ALTERADO-SEM-ATUALIZAR-HASH')
                elif label == 'integrity':
                    with sqlite3.connect(stage / 'codync.db') as db:
                        db.executescript("CREATE TABLE integrity_fixture(x INTEGER); INSERT INTO integrity_fixture VALUES(7);"
                                         "CREATE INDEX fixture_idx ON integrity_fixture(x); PRAGMA writable_schema=ON;"
                                         "UPDATE sqlite_master SET sql='CREATE INDEX fixture_idx ON integrity_fixture(-x)' "
                                         "WHERE name='fixture_idx'; PRAGMA writable_schema=OFF;")
                    meta['files']['codync.db']['sha256'] = b.digest(stage / 'codync.db')
                elif label == 'counts':
                    meta['counts']['entradas'] += 1
                elif label == 'references':
                    meta['references'][0]['sha256'] = 'f' * 64
                elif label == 'files':
                    (stage / 'arquivo-nao-inventariado.txt').write_text('EXTRA-FICTICIO')
                elif label == 'mode':
                    target = next(name for name in meta['files'] if name.endswith('exemplo.txt'))
                    meta['files'][target]['mode'] ^= 0o100
                elif label == 'directories':
                    (stage / 'pasta-nao-inventariada').mkdir()
                elif label == 'links':
                    meta['links']['link-nao-inventariado'] = 'ausente'
                elif label == 'kind':
                    meta['kind'] = 'vault'
                elif label == 'token':
                    with sqlite3.connect(stage / 'codync.db') as db:
                        db.execute("INSERT INTO kv VALUES('token','TOKEN-ANTIGO-FICTICIO')")
                    meta['files']['codync.db']['sha256'] = b.digest(stage / 'codync.db')
                elif label == 'format':
                    meta['format'] = 99
                manifest.write_text(json.dumps(meta))
                bad_tar = work / 'modified.tar.gz'
                with tarfile.open(bad_tar, 'w:gz', dereference=False) as tar:
                    for path in sorted(stage.rglob('*')):
                        info = tar.gettarinfo(str(path), arcname=str(path.relative_to(stage)))
                        if info.isdir():
                            info.mode = meta['directories'].get(info.name, 0o700)
                        if info.isfile():
                            with path.open('rb') as src:
                                tar.addfile(info, src)
                        else:
                            tar.addfile(info)
                encrypted = work / 'bad.dbkp'
                b.seal(bad_tar, encrypted, cfg['public_key'])
                obj.write_bytes(encrypted.read_bytes())
                destination = root / ('recusar-' + label)
                restore_work = work / 'restore'
                restore_work.mkdir()
                refused(lambda: b.restore(cfg, restore_work, key, root / 'private.pem', destination), label=label)
                assert not destination.exists()
                obj.write_bytes(pristine)
        print('OK A: 11 envelopes válidos com hash/integridade/contagens/referências/arquivos/modo/diretórios/links/tipo/token/formato inválidos recusados')
    finally:
        obj.write_bytes(pristine)


def prove_payload_race(root, cfg, workspace):
    real_copy = b.copy_inventory
    def race(items, stage, *args):
        upload = next(p for name, (p, _) in items.items() if name.endswith('/anexo-c.json'))
        upload.unlink()
        edited = next(p for name, (p, _) in items.items() if name.endswith('/anexo-a.json'))
        edited.write_bytes(b'EDITADO-DURANTE-COPIA-FICTICIA')
        return real_copy(items, stage, *args)
    with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
        stage = Path(tmp)
        with patch.object(b, 'copy_inventory', side_effect=race):
            meta = b.hub_snapshot(cfg, stage)
        assert sum(r['status'] == 'missing' for r in meta['references']) == 3
        assert any(r.get('sha256') == b.hashlib.sha256(b'EDITADO-DURANTE-COPIA-FICTICIA').hexdigest()
                   for r in meta['references'])
        assert any(r['path'].endswith('/anexo-c.json') and r['reason'] == 'disappeared_during_copy'
                   for r in meta['not_read'])
    # Troca de diretório por link durante a cópia: nunca abrir o alvo externo.
    upload_file = next(workspace.glob('uploads/*/anexo-i.json'))
    upload_dir = upload_file.parent
    moved = root / 'upload-original'
    outside = root / 'fora-do-workspace'
    outside.mkdir()
    (outside / 'anexo-i.json').write_text('SENTINELA-NAO-SEGUIR-LINK')
    def swap(items, stage, *args):
        upload_dir.rename(moved)
        upload_dir.symlink_to(outside)
        return real_copy(items, stage, *args)
    try:
        with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
            stage = Path(tmp)
            with patch.object(b, 'copy_inventory', side_effect=swap):
                refused(lambda: b.hub_snapshot(cfg, stage))
            assert all(b'SENTINELA-NAO-SEGUIR-LINK' not in p.read_bytes()
                       for p in stage.rglob('*') if p.is_file() and not p.is_symlink())
    finally:
        if upload_dir.is_symlink():
            upload_dir.unlink()
        if moved.exists():
            moved.rename(upload_dir)
    print('OK G: anexo apagado/editado registrado; troca por symlink recusa snapshot sem ler alvo externo')


def prove_payload_scope(root, cfg, workspace):
    path = workspace / 'projeto/public/uploads' / str(uuid.uuid4()) / 'foto.jpg'
    path.parent.mkdir(parents=True)
    path.write_bytes(b'INICIAL')
    (path.parent / '.env').write_text('EXCLUIDO-FICTICIO')
    real_copy = b.copy_inventory
    def change(items, stage, *args):
        path.write_bytes(b'MUDOU-DURANTE-COPIA')
        return real_copy(items, stage, *args)
    with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
        with patch.object(b, 'copy_inventory', side_effect=change):
            refused(lambda: b.hub_snapshot(cfg, Path(tmp)))
    for unreadable in (path, next(workspace.glob('uploads/*/anexo-i.json'))):
        mode = unreadable.stat().st_mode & 0o777
        unreadable.chmod(0)
        try:
            with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
                refused(lambda: b.hub_snapshot(cfg, Path(tmp)), b.BackupError)
        finally:
            unreadable.chmod(mode)
    bot = workspace.parent.name
    roots = {bot: 'bots/' + bot + '/workspace'}
    upload = str(uuid.uuid4())
    assert b.payload_path('bots/' + bot + '/uploads/' + upload + '/.env', roots)
    assert b.payload_path('bots/' + bot + '/files/' + upload, roots)
    assert not b.payload_path('bots/' + bot + '/files/not-a-uuid', roots)
    assert not b.payload_path('bots/' + bot + '/workspace/projeto/public/uploads/' + upload + '/foto.jpg', roots)
    assert not b.payload_path('external/' + bot + '/uploads/' + upload + '/foto.jpg')
    assert b.payload_path('external/' + bot + '/uploads/' + upload + '/foto.jpg', {bot: 'external/' + bot})
    assert not b.payload_path('bots/' + bot + '/uploads/' + upload.upper() + '/foto.jpg', roots)
    with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
        b.hub_snapshot(cfg, Path(tmp))
        assert not list(Path(tmp).rglob('.env'))
    print('OK G: uploads de subprojeto não escapa à consistência/filtro; EACCES real e comum falha; UUID/localizações estritos')


def fixture_db(path):
    """Esquema mínimo compatível com stats(), kv e o inventário de Spaces."""
    with sqlite3.connect(path) as db:
        db.executescript('''PRAGMA journal_mode=WAL;
            CREATE TABLE bots(id TEXT PRIMARY KEY,config TEXT,deleted INTEGER);
            CREATE TABLE entries(bot_id TEXT,data TEXT);
            CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT);
            CREATE TABLE devices(key TEXT);
            CREATE TABLE push_tickets(ticket TEXT);
            CREATE TABLE activity_tickets(ticket TEXT);
            CREATE TABLE donna_space_conversations(
                id TEXT PRIMARY KEY, space_id TEXT NOT NULL, bot_id TEXT NOT NULL,
                title TEXT NOT NULL, revision INTEGER NOT NULL, is_primary INTEGER NOT NULL,
                closed INTEGER NOT NULL DEFAULT 0, read_revision INTEGER NOT NULL DEFAULT 0);''')


def prove_coverage(root, cfg):
    """Spaces, uploads por conversa, camadas de memória, gerados,
    modo de motor separado e trava do Hub — nos dois layouts."""
    fixture = root / 'cobertura'
    fixture.mkdir()

    def roundtrip(conf, label):
        with b.tempfile.TemporaryDirectory(dir=conf['state_dir']) as tmp:
            result = b.backup(conf, Path(tmp))
        destination = fixture / ('restaurado-' + label)
        with b.tempfile.TemporaryDirectory(dir=conf['state_dir']) as tmp:
            b.restore(conf, Path(tmp), result['object'], root / 'private.pem', destination)
        return destination, json.loads((destination / 'manifest.json').read_text())

    # ---------- modo legado: ~/.mexerica com Spaces, camadas e gerados ----------
    home = fixture / 'hub-legado'
    state = fixture / 'state-legado'
    home.mkdir()
    state.mkdir()
    lcfg = {**cfg, 'hub_home': str(home), 'state_dir': str(state),
            'local_store': str(fixture / 's3-legado'), 'external_workspaces': []}
    bot = str(uuid.uuid4())
    space = str(uuid.uuid4())
    chat = str(uuid.uuid4())
    layer = str(uuid.uuid4())
    group = str(uuid.uuid4())
    personal = str(uuid.uuid4())
    fixture_db(home / 'codync.db')
    workspace = home / 'bots' / bot / 'workspace'
    workspace.mkdir(parents=True)
    (workspace / 'nota.txt').write_text('ESPAÇO-FICTICIO')
    conv_ws = home / 'spaces' / space / 'conversations' / chat / 'workspace'
    conv_ws.mkdir(parents=True)
    (conv_ws / 'arquivo-da-conversa.txt').write_text('CONVERSA-DE-SPACE')
    upload = str(uuid.uuid4())
    published = str(uuid.uuid4())
    (conv_ws / 'uploads' / upload).mkdir(parents=True)
    (conv_ws / 'uploads' / upload / 'certificado.txt').write_bytes(b'ANEXO-SPACE-LEGADO')
    published_path = home / 'bots' / chat / 'files' / published
    published_path.parent.mkdir(parents=True)
    published_path.write_bytes(b'PUBLICADO-DA-CONVERSA')
    (home / 'bots' / layer / 'bot-memory').mkdir(parents=True)
    (home / 'bots' / layer / 'bot-memory' / 'camada.md').write_text('CAMADA-DE-SPACE')
    # Perfis inteiros e nomes sensíveis dentro das próprias raízes novas.
    credential_names = ('client_secret_123.apps.googleusercontent.com.json', 'client_secrets.json',
                        'service_account.json', 'service_account_producao.json',
                        'application_default_credentials.json', 'private_key.json',
                        'secrets.toml', 'credentials.tfrc.json')
    for folder in (conv_ws, home / 'bots' / layer / 'bot-memory'):
        for profile in ('.claude', '.codex', '.grok', '.anthropic', '.gemini', '.opencode'):
            (folder / profile).mkdir()
            (folder / profile / 'arbitrario').write_text('PERFIL-FICTICIO-NAO-COPIAR')
        for name in ('.mcp.json', '.git-credentials', 'id_ed25519', '.pgpass',
                     '.vault-token', 'cert.p12', 'oauth_creds.json', 'Cookies', 'hosts.yml', '.claude.json',
                     'secrets.json', 'client_secret_x.json', 'token.json', 'credentials.json',
                     'Login Data', 'Local State', *credential_names):
            (folder / name).write_text('ARQUIVO-SENSIVEL-FICTICIO')
        (folder / 'normal.txt').write_text('-----BEGIN PRIV' 'ATE KEY-----\nCHAVE-FICTICIA\n-----END PRIVATE KEY-----')
        for profile in ('.cursor', '.continue', '.aider', '.azure', '.gcloud'):
            (folder / profile).mkdir()
            (folder / profile / 'login').write_text('PERFIL-FICTICIO')
    (conv_ws / 'fragmento.txt').write_bytes(b'x' * (b.CHUNK - 10) +
                                             b'-----BEGIN ML-DSA PRIVATE' b' KEY-----\nFICTICIO')
    (conv_ws / 'texto.txt').write_text('BEGIN PRIVATE KEY e apiKey são texto comum')
    (conv_ws / 'uploads' / upload / '.mcp.json').write_text('ANEXO-MCP-FICTICIO')
    (conv_ws / 'uploads' / upload / 'normal.txt').write_text('-----BEGIN PRIV' 'ATE KEY-----\nANEXO-FICTICIO')
    (conv_ws / 'uploads' / upload / '.claude').mkdir()
    (conv_ws / 'uploads' / upload / '.claude' / 'login').write_text('PERFIL-NAO-COPIAR')
    for name in ('secretaria', 'client_secrets_lib'):
        (conv_ws / name).mkdir()
        (conv_ws / name / 'dados.txt').write_text('DADO-LEGITIMO')
    (conv_ws / 'SecretSauce.docx').write_text('DOCUMENTO-LEGITIMO')
    (conv_ws / 'codigo.js').write_text('const apiKey = process.env.API_KEY;\n')
    conjunto = home / 'mexeriquinhas' / 'gerados' / str(uuid.uuid4())
    (conjunto / '256').mkdir(parents=True)
    (conjunto / 'conjunto.json').write_text('{"fixture":"GERADO"}')
    (conjunto / '256' / 'frente.webp').write_bytes(b'WEBP-FICTICIO')
    (home / 'grupos' / group / 'pasta').mkdir(parents=True)
    (home / 'grupos' / group / 'pasta' / 'compartilhado.txt').write_text('PASTA-DE-GRUPO')
    # Fora do pacote: credenciais de motor, sessão WhatsApp e binários.
    (home / 'engine-setup' / 'accounts' / 'conta').mkdir(parents=True)
    (home / 'engine-setup' / 'accounts' / 'conta' / 'anexo-a.json').write_text('CREDENCIAL-FICTICIA')
    (home / 'whatsapp' / bot).mkdir(parents=True)
    (home / 'whatsapp' / bot / 'session.db').write_bytes(b'SESSAO-FICTICIA')
    with sqlite3.connect(home / 'codync.db') as db:
        db.execute('INSERT INTO bots VALUES(?,?,0)', (bot, json.dumps({'cwd': str(workspace)})))
        db.execute('INSERT INTO donna_space_conversations VALUES(?,?,?,?,?,?,0,0)',
                   (bot, personal, bot, 'Principal', 1, 1))
        db.execute('INSERT INTO donna_space_conversations VALUES(?,?,?,?,?,?,0,0)',
                   (chat, space, bot, 'Adicional', 1, 0))
        db.execute('INSERT INTO kv VALUES(?,?)',
                   ('donna.space.memory.{}.{}'.format(space, bot), layer))
        db.execute('INSERT INTO entries VALUES(?,?)', (bot, json.dumps({'text': 'PRINCIPAL'})))
        db.execute('INSERT INTO entries VALUES(?,?)', (chat, json.dumps({
            'attachments': [{'id': upload, 'name': 'certificado.txt',
                             'size': (conv_ws / 'uploads' / upload / 'certificado.txt').stat().st_size,
                             'sha256': b.digest(conv_ws / 'uploads' / upload / 'certificado.txt'),
                             'spaceId': space, 'conversationId': chat, 'revision': 1},
                            {'id': upload, 'name': '.mcp.json', 'size': len('ANEXO-MCP-FICTICIO'),
                             'spaceId': space, 'conversationId': chat}],
            'files': [{'id': published, 'name': 'doc.pdf', 'size': published_path.stat().st_size,
                       'sha256': b.digest(published_path)}]})))
        db.commit()
    restored, meta = roundtrip(lcfg, 'legado')
    esperado = {
        'spaces/{}/conversations/{}/workspace/arquivo-da-conversa.txt'.format(space, chat):
            b'CONVERSA-DE-SPACE',
        'spaces/{}/conversations/{}/workspace/uploads/{}/certificado.txt'.format(space, chat, upload):
            b'ANEXO-SPACE-LEGADO',
        'bots/{}/files/{}'.format(chat, published): b'PUBLICADO-DA-CONVERSA',
        'bots/{}/bot-memory/camada.md'.format(layer): b'CAMADA-DE-SPACE',
        'mexeriquinhas/gerados/{}/conjunto.json'.format(conjunto.name): b'{"fixture":"GERADO"}',
        'grupos/{}/pasta/compartilhado.txt'.format(group): b'PASTA-DE-GRUPO',
    }
    for logical, content in esperado.items():
        assert (restored / logical).read_bytes() == content, logical
    refs = {(r['bot'], r['type'], r['id']): r for r in meta['references']}
    attachment_refs = [r for r in meta['references'] if r['bot'] == chat and r['type'] == 'attachments']
    assert len(attachment_refs) == 2 and all(r['status'] == 'copied' and r['matches_entry']
                                             and r['path'].startswith('spaces/') for r in attachment_refs)
    assert refs[(chat, 'files', published)]['status'] == 'copied'
    base = restored / 'spaces' / space / 'conversations' / chat / 'workspace'
    assert (base / 'uploads' / upload / '.mcp.json').read_text() == 'ANEXO-MCP-FICTICIO'
    assert (base / 'uploads' / upload / 'normal.txt').read_text() == '-----BEGIN PRIV' 'ATE KEY-----\nANEXO-FICTICIO'
    assert not (base / 'uploads' / upload / '.claude').exists()
    for name in ('secretaria', 'client_secrets_lib'):
        assert (base / name / 'dados.txt').read_text() == 'DADO-LEGITIMO'
    assert (base / 'SecretSauce.docx').read_text() == 'DOCUMENTO-LEGITIMO'
    assert (base / 'codigo.js').read_text() == 'const apiKey = process.env.API_KEY;\n'
    assert (base / 'texto.txt').is_file()
    for folder in (base, restored / 'bots' / layer / 'bot-memory'):
        for name in ('secrets.json', 'client_secret_x.json', 'token.json', 'credentials.json',
                     'Login Data', 'Local State', 'normal.txt', '.cursor', '.continue', '.aider', '.azure', '.gcloud',
                     *credential_names):
            assert not (folder / name).exists(), name
        for name in credential_names:
            logical = (folder / name).relative_to(restored).as_posix()
            assert any(r['reason'] == 'excluded_name' and r['path'] == logical for r in meta['not_read']), logical
    assert not (base / 'fragmento.txt').exists()
    assert any(r['reason'] == 'excluded_private_key' and r['path'].endswith('/fragmento.txt')
               for r in meta['not_read'])
    packed = {name for name in meta['files']}
    assert not any(Path(name).name in ('.mcp.json', '.git-credentials', 'id_ed25519',
                   '.pgpass', '.vault-token', 'cert.p12', 'oauth_creds.json', 'Cookies',
                   'hosts.yml', '.claude.json') and not b.payload_path(name) for name in packed)
    assert not any(part in ('.claude', '.codex', '.grok', '.anthropic', '.gemini', '.opencode')
                   for name in packed for part in Path(name).parts)
    assert any(r['reason'] == 'excluded_name' for r in meta['not_read'])
    assert not any(name.startswith(('engine-setup/', 'whatsapp/', 'motor/')) for name in packed)
    assert b'SESSAO-FICTICIA' not in b''.join((restored / n).read_bytes() for n in packed
                                            if (restored / n).is_file())
    print('OK legado: Spaces, uploads de conversa, arquivos publicados, camada de memória kv, gerados e grupos')

    # Nem hub_home pode ser configurado como uma raiz de perfil conhecida.
    for forbidden_home in ('/var/lib/donnabot/motor', '/var/lib/donnabot/motor/conta',
                           '/tmp/.claude', '/tmp/engine-setup/accounts'):
        refused(lambda: b.validate_hub_home(Path(forbidden_home)))

    # ---------- modo de motor separado: /var/lib/donnabot/trabalho ----------
    home2 = fixture / 'hub-motor'
    work = fixture / 'trabalho'
    state2 = fixture / 'state-motor'
    for path in (home2, work / 'bots', work / 'spaces', state2):
        path.mkdir(parents=True)
    outside = fixture / 'credentials-service'
    outside.mkdir()
    fixture_db(home2 / 'codync.db')
    bot2 = str(uuid.uuid4())
    bot3 = str(uuid.uuid4())
    bot4 = str(uuid.uuid4())
    own = work / 'bots' / bot3 / 'workspace'
    own.mkdir(parents=True)
    space2 = str(uuid.uuid4())
    chat2 = str(uuid.uuid4())
    (work / 'bots' / bot2 / 'workspace').mkdir(parents=True)
    (work / 'bots' / bot2 / 'workspace' / 'motor.txt').write_text('WORKSPACE-MOTOR')
    (work / 'bots' / bot2 / 'workspace' / '.codex').mkdir()
    (work / 'bots' / bot2 / 'workspace' / '.codex' / 'qualquer').write_text('PERFIL-MOTOR')
    (work / 'bots' / bot2 / 'workspace' / 'hosts.yml').write_text('HOSTS-FICTICIO')
    (work / 'bots' / bot2 / 'workspace' / 'nota.txt').write_text('TOKEN-GITHUB-FICTICIO')
    up2 = str(uuid.uuid4())
    (work / 'bots' / bot2 / 'uploads' / up2).mkdir(parents=True)
    (work / 'bots' / bot2 / 'uploads' / up2 / 'anexo-t.txt').write_bytes(b'ANEXO-MOTOR')
    (work / 'spaces' / space2 / 'conversations' / chat2 / 'workspace').mkdir(parents=True)
    (work / 'spaces' / space2 / 'conversations' / chat2 / 'workspace' / 'conv.txt').write_text('CONV-MOTOR')
    (work / 'spaces' / space2 / 'conversations' / chat2 / 'workspace' / '.mcp.json').write_text('MCP-FICTICIO')
    up3 = str(uuid.uuid4())
    (work / 'spaces' / space2 / 'conversations' / chat2 / 'uploads' / up3).mkdir(parents=True)
    (work / 'spaces' / space2 / 'conversations' / chat2 / 'uploads' / up3 / 'doc.pdf').write_bytes(b'ANEXO-MOTOR-SPACE')
    (own / 'dado.txt').write_text('PASTA-PROPRIA-MOTOR')
    (outside / 'fora.txt').write_text('EXTERNO-FICTICIO')
    (work / 'skills' / 'habilidade').mkdir(parents=True)
    (work / 'skills' / 'habilidade' / 'SKILL.md').write_text('SKILL-MOTOR')
    # Efêmeros do modo separado ficam fora, como no legado.
    (work / 'background' / 'job').mkdir(parents=True)
    (work / 'background' / 'job' / 'scratch.txt').write_text('EFEMERO')
    published2 = str(uuid.uuid4())
    published2_path = home2 / 'bots' / chat2 / 'files' / published2
    published2_path.parent.mkdir(parents=True)
    published2_path.write_bytes(b'PUBLICADO-MOTOR')
    # Credenciais de motor ficam fora de trabalho/ e nunca entram no pacote.
    motor = fixture / 'motor'
    (motor / 'engine-setup' / 'accounts').mkdir(parents=True)
    (motor / 'engine-setup' / 'accounts' / 'segredo').write_text('CREDENCIAL-MOTOR')
    with sqlite3.connect(home2 / 'codync.db') as db:
        db.execute('INSERT INTO bots VALUES(?,?,0)',
                   (bot2, json.dumps({'cwd': str(work / 'bots' / bot2 / 'workspace')})))
        db.execute('INSERT INTO bots VALUES(?,?,0)',
                   (bot3, json.dumps({'cwd': str(own)})))
        db.execute('INSERT INTO bots VALUES(?,?,0)',
                   (bot4, json.dumps({'cwd': str(outside)})))
        db.execute('INSERT INTO donna_space_conversations VALUES(?,?,?,?,?,?,0,0)',
                   (chat2, space2, bot2, 'Adicional', 1, 0))
        db.execute('INSERT INTO entries VALUES(?,?)', (bot2, json.dumps({
            'attachments': [{'id': up2, 'name': 'anexo-t.txt', 'size': 12, 'sha256': '0' * 64}]})))
        db.execute('INSERT INTO entries VALUES(?,?)', (chat2, json.dumps({
            'attachments': [{'id': up3, 'name': 'doc.pdf',
                             'size': (work / 'spaces' / space2 / 'conversations' / chat2 / 'uploads' / up3 / 'doc.pdf').stat().st_size,
                             'sha256': b.digest(work / 'spaces' / space2 / 'conversations' / chat2 / 'uploads' / up3 / 'doc.pdf'),
                             'spaceId': space2, 'conversationId': chat2, 'revision': 1}],
            'files': [{'id': published2, 'name': 'doc.pdf', 'size': published2_path.stat().st_size,
                       'sha256': b.digest(published2_path)}]})))
        db.commit()
    mcfg = {**lcfg, 'hub_home': str(home2), 'state_dir': str(state2),
            'local_store': str(fixture / 's3-motor'), 'motor_work': str(work),
            'external_workspaces': []}
    # cwd fora de trabalho/ continua exigindo lista explícita (fail-closed).
    try:
        with b.tempfile.TemporaryDirectory(dir=state2) as tmp:
            b.hub_snapshot(mcfg, Path(tmp))
    except b.BackupError as error:
        assert str(error) == 'workspace_externo_exige_lista', error
    else:
        raise AssertionError('cwd externo sem lista deveria falhar')
    mcfg['external_workspaces'] = [str(outside)]
    # background e qualquer outro diretório arbitrário em motor_work são recusados.
    with sqlite3.connect(home2 / 'codync.db') as db:
        db.execute('UPDATE bots SET config=? WHERE id=?', (json.dumps({'cwd': str(work / 'background' / 'job')}), bot3))
    with b.tempfile.TemporaryDirectory(dir=state2) as tmp:
        try:
            b.hub_snapshot(mcfg, Path(tmp))
        except b.BackupError as error:
            assert str(error) == 'motor_work_workspace_recusado', error
        else:
            raise AssertionError('cwd em background deveria ser recusado')
    # Apontar uma raiz externa diretamente para um perfil também falha.
    profile = fixture / '.codex'
    profile.mkdir()
    with sqlite3.connect(home2 / 'codync.db') as db:
        db.execute('UPDATE bots SET config=? WHERE id=?', (json.dumps({'cwd': str(profile)}), bot3))
    with b.tempfile.TemporaryDirectory(dir=state2) as tmp:
        try:
            b.hub_snapshot({**mcfg, 'external_workspaces': [str(outside), str(profile)]}, Path(tmp))
        except b.BackupError as error:
            assert str(error) == 'raiz_perfil_motor_recusada', error
        else:
            raise AssertionError('perfil explícito deveria ser recusado')
    with sqlite3.connect(home2 / 'codync.db') as db:
        db.execute('UPDATE bots SET config=? WHERE id=?', (json.dumps({'cwd': str(own)}), bot3))
    restored2, meta2 = roundtrip(mcfg, 'motor')
    esperado2 = {
        'work/bots/{}/workspace/motor.txt'.format(bot2): b'WORKSPACE-MOTOR',
        'work/bots/{}/uploads/{}/anexo-t.txt'.format(bot2, up2): b'ANEXO-MOTOR',
        'work/spaces/{}/conversations/{}/workspace/conv.txt'.format(space2, chat2): b'CONV-MOTOR',
        'work/spaces/{}/conversations/{}/uploads/{}/doc.pdf'.format(space2, chat2, up3):
            b'ANEXO-MOTOR-SPACE',
        'work/bots/{}/workspace/dado.txt'.format(bot3): b'PASTA-PROPRIA-MOTOR',
        'work/skills/habilidade/SKILL.md': b'SKILL-MOTOR',
        'external/{}/fora.txt'.format(bot4): b'EXTERNO-FICTICIO',
        'bots/{}/files/{}'.format(chat2, published2): b'PUBLICADO-MOTOR',
    }
    for logical, content in esperado2.items():
        assert (restored2 / logical).read_bytes() == content, logical
    refs2 = {(r['bot'], r['type'], r['id']): r for r in meta2['references']}
    assert refs2[(bot2, 'attachments', up2)]['status'] == 'copied'
    assert refs2[(bot2, 'attachments', up2)]['path'].startswith('work/bots/')
    assert refs2[(chat2, 'attachments', up3)]['status'] == 'copied'
    assert refs2[(chat2, 'attachments', up3)]['path'].startswith('work/spaces/')
    assert refs2[(chat2, 'files', published2)]['status'] == 'copied'
    packed2 = set(meta2['files'])
    assert not any(name.endswith(('/.codex/qualquer', '/hosts.yml', '/.mcp.json')) for name in packed2)
    assert not any(name.startswith(('motor/', 'engine-setup/', 'work/background/'))
                   for name in packed2)
    assert (restored2 / 'work' / 'bots' / bot2 / 'workspace' / 'nota.txt').is_file()
    # Raiz externa de nome realmente reservado é omitida com aviso visível.
    excluded_root = fixture / 'credentials'
    excluded_root.mkdir()
    (excluded_root / 'dado.txt').write_text('NAO-COPIAR')
    with sqlite3.connect(home2 / 'codync.db') as db:
        db.execute('UPDATE bots SET config=? WHERE id=?', (json.dumps({'cwd': str(excluded_root)}), bot4))
    warning = io.StringIO()
    with b.tempfile.TemporaryDirectory(dir=state2) as tmp, contextlib.redirect_stderr(warning):
        meta_root = b.hub_snapshot({**mcfg, 'external_workspaces': [str(excluded_root)]}, Path(tmp))
        assert not (Path(tmp) / 'external' / bot4).exists()
    assert any(r['reason'] == 'excluded_root_name' for r in meta_root['not_read'])
    notices = [json.loads(line) for line in warning.getvalue().splitlines()]
    assert notices == [{'warning': 'itens_excluidos_do_backup', 'count': sum(
        item['reason'].startswith('excluded_') for item in meta_root['not_read'])}]
    with sqlite3.connect(home2 / 'codync.db') as db:
        db.execute('UPDATE bots SET config=? WHERE id=?', (json.dumps({'cwd': str(outside)}), bot4))
    # motor_work ausente ou sobreposto ao hub_home falha com código claro.
    for bad, code in ((str(fixture / 'trabalho-inexistente'), 'motor_work_ausente'),
                      (str(home2 / 'trabalho'), 'motor_work_sobrepoe_hub')):
        try:
            with b.tempfile.TemporaryDirectory(dir=state2) as tmp:
                b.hub_snapshot({**mcfg, 'motor_work': bad}, Path(tmp))
        except b.BackupError as error:
            assert str(error) == code, (bad, error)
        else:
            raise AssertionError('motor_work inválido deveria falhar: ' + bad)
    invalid_layout = fixture / 'trabalho-layout-invalido'
    invalid_layout.mkdir()
    (invalid_layout / 'bots').write_text('não é diretório')
    with b.tempfile.TemporaryDirectory(dir=state2) as tmp:
        try:
            b.hub_snapshot({**mcfg, 'motor_work': str(invalid_layout)}, Path(tmp))
        except b.BackupError as error:
            assert str(error) == 'motor_work_layout_invalido', error
        else:
            raise AssertionError('layout inválido deve ser recusado')
    print('OK motor: bots/spaces com layout validado, uploads siblings, '
          'externo fail-closed, motor/ fora do pacote')

    # ---------- trava do Hub: manutenção aborta, serve e livre seguem ----------
    home3 = fixture / 'hub-lock'
    state3 = fixture / 'state-lock'
    home3.mkdir()
    state3.mkdir()
    lcfg3 = {**lcfg, 'hub_home': str(home3), 'state_dir': str(state3),
             'local_store': str(fixture / 's3-lock')}
    fixture_db(home3 / 'codync.db')
    lock = home3 / 'host.lock'
    lock.write_text('')
    # Livre: nenhum detentor.
    with b.tempfile.TemporaryDirectory(dir=state3) as tmp:
        b.hub_snapshot(lcfg3, Path(tmp))
    # Detentor genérico (manutenção): aborta com status claro.
    holder = fixture / 'lock-holder.py'
    holder.write_text(
        'import fcntl, os, sys, time\n'
        'fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT, 0o600)\n'
        'fcntl.flock(fd, fcntl.LOCK_EX)\n'
        'os.set_inheritable(fd, True)\n'
        'if len(sys.argv) > 2 and sys.argv[2] == "exec":\n'
        '    os.execv(sys.executable, ["python3", sys.argv[3], sys.argv[4]])\n'
        'time.sleep(60)\n')
    children = []
    def spawn(mode):
        args = [sys.executable, str(holder), str(lock)]
        if mode == 'exec':
            daemon = fixture / 'donnabot-host'
            daemon.write_text('import time\ntime.sleep(60)\n')
            args += ['exec', str(daemon), 'serve']
        proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        children.append(proc)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if b.host_lock_holders(lock):
                return
            time.sleep(0.05)
        raise AssertionError('detentor da trava não apareceu')
    try:
        spawn('live')
        real_read = Path.read_text
        def inaccessible_proc(path, *args, **kwargs):
            if path == Path('/proc/locks'):
                raise PermissionError('fixture')
            return real_read(path, *args, **kwargs)
        with patch.object(Path, 'read_text', inaccessible_proc):
            try:
                b.host_lock_holders(lock)
            except b.BackupError as error:
                assert str(error) == 'host_lock_verificacao_indisponivel'
            else:
                raise AssertionError('/proc/locks ilegível deve falhar fechado')
        with b.tempfile.TemporaryDirectory(dir=state3) as tmp:
            try:
                b.hub_snapshot(lcfg3, Path(tmp))
            except b.BackupError as error:
                assert str(error) == 'hub_em_manutencao', error
            else:
                raise AssertionError('manutenção segurando host.lock deveria abortar')
        children[-1].kill()
        children[-1].wait()
        children.clear()
        # Servidor do Hub (argv contém donnabot-host ... serve): backup segue.
        spawn('exec')
        with b.tempfile.TemporaryDirectory(dir=state3) as tmp:
            b.hub_snapshot(lcfg3, Path(tmp))
    finally:
        for proc in children:
            proc.kill()
            proc.wait()
    # Substituição do banco durante cópia de arquivos impede publicar o snapshot.
    real_copy = b.copy_inventory
    def swap_database(items, stage, *args):
        original = home3 / 'codync.db'
        moved = home3 / 'banco-original'
        original.rename(moved)
        shutil.copyfile(moved, original)
        return real_copy(items, stage, *args)
    with b.tempfile.TemporaryDirectory(dir=state3) as tmp:
        with patch.object(b, 'copy_inventory', side_effect=swap_database):
            try:
                b.hub_snapshot(lcfg3, Path(tmp))
            except b.BackupError as error:
                assert str(error) == 'banco_substituido_durante_backup', error
            else:
                raise AssertionError('troca do banco deve abortar')
    assert b.hub_lock_allows_backup([['/x/donnabot-host', 'serve', '--port', '1']])
    assert not b.hub_lock_allows_backup([['/x/donnabot-host', 'migrar-motor']])
    assert not b.hub_lock_allows_backup([['python3', '-c', 'x']])
    assert b.hub_lock_allows_backup([]) and b.hub_lock_allows_backup(None)
    print('OK trava: manutenção aborta com hub_em_manutencao; serve e trava livre seguem')


def prove_payload_history(root, cfg):
    """Mesmo fluxo do revisor: cwd gerenciado -> pasta do dono -> bot apagado."""
    fixture = root / 'payload-history'
    home = fixture / 'hub-ficticio'
    state = fixture / 'state'
    owner = fixture / 'pasta-do-dono'
    for path in (home, state, owner):
        path.mkdir(parents=True)
    cfg = {**cfg, 'hub_home': str(home), 'state_dir': str(state),
           'local_store': str(fixture / 's3-simulado'), 'external_workspaces': [str(owner)]}
    bot = str(uuid.uuid4())
    managed = home / 'bots' / bot / 'workspace'
    managed.mkdir(parents=True)
    names = ('credentials.json', 'apresentacao.key', 'certificado.pem', 'normal.txt')
    attachments = []
    expected = {}
    for name in names:
        upload = str(uuid.uuid4())
        relative = 'bots/' + bot + '/workspace/uploads/' + upload + '/' + name
        path = home / relative
        path.parent.mkdir(parents=True)
        path.write_bytes(('ANEXO-FICTICIO-' + name).encode())
        expected[relative] = path.read_bytes()
        attachments.append({'id': upload, 'name': name, 'size': path.stat().st_size, 'sha256': b.digest(path)})

    def roundtrip(label):
        with b.tempfile.TemporaryDirectory(dir=state) as tmp:
            result = b.backup(cfg, Path(tmp))
        destination = fixture / ('restaurado-' + label)
        with b.tempfile.TemporaryDirectory(dir=state) as tmp:
            b.restore(cfg, Path(tmp), result['object'], root / 'private.pem', destination)
        return destination, json.loads((destination / 'manifest.json').read_text())

    failures = []
    with sqlite3.connect(home / 'codync.db') as db:
        db.executescript('''PRAGMA journal_mode=WAL;
            CREATE TABLE bots(id TEXT PRIMARY KEY,config TEXT,deleted INTEGER);
            CREATE TABLE entries(bot_id TEXT,data TEXT);
            CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT);
            CREATE TABLE devices(key TEXT);
            CREATE TABLE push_tickets(ticket TEXT);
            CREATE TABLE activity_tickets(ticket TEXT);''')
        db.execute('INSERT INTO bots VALUES(?,?,0)', (bot, json.dumps({'cwd': str(managed)})))
        db.execute('INSERT INTO entries VALUES(?,?)', (bot, json.dumps({'attachments': attachments})))
        db.commit()
        for label in ('workspace-atual', 'workspace-anterior', 'bot-apagado'):
            if label == 'workspace-anterior':
                db.execute('UPDATE bots SET config=? WHERE id=?', (json.dumps({'cwd': str(owner)}), bot))
            elif label == 'bot-apagado':
                db.execute('UPDATE bots SET deleted=1 WHERE id=?', (bot,))
                db.execute('DELETE FROM entries WHERE bot_id=?', (bot,))
            db.commit()
            destination, meta = roundtrip(label)
            recovered = [Path(relative).name for relative, content in expected.items()
                         if (destination / relative).is_file() and (destination / relative).read_bytes() == content]
            copied = sum(r['status'] == 'copied' and r.get('matches_entry') is True for r in meta['references'])
            print('R3 ' + label + ': ' + json.dumps({'arquivos': recovered, 'referencias_copiadas': copied,
                  'referencias_ausentes': sum(r['status'] == 'missing' for r in meta['references']),
                  'not_read': meta['not_read'], 'counts': meta['counts']}, ensure_ascii=False), flush=True)
            if recovered != list(names) or copied != (0 if label == 'bot-apagado' else 4):
                failures.append(label)
            assert meta['counts'] == {'bots': 0 if label == 'bot-apagado' else 1,
                                      'conversas': 0 if label == 'bot-apagado' else 1,
                                      'entradas': 0 if label == 'bot-apagado' else 1}
            if label == 'bot-apagado':
                assert meta['references'] == []

        # Grupo e pasta própria do dono usam bots/<id>/uploads; publicados não dependem do cwd.
        for kind in ('grupo', 'pasta-propria'):
            current = str(uuid.uuid4())
            config = {'cwd': '' if kind == 'grupo' else str(owner)}
            db.execute('INSERT INTO bots VALUES(?,?,0)', (current, json.dumps(config)))
            items = []
            for name in names:
                upload = str(uuid.uuid4())
                relative = 'bots/' + current + '/uploads/' + upload + '/' + name
                path = home / relative
                path.parent.mkdir(parents=True)
                path.write_bytes(('ANEXO-FICTICIO-' + kind + '-' + name).encode())
                expected[relative] = path.read_bytes()
                items.append({'id': upload, 'name': name, 'size': path.stat().st_size, 'sha256': b.digest(path)})
            items.append({'id': str(uuid.uuid4()), 'name': 'credentials.json', 'size': 9})
            published = str(uuid.uuid4())
            relative = 'bots/' + current + '/files/' + published
            path = home / relative
            path.parent.mkdir()
            path.write_bytes(b'PUBLICADO-FICTICIO')
            expected[relative] = path.read_bytes()
            db.execute('INSERT INTO entries VALUES(?,?)', (current, json.dumps({'attachments': items, 'files': [
                {'id': published, 'name': 'certificado.pem', 'size': path.stat().st_size, 'sha256': b.digest(path)},
                {'id': str(uuid.uuid4()), 'name': 'apresentacao.key', 'size': 9}]})))
        db.commit()
        destination, meta = roundtrip('grupo-pasta-propria')
        assert all((destination / relative).read_bytes() == content for relative, content in expected.items()
                   if not relative.startswith('bots/' + bot + '/'))
        assert sum(r['status'] == 'copied' and r.get('matches_entry') is True for r in meta['references']) == 10
        assert sum(r['status'] == 'missing' for r in meta['references']) == 4
        assert {r['type'] for r in meta['references'] if r['status'] == 'missing'} == {'files', 'attachments'}
    assert not failures, 'anexos_historicos: ' + ', '.join(failures)
    print('OK R3: quatro anexos preservados após trocar cwd e apagar bot; grupo/pasta própria/publicados e ausências no manifesto')


def prove_control_paths(root, cfg, cli, key):
    alias = root / 'alias'
    alias.symlink_to(root, target_is_directory=True)
    path = root / 'config-links.json'
    path.write_text(json.dumps({**cfg, 'state_dir': str(alias / 'state'), 'public_key': str(alias / 'public.pem'),
                               'local_store': str(alias / 's3-simulado')}))
    run = subprocess.run(cli + ['restore', '--config', str(alias / path.name), '--object', key,
                         '--private-key', str(alias / 'private.pem'), '--destination', str(alias / 'restaurado-alias')],
                         capture_output=True, text=True)
    assert run.returncode == 0, run.stderr
    link = root / 'config-link-terminal.json'
    link.symlink_to(path)
    run = subprocess.run(cli + ['check-health', '--config', str(link)], capture_output=True, text=True)
    assert run.returncode == 1 and json.loads(run.stderr)['error'] == 'config_link_recusado'
    invalid = {**cfg, 'state_dir': str(root / 'public.pem' / 'state')}
    path.write_text(json.dumps(invalid))
    run = subprocess.run(cli + ['check-health', '--config', str(path)], capture_output=True, text=True)
    assert run.returncode == 1 and json.loads(run.stderr)['error'] == 'state_dir_criacao_falhou_errno_20'
    alias.unlink()
    print('OK C: config/chave/state/pública por pasta-link restauram; link terminal e pai não diretório têm erro claro')


def prove_crossdisk(root, cfg, key):
    # Linux: tmpfs separado, pasta exclusiva desta prova, removida mesmo em falha.
    with b.tempfile.TemporaryDirectory(prefix='donnabot-pr55-r2-', dir='/dev/shm') as tmp:
        parent = Path(tmp)
        assert parent.stat().st_dev != Path(cfg['state_dir']).stat().st_dev, 'prova D exige dois filesystems'
        with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as work:
            destination = parent / 'restaurado'
            b.restore(cfg, Path(work), key, root / 'private.pem', destination)
            readonly = next(destination.glob('bots/*/workspace/somente-leitura'))
            assert readonly.stat().st_mode & 0o777 == 0o555
            assert (readonly / 'modulo.txt').read_text() == 'FICTICIO-0555'
            b.remove_private_tree(destination)
        real_chmod = Path.chmod
        failed = False
        def fail_after_modes(path, mode, *args, **kwargs):
            nonlocal failed
            if path == parent / 'falha/bots' and not failed:
                failed = True
                raise PermissionError('falha fictícia após aplicar 0555')
            return real_chmod(path, mode, *args, **kwargs)
        with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as work:
            with patch.object(Path, 'chmod', fail_after_modes):
                try:
                    b.restore(cfg, Path(work), key, root / 'private.pem', parent / 'falha')
                except b.BackupError as error:
                    assert str(error) == 'promocao_falhou_destino_removido'
                else:
                    raise AssertionError('falha de promoção não detectada')
            assert failed and not (parent / 'falha').exists()
    print('OK D: restauração entre filesystems com pasta 0555; falha após modos deixa destino ausente')


def prove_retry(root, cfg):
    transfer = b.Storage.transfer
    for scenario in ('get', 'put_uncertain', 'status_full'):
        store = root / ('retry-' + scenario)
        retry_cfg = {**cfg, 'local_store': str(store)}
        puts = []
        durable = b.durable_json
        def fail_status(path, value):
            if scenario == 'status_full' and path.name == 'backup.json':
                raise OSError(28, 'disco fictício cheio ao registrar sucesso')
            durable(path, value)
        def fail(self, key, path, upload=False):
            if upload:
                puts.append(key)
                transfer(self, key, path, upload=True)
                if scenario == 'put_uncertain':
                    raise b.BackupError('r2_falhou')
                return
            if scenario != 'status_full':
                raise b.BackupError('r2_falhou')
            return transfer(self, key, path)
        with patch.object(b.Storage, 'transfer', fail), patch.object(b, 'durable_json', fail_status):
            for _ in range(3):
                with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
                    refused(lambda: b.backup(retry_cfg, Path(tmp)), OSError if scenario == 'status_full' else b.BackupError)
        assert len(puts) == 1 and len(list((store / 'backups').iterdir())) == 1
        pending = Path(cfg['state_dir']) / 'hub-pending'
        record = json.loads((pending / 'point.json').read_text())
        assert record['put_started'] and record['object'] == puts[0]
        if scenario == 'get':
            old = {k: v for k, v in record.items() if k != 'credential_policy'}
            old['put_started'] = False
            b.durable_json(pending / 'point.json', old)
            with patch.object(b.Storage, 'transfer') as network:
                with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
                    try:
                        b.backup(retry_cfg, Path(tmp))
                    except b.BackupError as error:
                        assert str(error) == 'pendente_hub_politica_antiga'
                    else:
                        raise AssertionError('pendência antiga deve bloquear envio')
                network.assert_not_called()
            assert pending.exists()
            b.durable_json(pending / 'point.json', record)
        # Conferir ponto velho não o rejuvenesce no alerta de 26 h.
        record['point_at'] = (b.dt.datetime.now(b.dt.timezone.utc) - b.dt.timedelta(days=3)).isoformat()
        b.durable_json(pending / 'point.json', record)
        with b.tempfile.TemporaryDirectory(dir=cfg['state_dir']) as tmp:
            result = b.backup(retry_cfg, Path(tmp))
        assert result['object'] == puts[0] and result['point_at'] == record['point_at'] and not pending.exists()
        (Path(cfg['state_dir']) / 'backup.json').write_text(json.dumps(
            {'ok': True, 'at': b.dt.datetime.now(b.dt.timezone.utc).isoformat(), **result}))
        try:
            b.check_health(cfg)
        except b.BackupError as error:
            assert 'backup_atrasado_26h' in error.alerts
        else:
            raise AssertionError('ponto velho apresentado como novo')
    print('OK E: 3 tentativas após GET falho/PUT incerto/disco cheio no status criam só 1 objeto; só GET, ponto rastreado e idade preservada')


def prove_retention(root, cfg):
    path = root / 'retention.json'
    r2 = {**cfg, 'transport': 'r2', 'account_id': '0' * 32, 'bucket': 'fixture-backups',
          'credentials': str(root / 'r2.json')}
    for retention in ({'retention': 'bucket-lock-indefinite'},
                      {'retention': 'bucket-lock-age', 'lock_days': 7, 'expiration_days': 7},
                      {'retention': 'bucket-lock-age', 'lock_days': 35, 'expiration_days': 90}):
        path.write_text(json.dumps({**r2, **retention}))
        assert b.config(path)['retention'] == retention['retention']
    for invalid in ({}, {'retention': 'ESCOLHER_RETENCAO'},
                    {'retention': 'bucket-lock-age', 'lock_days': 6, 'expiration_days': 7},
                    {'retention': 'bucket-lock-age', 'lock_days': 7, 'expiration_days': 6},
                    {'retention': 'bucket-lock-age', 'lock_days': '7', 'expiration_days': 7},
                    {'retention': 'bucket-lock-age', 'lock_days': 7, 'expiration_days': '7'},
                    {'retention': 'bucket-lock-age', 'lock_days': 35, 'expiration_days': 30}):
        path.write_text(json.dumps({**r2, **invalid}))
        refused(lambda: b.config(path))
    for jurisdiction in ('default', 'eu', 'us'):
        with patch.object(b.subprocess, 'run') as run:
            run.return_value.returncode = 0
            run.return_value.stdout = b''
            b.curl({**r2, 'jurisdiction': jurisdiction}, 'GET', 'backups/hub-20200101T000000Z-' + '0' * 32 + '.dbkp')
            suffix = '' if jurisdiction == 'default' else '.' + jurisdiction
            assert 'https://' + '0' * 32 + suffix + '.r2.cloudflarestorage.com/' in run.call_args.kwargs['input'].decode()
    print('OK B: 7/7, indefinida e 35/90 aceitas; escolha ausente, tipos e prazos inválidos recusados; endpoints sem rede')


def prove_status_and_interrupt(root, cfg, cli, key, obj, pristine):
    obj.write_bytes(pristine)
    config = str(root / 'config.json')
    state = Path(cfg['state_dir'])
    run = subprocess.run(cli + ['restore', '--config', config, '--object', key,
                         '--private-key', str(root / 'private.pem'), '--destination', str(root / 'restore-cli')],
                         capture_output=True, text=True)
    assert run.returncode == 0, run.stderr
    status = json.loads((state / 'restore.json').read_text())
    assert status['object'] == key and status['created_at'] and status['bytes'] > 0
    # Processo real bloqueado em transporte simulado; o grupo inteiro recebe SIGTERM.
    # O curl fictício não abre sockets nem usa credenciais reais.
    stub = root / 'transport-stub'
    stub.mkdir()
    ready = stub / 'ready'
    (stub / 'curl').write_text('#!/usr/bin/python3\nimport os,sys,time\n'
                              'sys.stdin.buffer.read()\n'
                              'open(os.environ["FIXTURE_READY"],"w").close()\n'
                              'time.sleep(60)\n')
    (stub / 'curl').chmod(0o700)
    config_path = root / 'interrupt-config.json'
    config_path.write_text(json.dumps({**cfg, 'transport': 'r2', 'account_id': '0' * 32,
                                     'bucket': 'fixture-backups', 'credentials': str(root / 'r2.json'),
                                     'retention': 'bucket-lock-indefinite'}))
    env = {**os.environ, 'PATH': str(stub) + ':' + os.environ['PATH'], 'FIXTURE_READY': str(ready)}
    for _ in range(2):
        ready.unlink(missing_ok=True)
        proc = subprocess.Popen(cli + ['restore', '--config', str(config_path), '--object', key,
                                '--private-key', str(root / 'private.pem'), '--destination', str(root / 'interrompido')],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env, start_new_session=True)
        try:
            deadline = time.monotonic() + 10
            while not ready.exists() and time.monotonic() < deadline:
                assert proc.poll() is None
                time.sleep(0.02)
            assert ready.exists()
            os.killpg(proc.pid, signal.SIGTERM)
            stdout, stderr = proc.communicate(timeout=10)
            assert proc.returncode == 1 and json.loads(stderr)['error'] == 'interrompido_SIGTERM'
            assert json.loads((state / 'restore-failure.json').read_text())['error'] == 'interrompido_SIGTERM'
            assert not list(state.glob('job-*')) and not (root / 'interrompido').exists()
            assert json.loads((state / 'restore.json').read_text()) == status
        finally:
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
    # Idade dos dois sucessos e última tentativa; falha recente não pode aparecer ok.
    now = b.dt.datetime.now(b.dt.timezone.utc)
    for operation in ('backup', 'vault-backup'):
        (state / (operation + '.json')).write_text(json.dumps({'ok': True, 'at': now.isoformat()}))
        (state / (operation + '-failure.json')).unlink(missing_ok=True)
    assert b.check_health(cfg)['status'] == 'em_dia'
    (state / 'backup.json').write_text(json.dumps({'ok': True, 'at': (now - b.dt.timedelta(hours=27)).isoformat()}))
    run = subprocess.run(cli + ['check-health', '--config', config], capture_output=True, text=True)
    assert run.returncode == 1 and json.loads(run.stderr)['error'] == 'backup_atrasado_26h'
    (state / 'backup.json').write_text(json.dumps({'ok': True, 'at': now.isoformat()}))
    (state / 'backup-failure.json').write_text(json.dumps({'at': (now + b.dt.timedelta(seconds=1)).isoformat()}))
    refused(lambda: b.check_health(cfg))
    expiry_cfg = {**cfg, 'credential_expires_at': (now + b.dt.timedelta(days=10)).isoformat()}
    (state / 'backup-failure.json').unlink()
    refused(lambda: b.check_health(expiry_cfg))
    # Data curta/sem fuso é UTC; erro de validade não esconde atraso/falha.
    assert b.check_health({**cfg, 'credential_expires_at': (now + b.dt.timedelta(days=90)).date().isoformat()})['status'] == 'em_dia'
    refused(lambda: b.check_health({**cfg, 'credential_expires_at': (now + b.dt.timedelta(days=10)).date().isoformat()}))
    (state / 'backup.json').write_text(json.dumps({'ok': True, 'at': (now - b.dt.timedelta(hours=27)).isoformat()}))
    (state / 'vault-backup-failure.json').write_text(json.dumps({'at': (now + b.dt.timedelta(seconds=1)).isoformat()}))
    try:
        b.check_health({**cfg, 'credential_expires_at': 'data-invalida'})
    except b.BackupError as error:
        assert set(error.alerts) == {'backup_atrasado_26h', 'vault-backup_ultima_tentativa_falhou', 'validade_r2_data_invalida'}
    else:
        raise AssertionError('alertas ocultos por validade inválida')
    print('OK achados 9/15/16: SIGTERM real (2x), falha registrada, último sucesso preservado, temporários limpos, id/data/bytes do restore')
    print('OK alerta: sucesso há 27 h, última tentativa falha e token próximo da validade recusam status saudável')
    print('OK F: validade sem fuso normalizada para UTC; validade inválida preserva todos os alertas de atraso/falha')


def prove_independent_jobs(root, cfg):
    # Exercita os mesmos dois comandos lançados pelo target, um deles falhando.
    bad_cfg = {**cfg, 'hub_home': str(root / 'hub-inexistente')}
    path = root / 'bad-config.json'
    path.write_text(json.dumps(bad_cfg))
    cli = ['python3', str(Path(__file__).with_name('backup.py'))]
    hub = subprocess.Popen(cli + ['backup', '--config', str(path)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    vault = subprocess.Popen(cli + ['vault-backup', '--config', str(path), '--keyrings',
                             str(root / 'vault-source/.local/share/keyrings')], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    hub.communicate(timeout=30)
    vault.communicate(timeout=30)
    assert hub.returncode == 1 and vault.returncode == 0
    folder = Path(__file__).parent
    target = (folder / 'donnabot-backup.target').read_text()
    assert 'Wants=donnabot-backup.service donnabot-vault-backup.service' in target
    for name in ('donnabot-backup.service', 'donnabot-vault-backup.service'):
        unit = (folder / name).read_text()
        assert unit.count('ExecStart=') == 1 and 'Restart=on-failure' in unit and 'RestartSec=15min' in unit
        assert 'StartLimitIntervalSec=1d' in unit and 'StartLimitBurst=3' in unit
    print('OK E/14: Hub falha e cofre conclui; unidades independentes, no máximo 3 partidas em 1 dia')


def prove_mutations(root):
    source = Path(__file__).with_name('backup.py').read_text()
    mutations = {
        'hash': ("require(digest(stage / name) == expected['sha256'], 'hash_falhou')", 'pass  # mutação hash'),
        'integrity': ("require(db.execute('PRAGMA integrity_check').fetchall() == [('ok',)], 'integridade_falhou')", 'pass  # mutação integrity'),
        'counts': ("require(stats(db) == meta['counts'], 'contagens_divergentes')", "stats(db)  # mutação counts"),
        'references': ('validate_references(db, stage, meta)\n', 'pass  # mutação references\n'),
        'files': ("set(meta['files']) == set(files) - {'manifest.json'}", 'True'),
        'mode': ("require(files[name] == expected['mode'], 'modo_divergente')", 'pass  # mutação mode'),
        'directories': ("meta['directories'] == directories", 'True'),
        'links': ("meta['links'] == links", 'True'),
        'kind': ("require(meta['kind'] == key[8:].split('-')[0], 'tipo_divergente')", 'pass  # mutação kind'),
        'token': ('require(db.execute("SELECT 1 FROM kv WHERE k=\'token\'").fetchone() is None, \'token_antigo_recusado\')', 'pass  # mutação token'),
        'format': ("meta['format'] == 2", 'True'),
        'payload-history': ("bases = ['bots/' + bot + '/workspace', 'bots/' + bot]",
                            "bases = ['bots/' + bot]"),
    }
    for label, (before, after) in mutations.items():
        assert source.count(before) == 1, label
        folder = root / ('mutant-' + label)
        folder.mkdir()
        (folder / 'backup.py').write_text(source.replace(before, after))
        for path in Path(__file__).parent.iterdir():
            if path.is_file() and path.name != 'backup.py':
                shutil.copyfile(path, folder / path.name)
        run = subprocess.run(['python3', str(folder / 'prove.py'), '--root', str(folder / 'fixture'), '--skip-mutations'],
                             capture_output=True, text=True, timeout=120)
        detector = 'anexos_historicos:' if label == 'payload-history' else 'recusada: ' + label
        assert run.returncode != 0 and detector in run.stderr, label + ': mutação não detectada pelo próprio cenário'
        print('OK mutação detectada: retirar proteção de ' + label + ' faz prove.py FALHAR')


def prove(root, mutations=True, payload_history_only=False):
    os.umask(0o077)
    root.mkdir(mode=0o700)  # Se já existe, não toca nem remove.
    keeper = None
    try:
        home = root / 'hub-ficticio'
        home.mkdir()
        state = root / 'state'
        state.mkdir()
        cfg = {'transport': 'local', 'hub_home': str(home), 'state_dir': str(state),
               'public_key': str(root / 'public.pem'), 'local_store': str(root / 's3-simulado')}
        (root / 'config.json').write_text(json.dumps(cfg))
        private = rsa.generate_private_key(public_exponent=65537, key_size=4096)
        (root / 'private.pem').write_bytes(private.private_bytes(serialization.Encoding.PEM,
                                      serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
        Path(cfg['public_key']).write_bytes(private.public_key().public_bytes(serialization.Encoding.PEM,
                                          serialization.PublicFormat.SubjectPublicKeyInfo))
        if payload_history_only:
            prove_payload_history(root, cfg)
            return
        keeper = sqlite3.connect(home / 'codync.db')
        keeper.executescript('''PRAGMA journal_mode=WAL;
            CREATE TABLE bots(id TEXT PRIMARY KEY,config TEXT,deleted INTEGER);
            CREATE TABLE entries(bot_id TEXT,data TEXT);
            CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT);
            CREATE TABLE devices(key TEXT);
            CREATE TABLE push_tickets(ticket TEXT);
            CREATE TABLE activity_tickets(ticket TEXT);
            INSERT INTO devices VALUES('PAREAMENTO-FICTICIO');
            INSERT INTO kv VALUES('vault.id','COFRE-FICTICIO');
            INSERT INTO kv VALUES('vault.initialized','true');
            INSERT INTO kv VALUES('token','TOKEN-KV-FICTICIO-NAO-COPIAR');
            INSERT INTO kv VALUES('schema','6');''')
        bots = [str(uuid.uuid4()), str(uuid.uuid4())]
        for bot in bots:
            folder = home / 'bots' / bot
            (folder / 'workspace').mkdir(parents=True)
            (folder / 'bot-memory').mkdir()
            (folder / 'workspace' / 'exemplo.txt').write_text('ARQUIVO-FICTICIO')
            (folder / 'workspace' / 'pasta-vazia').mkdir()
            readonly = folder / 'workspace/somente-leitura'
            readonly.mkdir()
            (readonly / 'modulo.txt').write_text('FICTICIO-0555')
            readonly.chmod(0o555)
            (folder / 'bot-memory' / 'memoria.md').write_text('MEMORIA-FICTICIA')
            (folder / 'workspace' / '.codex').mkdir()
            (folder / 'workspace' / '.codex' / 'anexo-a.json').write_text('LOGIN-NAO-COPIAR')
            (folder / 'workspace' / '.env').write_text('NAO-COPIAR')
            keeper.execute('INSERT INTO bots VALUES(?,?,0)', (bot, json.dumps({'cwd': str(folder / 'workspace')})))
            keeper.execute('INSERT INTO entries VALUES(?,?)', (bot, json.dumps({'text': 'CONVERSA-FICTICIA'})))
        published = str(uuid.uuid4())
        path = home / 'bots' / bots[0] / 'files' / published
        path.parent.mkdir()
        path.write_bytes(b'PUBLICADO-FICTICIO')
        keeper.execute('INSERT INTO entries VALUES(?,?)', (bots[0], json.dumps({'files': [
            {'id': published, 'size': path.stat().st_size, 'sha256': b.digest(path)}]})))
        (home / 'shared-memory').mkdir()
        (home / 'shared-memory' / 'compartilhada.md').write_text('MEMORIA-COMPARTILHADA')
        (home / 'harness-context.json').write_text('{"timeZone":"UTC"}')
        (home / 'token').write_text('TOKEN-GERAL-NAO-COPIAR')
        (home / 'identity.json').write_text('IDENTIDADE-NAO-COPIAR')
        # 1/2: nomes reservados, versões editadas e referências já ausentes.
        workspace = home / 'bots' / bots[0] / 'workspace'
        attachments = []
        for name in ('anexo-c.json', 'apresentacao.txt', 'certificado.txt', 'anexo-t.txt', 'anexo-a.json', 'anexo-i.json'):
            upload = str(uuid.uuid4())
            path = workspace / 'uploads' / upload / name
            path.parent.mkdir(parents=True)
            path.write_bytes(b'ANEXO-FICTICIO-EDITADO')
            attachments.append({'id': upload, 'name': name, 'size': 1})
        attachments.append({'id': str(uuid.uuid4()), 'name': 'apagado.txt', 'size': 7})
        keeper.execute('INSERT INTO entries VALUES(?,?)', (bots[0], json.dumps({'attachments': attachments})))
        # Publicado alterado e outro ausente também não bloqueiam o próximo ponto.
        path = home / 'bots' / bots[0] / 'files' / published
        path.write_bytes(b'PUBLICADO-ALTERADO')
        keeper.execute('INSERT INTO entries VALUES(?,?)', (bots[0], json.dumps({'files': [
            {'id': str(uuid.uuid4()), 'size': 8, 'sha256': '0' * 64}]})))
        executable = workspace / 'executar.sh'
        executable.write_text('#!/bin/sh\nexit 0\n')
        executable.chmod(0o751)
        (workspace / 'pasta-vazia').chmod(0o750)
        # 4: venv com alvo externo e links relativos/quebrados, sem seguir nenhum.
        (workspace / 'venv/bin').mkdir(parents=True)
        (workspace / 'venv/bin/python').symlink_to('/usr/bin/python3')
        (workspace / 'link-relativo').symlink_to('exemplo.txt')
        (workspace / 'link-quebrado').symlink_to('ausente')
        (workspace / 'link-diretorio').symlink_to(root)
        # 8: cwd descendente do próprio workspace, sem autorização de pasta externa.
        nested = home / 'bots' / bots[1] / 'workspace' / 'projeto'
        nested.mkdir()
        keeper.execute('UPDATE bots SET config=? WHERE id=?', (json.dumps({'cwd': str(nested)}), bots[1]))
        keeper.commit()
        assert (home / 'codync.db-wal').stat().st_size > 0
        original = b.digest(home / 'codync.db-wal')
        store = b.Storage(cfg)
        old = Path(cfg['local_store']) / 'backups' / ('hub-20200101T000000Z-' + '0' * 32 + '.dbkp')
        old.parent.mkdir(parents=True)
        old.write_bytes(b'EXPIRADO')
        os.utime(old, (time.time() - 31 * 86400,) * 2)
        unrelated = old.parent / 'nao-apagar.txt'
        unrelated.write_text('OUTRO')
        with b.tempfile.TemporaryDirectory(dir=state) as temp:
            with patch.object(b.serialization, 'load_pem_private_key', side_effect=AssertionError('privada usada no backup')):
                result = b.backup(cfg, Path(temp))
        key = result['object']
        assert old.exists() and unrelated.exists()  # Sem expiração automática nem DELETE.
        obj = old.parent / key[8:]
        assert b'CONVERSA-FICTICIA' not in obj.read_bytes()
        assert obj.read_bytes()[:8] == b.MAGIC
        # A geração acima não pôde carregar uma privada; a restauração precisa dela.
        with b.tempfile.TemporaryDirectory(dir=state) as temp:
            result = b.restore(cfg, Path(temp), key, root / 'private.pem', root / 'restaurado')
        restored = root / 'restaurado'
        assert result['bots'] == 2 and result['conversas'] == 2 and result['entradas'] == 5
        assert not (restored / 'token').exists() and not (restored / 'identity.json').exists()
        assert not (restored / 'bots' / bots[0] / 'workspace' / '.codex').exists()
        assert not list(restored.rglob('.env'))
        assert (restored / 'bots' / bots[0] / 'workspace' / 'pasta-vazia').is_dir()
        with contextlib.closing(sqlite3.connect(restored / 'codync.db')) as check:
            assert check.execute('SELECT count(*) FROM bots WHERE deleted=0').fetchone() == (2,)
            assert check.execute('SELECT count(DISTINCT bot_id) FROM entries').fetchone() == (2,)
            assert check.execute('SELECT count(*) FROM entries').fetchone() == (5,)
            assert check.execute('SELECT count(*) FROM devices').fetchone() == (0,)
            assert check.execute("SELECT 1 FROM kv WHERE k='token'").fetchone() is None
            assert b'TOKEN-KV-FICTICIO-NAO-COPIAR' not in (restored / 'codync.db').read_bytes()
            assert check.execute('SELECT v FROM kv WHERE k=\'vault.id\'').fetchone() == ('COFRE-FICTICIO',)
            assert check.execute('PRAGMA integrity_check').fetchone() == ('ok',)
        assert b.digest(home / 'codync.db-wal') == original
        assert keeper.execute("SELECT v FROM kv WHERE k='token'").fetchone() == ('TOKEN-KV-FICTICIO-NAO-COPIAR',)
        restored_workspace = restored / 'bots' / bots[0] / 'workspace'
        assert (restored_workspace / 'executar.sh').stat().st_mode & 0o777 == 0o751
        assert (restored_workspace / 'pasta-vazia').stat().st_mode & 0o777 == 0o750
        for relative, target in [('venv/bin/python', '/usr/bin/python3'), ('link-relativo', 'exemplo.txt'),
                                 ('link-quebrado', 'ausente'), ('link-diretorio', str(root))]:
            assert (restored_workspace / relative).is_symlink()
            assert os.readlink(restored_workspace / relative) == target
        assert not (restored_workspace / 'link-diretorio' / 'COPIADO-DO-ALVO').exists()
        manifest = json.loads((restored / 'manifest.json').read_text())
        assert len(manifest['references']) == 9
        assert sum(r['status'] == 'missing' for r in manifest['references']) == 2
        assert sum(r.get('matches_entry') is False for r in manifest['references']) == 7
        for attachment in attachments[:-1]:
            relative = 'bots/' + bots[0] + '/workspace/uploads/' + attachment['id'] + '/' + attachment['name']
            assert (restored / relative).read_bytes() == b'ANEXO-FICTICIO-EDITADO'
            assert b.digest(restored / relative) == manifest['files'][relative]['sha256']
        assert not any(name.startswith('external/') for name in manifest['files'])
        assert len(manifest['files']) == len(set(manifest['files']))
        assert result['object'] == key and result['created_at'] == manifest['created_at'] and result['bytes'] > 0
        print('OK backup SQLite online (WAL aberto) -> AES-256-GCM/RSA-4096 -> envio local -> download -> restauração isolada')
        print('OK integrity_check=ok; bots=2; conversas=2; entradas=5; manifesto, memórias e configurações conferidos')
        print('OK achados 1/2/4/6/8: anexos editados/ausentes, publicados divergentes, symlinks, modos e cwd aninhado')
        print('OK retenção: ponto antigo preservado mesmo sem backup novo; script não contém DELETE nem expiração')
        print('OK token, identidade, logins CLI e .env excluídos; grants limpos só na cópia; origem intacta')
        # Fronteiras de restauração: corrupção, chave errada, destino e tar hostil.
        with b.tempfile.TemporaryDirectory(dir=state) as temp:
            work = Path(temp)
            refused(lambda: b.restore(cfg, work, key, root / 'private.pem', restored))
            refused(lambda: b.restore(cfg, work, key, root / 'private.pem', home / 'novo'))
            corrupt = bytearray(obj.read_bytes())
            corrupt[-17] ^= 1
            bad = work / 'bad.dbkp'
            bad.write_bytes(corrupt)
            refused(lambda: b.unseal(bad, work / 'unauthenticated.tar.gz', root / 'private.pem'), InvalidTag)
            wrong = rsa.generate_private_key(public_exponent=65537, key_size=4096)
            wrong_path = work / 'wrong.pem'
            wrong_path.write_bytes(wrong.private_bytes(serialization.Encoding.PEM,
                                   serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
            refused(lambda: b.unseal(obj, work / 'wrong.tar.gz', wrong_path), ValueError)
            hostile = work / 'hostile.tar.gz'
            with tarfile.open(hostile, 'w:gz') as archive:
                member = tarfile.TarInfo('../fora')
                member.size = 1
                archive.addfile(member, io.BytesIO(b'x'))
            out = work / 'unpack'
            out.mkdir()
            refused(lambda: b.unpack(hostile, out))
            assert not (work / 'fora').exists()
            # Um link de diretório do tar jamais pode redirecionar outro membro.
            hostile_link = work / 'hostile-link.tar.gz'
            with tarfile.open(hostile_link, 'w:gz') as archive:
                member = tarfile.TarInfo('salto')
                member.type = tarfile.SYMTYPE
                member.linkname = str(root)
                archive.addfile(member)
                member = tarfile.TarInfo('salto/nao-escrever')
                member.size = 1
                archive.addfile(member, io.BytesIO(b'x'))
            link_out = work / 'unpack-link'
            link_out.mkdir()
            refused(lambda: b.unpack(hostile_link, link_out))
            assert not (root / 'nao-escrever').exists()
            # Escrever no WAL durante cópia detecta uma cópia fora da janela estável.
            def mutate(items, stage, *args):
                keeper.execute('INSERT INTO kv VALUES(\'mudanca\',\'fixture\')')
                keeper.commit()
                return real_copy(items, stage, *args)
            real_copy = b.copy_inventory
            changed = work / 'changed'
            changed.mkdir()
            with patch.object(b, 'copy_inventory', side_effect=mutate):
                refused(lambda: b.hub_snapshot(cfg, changed))
        print('OK corrupção, chave errada, pasta existente/Hub, travessia de tar/link e escrita concorrente recusados')
        prove_restore_guards(root, cfg, obj, key)
        prove_payload_history(root, cfg)
        prove_crossdisk(root, cfg, key)
        cli = ['python3', str(Path(__file__).with_name('backup.py'))]
        prove_control_paths(root, cfg, cli, key)
        prove_payload_race(root, cfg, workspace)
        prove_payload_scope(root, cfg, workspace)
        prove_coverage(root, cfg)
        prove_retry(root, cfg)
        # Secret Service real em D-Bus e HOME próprios, somente com chave fictícia.
        (root / 'master-ficticia').write_bytes(os.urandom(16).hex().encode())
        run_secret_service('write', root / 'vault-source')
        keyrings = root / 'vault-source' / '.local/share/keyrings'
        with b.tempfile.TemporaryDirectory(dir=state) as temp:
            vault = b.backup(cfg, Path(temp), keyrings)
        with b.tempfile.TemporaryDirectory(dir=state) as temp:
            b.restore(cfg, Path(temp), vault['object'], root / 'private.pem', root / 'cofre-restaurado')
        assert b.digest(root / 'cofre-restaurado' / 'keyrings' / 'login.keyring') == b.digest(keyrings / 'login.keyring')
        assert not (restored / 'keyrings').exists()
        recovery = root / 'vault-recovery'
        (recovery / '.local/share').mkdir(parents=True)
        shutil.copytree(root / 'cofre-restaurado' / 'keyrings', recovery / '.local/share/keyrings')
        run_secret_service('read', recovery)
        print('OK cofre fictício: Secret Service em D-Bus/HOME isolados, chave de 32 bytes recuperada com mesmo serviço/ID sem exibir valor')
        # Interface de curl/SigV4: credenciais não vão a argv e somente envelope é enviado.
        creds = root / 'r2.json'
        creds.write_text(json.dumps({'access_key_id': 'FIXTURE', 'secret_access_key': 'SEGREDOFICTICIO'}))
        prove_retention(root, cfg)
        r2 = {**cfg, 'transport': 'r2', 'account_id': '0' * 32, 'bucket': 'fixture-backups', 'credentials': str(creds)}
        with patch.object(b.subprocess, 'run') as run:
            run.return_value.returncode = 0
            run.return_value.stdout = b''
            b.curl(r2, 'PUT', key, upload=obj)
            argv = run.call_args.args[0]
            payload = run.call_args.kwargs['input'].decode()
            assert 'SEGREDOFICTICIO' not in ' '.join(argv)
            assert 'aws:amz:auto:s3' in payload and str(obj) in payload and b.digest(obj) in payload
        print('OK interface curl/SigV4: segredo só em stdin; upload aponta só para objeto cifrado (sem rede real)')
        # CLI real: status sanitizado, modos e limpeza dos temporários na falha.
        cli = ['python3', str(Path(__file__).with_name('backup.py'))]
        run = subprocess.run(cli + ['backup', '--config', str(root / 'config.json')], capture_output=True, text=True)
        assert run.returncode == 0, run.stderr
        health = json.loads(run.stdout)
        assert health['ok'] and not set(health) & {'bots', 'conversas', 'entradas', 'files'}
        notices = [json.loads(line) for line in run.stderr.splitlines()]
        assert len(notices) == 1 and notices[0]['warning'] == 'itens_excluidos_do_backup'
        assert notices[0]['count'] > 0 and set(notices[0]) == {'warning', 'count'}
        assert not list(state.glob('job-*'))
        tampered = obj.read_bytes()
        obj.write_bytes(tampered[:-1] + bytes([tampered[-1] ^ 1]))
        run = subprocess.run(cli + ['restore', '--config', str(root / 'config.json'), '--object', key,
                                   '--private-key', str(root / 'private.pem'), '--destination', str(root / 'nao-criar')],
                             capture_output=True, text=True)
        assert run.returncode == 1 and not (root / 'nao-criar').exists()
        assert json.loads(run.stderr)['error'] == 'InvalidTag' and not list(state.glob('job-*'))
        (root / 'config.json').chmod(0o644)
        run = subprocess.run(cli + ['backup', '--config', str(root / 'config.json')], capture_output=True, text=True)
        assert run.returncode == 1 and json.loads(run.stderr)['error'] == 'arquivo_exige_modo_600'
        (root / 'config.json').chmod(0o600)
        prove_status_and_interrupt(root, cfg, cli, key, obj, tampered)
        prove_independent_jobs(root, cfg)
        if mutations:
            prove_mutations(root)
        print('OK CLI real: status sem conteúdo/contagens, modo 600 exigido, falha registrada e temporários removidos')
    finally:
        if keeper:
            keeper.close()
        b.remove_private_tree(root)
        print('OK pasta de teste e chaves fictícias removidas')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--payload-history-only', action='store_true', help='Somente o cenário diferencial da correção 3')
    parser.add_argument('--vault-phase', choices=('write', 'read'))
    parser.add_argument('--skip-mutations', action='store_true')
    parser.add_argument('--private-roots-only', action='store_true', help='Somente negativos D520 de pastas privadas')
    args = parser.parse_args()
    assert args.root.is_absolute()
    if args.private_roots_only:
        prove_private_roots(args.root)
    elif args.vault_phase:
        secret_service_phase(args.vault_phase, args.root)
    else:
        prove(args.root, not args.skip_mutations, args.payload_history_only)
