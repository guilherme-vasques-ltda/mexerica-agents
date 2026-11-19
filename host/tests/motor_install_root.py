#!/usr/bin/env python3
"""D330 F3b, somente VM descartável: SCRIPT HELPER HOST TEST_BIN --vm-descartavel."""
import io
from base64 import b64encode
from hashlib import sha512
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import pwd
import shutil
import subprocess
import sys
import tarfile
import tempfile
from threading import Thread

assert os.geteuid() == 0 and len(sys.argv) == 5 and sys.argv[4] == '--vm-descartavel'
helper = Path('/opt/donnabot/bin/donnabot-motor-exec')
config = Path('/etc/donnabot/motor.conf')
base = Path('/var/lib/donnabot')
assert not helper.exists() and not config.exists() and not base.exists(), 'VM já configurada'
created = []
command_env = {"PATH": "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin", "HOME": "/"}
private = Path(tempfile.mkdtemp(prefix='d330-f3b-hub-'))
registro = None
servidor = None
try:
    for name in ['d330-f3b-hub', 'd330-f3b-motor']:
        subprocess.run(['/usr/sbin/useradd', '--system', '--no-create-home', name], env=command_env, check=True)
        created.append(name)
    hub, motor = (pwd.getpwnam(name) for name in created)
    os.chown(private, hub.pw_uid, hub.pw_gid)
    private.chmod(0o700)
    for name in ['token', 'db.sqlite', 'hub-binary', 'old-cli']:
        path = private / name
        path.write_text('sentinela fictícia do Hub')
        os.chown(path, hub.pw_uid, hub.pw_gid)
        path.chmod(0o700 if name in ['hub-binary', 'old-cli'] else 0o600)
    hub_bin = private / 'bin'
    hub_bin.mkdir()
    os.chown(hub_bin, hub.pw_uid, hub.pw_gid)
    for name in ['codex-acp', 'claude-agent-acp', 'claude-code-acp']:
        path = hub_bin / name
        path.write_text('#!/bin/sh\nexit 91\n')
        os.chown(path, hub.pw_uid, hub.pw_gid)
        path.chmod(0o700)
    base.mkdir(mode=0o755)
    home = base / 'motor'
    home.mkdir(mode=0o700)
    os.chown(home, motor.pw_uid, motor.pw_gid)
    work = base / 'trabalho'
    work.mkdir()
    os.chown(work, hub.pw_uid, motor.pw_gid)
    work.chmod(0o2770)
    helper.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(sys.argv[1], helper)
    os.chown(helper, 0, hub.pw_gid)
    helper.chmod(0o750)
    subprocess.run(['/usr/sbin/setcap', 'cap_setuid,cap_setgid+ep', str(helper)], env=command_env, check=True)
    config.parent.mkdir(exist_ok=True)
    config.write_text(f'motor_user={motor.pw_name}\nhub_uid={hub.pw_uid}\nwork_gid={motor.pw_gid}\n')
    config.chmod(0o644)
    package = home / 'fixture.tgz'
    probe = r'''
const fs=require('fs'),path=require('path');
const hub=JSON.parse(fs.readFileSync(path.join(__dirname,'proof.json'),'utf8')).hub;
function readHub(){try{fs.readFileSync(path.join(hub,'token'));return true}catch{return false}}
function writeHub(name){try{fs.appendFileSync(path.join(hub,name),'alteracao');return true}catch{return false}}
const proof={uid:process.getuid(),readHub:readHub(),writeHub:writeHub('db.sqlite'),writeBinary:writeHub('hub-binary'),hubEnvironment:Object.hasOwn(process.env,'DONNABOT_PROOF_SECRET'),executable:fs.realpathSync(__filename)};
'''
    postinstall = probe + r'''
fs.writeFileSync(path.join(process.env.HOME,'postinstall-proof.json'),JSON.stringify(proof));
console.log('{"result":"command","program":"/forjado","args":""}');
'''
    acp = '#!/usr/bin/env node\n' + probe + r'''
require('readline').createInterface({input:process.stdin}).on('line',line=>{
const msg=JSON.parse(line);if(!('id' in msg))return;
const result=msg.method==='initialize'?{protocolVersion:1,agentCapabilities:{}}:proof;
console.log(JSON.stringify({jsonrpc:'2.0',id:msg.id,result}));
});
'''
    files = {'package.json': json.dumps({'name':'d330-fixture-acp','version':'1.0.0','bin':{name:'acp.js' for name in ['d330-fixture-acp','codex-acp','claude-agent-acp','claude-code-acp']},'scripts':{'postinstall':'node postinstall.js'}}), 'proof.json':json.dumps({'hub':str(private)}),'postinstall.js':postinstall,'acp.js':acp}
    with tarfile.open(package, 'w:gz') as archive:
        for name, content in files.items():
            data = content.encode()
            entry = tarfile.TarInfo('package/' + name)
            entry.size = len(data)
            entry.mode = 0o755 if name == 'acp.js' else 0o644
            archive.addfile(entry, io.BytesIO(data))
    os.chown(package, motor.pw_uid, motor.pw_gid)
    pacote = json.loads(files['package.json'])
    nome, versao = pacote['name'], pacote['version']
    conteudo = package.read_bytes()

    class RegistroLocal(BaseHTTPRequestHandler):
        def log_message(self, formato, *args):
            pass

        def do_GET(self):
            if self.path == '/' + nome:
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(ficha)
            elif self.path == '/fixture.tgz':
                self.send_response(200)
                self.send_header('Content-Type', 'application/octet-stream')
                self.end_headers()
                self.wfile.write(conteudo)
            else:
                self.send_error(404)

    registro = ThreadingHTTPServer(('127.0.0.1', 0), RegistroLocal)
    url = f'http://127.0.0.1:{registro.server_port}'
    pacote['dist'] = {'tarball': url + '/fixture.tgz', 'integrity': 'sha512-' + b64encode(sha512(conteudo).digest()).decode()}
    ficha = json.dumps({'name': nome, 'versions': {versao: pacote}}).encode()
    npmrc = home / 'npm-globalrc'
    npmrc.write_text(f'registry={url}/\naudit=false\nfund=false\n')
    os.chown(npmrc, motor.pw_uid, motor.pw_gid)
    servidor = Thread(target=registro.serve_forever, daemon=True)
    servidor.start()
    def as_hub():
        os.setgroups([motor.pw_gid])
        os.setresgid(hub.pw_gid, hub.pw_gid, hub.pw_gid)
        os.setresuid(hub.pw_uid, hub.pw_uid, hub.pw_uid)
        os.umask(0o077)
    proof_env = {
        'PATH': str(hub_bin) + ':' + command_env['PATH'],
        'HOME': str(private),
        'MEXERICA_HOME': str(private),
        'DONNABOT_MOTOR_PROOF_HOST': str(Path(sys.argv[2]).resolve()),
        'DONNABOT_PROOF_PACKAGE': f'{nome}@{versao}',
        'DONNABOT_PROOF_OLD': str(private / 'old-cli'),
        'DONNABOT_PROOF_SECRET': 'FALSO_TESTE',
    }
    subprocess.run([sys.argv[3], '--ignored', '--exact', 'fork::motor_install::tests::instalador_prova_uid_real', '--nocapture'], env=proof_env, preexec_fn=as_hub, check=True, timeout=900)
    for path in home.rglob('*'):
        assert path.lstat().st_uid == motor.pw_uid, f'arquivo não pertence ao motor: {path}'
    for name in ['token','db.sqlite','hub-binary','old-cli']:
        assert (private/name).read_text() == 'sentinela fictícia do Hub'
    print('PASS raiz publicada pertence ao motor; sentinelas privadas do Hub intactas',flush=True)
finally:
    if registro is not None:
        if servidor is not None and servidor.is_alive():
            registro.shutdown()
        registro.server_close()
    if len(created) == 2 and config.exists() and helper.exists() and 'as_hub' in locals():
        subprocess.run([str(helper), 'cleanup'], env=command_env, preexec_fn=as_hub, check=True, timeout=15)
    config.unlink(missing_ok=True)
    helper.unlink(missing_ok=True)
    shutil.rmtree(base,ignore_errors=True)
    shutil.rmtree(private)
    for name in reversed(created):
        subprocess.run(['/usr/sbin/userdel',name],env=command_env,check=True)
