import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { analisarSaida } from './quadros.ts'
import { avaliarSondagem, classificarEstado812 } from './sondagem.ts'
import { HASH_LEITURA_ROOT, HASH_SONDAGEM, SCRIPT_LEITURA_ROOT, SCRIPT_SONDAGEM, sha256Texto } from './scripts.ts'

const linux = process.platform === 'linux'
const roda = (script: string, cwd?: string) => spawnSync('/bin/sh', ['-s'], { input: script, cwd, env: { PATH: '/usr/sbin:/usr/bin:/sbin:/bin', HOME: '/nonexistent' } })

test('hash dos scripts fixos: mudar o texto exige mudar o hash aqui, de propósito, na revisão', () => {
  assert.equal(sha256Texto(SCRIPT_SONDAGEM), HASH_SONDAGEM)
  assert.equal(sha256Texto(SCRIPT_LEITURA_ROOT), HASH_LEITURA_ROOT)
  assert.match(HASH_SONDAGEM, /^[0-9a-f]{64}$/, 'hash literal no código, não calculado do próprio texto')
  assert.match(HASH_LEITURA_ROOT, /^[0-9a-f]{64}$/)
})

test('scripts: sem aspa simples, crase, NUL nem texto livre; sintaxe válida em sh', () => {
  for (const s of [SCRIPT_SONDAGEM, SCRIPT_LEITURA_ROOT]) {
    assert.ok(!s.includes("'") && !s.includes('`') && !s.includes('\u0000'))
    assert.ok(!s.includes('${ESPACO') && !s.includes('@{'), 'os marcadores @{ foram trocados')
    assert.ok(s.length < 8192, 'cabe numa linha de comando')
    assert.ok(Buffer.byteLength(s) === s.length, 'só ASCII')
    assert.equal(spawnSync('/bin/sh', ['-n', '-c', s]).status, 0)
  }
  for (const proibido of [/\bcurl\b/, /\bwget\b/, /\brm\b/, /\bmv\b/, /\bchmod\b/, /\buseradd\b/, />\s*[^&\s/]/, /\btee\b/]) {
    assert.doesNotMatch(SCRIPT_SONDAGEM, proibido, `sondagem (a) é só leitura: ${proibido}`)
    assert.doesNotMatch(SCRIPT_LEITURA_ROOT, proibido, `leitura root é só leitura: ${proibido}`)
  }
})

test('sondagem (a) rodando de verdade (leitura): só quadros, nada descartado, eventos coerentes', { skip: !linux }, () => {
  const r = roda(SCRIPT_SONDAGEM)
  assert.equal(r.status, 0)
  const q = analisarSaida(r.stdout)
  assert.equal(q.falha, null)
  assert.equal(q.descartadas, 0, 'a sondagem só imprime quadros')
  assert.equal(r.stderr.length, 0)
  const codigos = q.eventos.map((e) => e.codigo)
  for (const c of ['so', 'arquitetura', 'systemd', 'espaco', 'sudo', 'sudotty', 'sudosemsenha', 'libcap', 'nosuid', 'hardlinks', 'relogio', 'linger', 'motor', 'hostinst', 'contahub', 'ehhub', 'portaocupada']) {
    assert.equal(codigos.filter((x) => x === c).length, 1, c)
  }
  const uid = q.dados.find((d) => d.tipo === 'uid')
  assert.ok(uid && uid.tipo === 'uid' && uid.uidAdmin === process.getuid?.())
  const avaliacao = avaliarSondagem(q)
  assert.equal(avaliacao.uidAdmin, process.getuid?.() ?? null)
  assert.ok(['sem-senha', 'pede-senha', 'indisponivel'].includes(avaliacao.sudo))
})

test('leitura root: sem ser root responde `ilegivel` (nunca limpo)', { skip: !linux || process.getuid?.() === 0 }, () => {
  assert.equal(classificarEstado812(analisarSaida(roda(SCRIPT_LEITURA_ROOT).stdout)), 'ilegivel')
})

// A lógica de ausência comprovada, rodando o mesmo script contra uma árvore de teste (caminhos remapeados, "root" simulado).
// `conta`: nome consultado no `getent` no lugar da conta do Hub (inexistente = ausente; `root` = presente).
// `inventario`: liga o `inventario=1` (o que a F1 fará quando o inventário do #812 existir).
function leituraEmArvore(montar: (raiz: string) => void, opcoes: { conta?: string; inventario?: boolean } = {}) {
  const raiz = mkdtempSync(join(tmpdir(), 'mex-leitura-'))
  try {
    mkdirSync(join(raiz, 'etc/systemd/system'), { recursive: true })
    mkdirSync(join(raiz, 'usr/lib/systemd/system'), { recursive: true })
    montar(raiz)
    let script = SCRIPT_LEITURA_ROOT.replaceAll('/var/lib/donnabot', `${raiz}/var/lib/donnabot`).replaceAll('/opt/donnabot', `${raiz}/opt/donnabot`).replaceAll('/etc/sudoers.d', `${raiz}/etc/sudoers.d`)
      .replaceAll('/etc/systemd/system', `${raiz}/etc/systemd/system`).replaceAll('/usr/lib/systemd/system', `${raiz}/usr/lib/systemd/system`)
      .replace('[ "$(id -u)" = 0 ]', 'true').replace('getent passwd donnabot-hub', `getent passwd ${opcoes.conta ?? 'mex-conta-que-nao-existe'}`)
    if (opcoes.inventario) script = script.replace('inventario=0', 'inventario=1')
    return classificarEstado812(analisarSaida(roda(script).stdout))
  } finally {
    spawnSync('chmod', ['-R', 'u+rwx', raiz])
    rmSync(raiz, { recursive: true, force: true })
  }
}

test('leitura root: com o inventário do #812 incompleto (F0) nunca devolve limpo-comprovado, nem sem vestígio algum', { skip: !linux }, () => {
  assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }) }), 'ilegivel')
})

test('leitura root: conferidos todos os vestígios e inventário completo = limpo-comprovado', { skip: !linux }, () => {
  const com = { inventario: true }
  assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'opt'), { recursive: true }); mkdirSync(join(r, 'var'), { recursive: true }); mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }); writeFileSync(join(r, 'etc/sudoers.d/outro'), 'x') }, com), 'limpo-comprovado')
  assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }) }, com), 'limpo-comprovado', 'pais ausentes também provam ausência')
})

test('leitura root: qualquer vestígio (caminhos, sudoers, unidades, conta do Hub) = incompleto; nunca limpo', { skip: !linux }, () => {
  const com = { inventario: true }
  for (const caminho of ['var/lib/donnabot/atualizacao', 'opt/donnabot/sbin', 'opt/donnabot/releases', 'opt/donnabot/bin']) {
    assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, caminho), { recursive: true }); mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }) }, com), 'incompleto', caminho)
  }
  assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }); writeFileSync(join(r, 'etc/sudoers.d/donnabot-atualizacao'), 'x') }, com), 'incompleto')
  for (const unidade of ['etc/systemd/system/donnabot-hub.service', 'usr/lib/systemd/system/donnabot-host.service']) {
    assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }); writeFileSync(join(r, unidade), 'x') }, com), 'incompleto', unidade)
  }
  assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }) }, { ...com, conta: 'root' }), 'incompleto', 'conta do Hub existente')
})

test('leitura root: pai ilegível, sudoers.d ou pasta de unidades ausente = ilegivel, nunca limpo', { skip: !linux || process.getuid?.() === 0 }, () => {
  const com = { inventario: true }
  assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'opt/donnabot'), { recursive: true }); chmodSync(join(r, 'opt/donnabot'), 0o000); mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }) }, com), 'ilegivel')
  assert.equal(leituraEmArvore(() => {}, com), 'ilegivel', 'sem /etc/sudoers.d a leitura não é completa')
  assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }); rmSync(join(r, 'usr/lib/systemd/system'), { recursive: true }) }, com), 'ilegivel', 'sem pasta de unidades')
  assert.equal(leituraEmArvore((r) => { mkdirSync(join(r, 'etc/sudoers.d'), { recursive: true }); chmodSync(join(r, 'etc/systemd/system'), 0o000) }, com), 'ilegivel', 'pasta de unidades ilegível')
})

// V6: o `sudo -n` é sondado antes de qualquer senha. Um `sudo` de mentira na frente do PATH reproduz cada resposta real.
function sondagemComSudo(corpo: string | null) {
  const pasta = mkdtempSync(join(tmpdir(), 'mex-sudo-'))
  try {
    if (corpo !== null) {
      writeFileSync(join(pasta, 'sudo'), `#!/bin/sh\n${corpo}\n`)
      chmodSync(join(pasta, 'sudo'), 0o755)
    }
    // Sem `sudo` real no caminho: só o diretório de teste mais as ferramentas básicas.
    const semSudo = join(pasta, 'bin')
    mkdirSync(semSudo)
    for (const f of ['uname', 'df', 'tail', 'cat', 'cut', 'id', 'grep', 'getent', 'findmnt', 'timedatectl', 'ss', 'printf', 'sh', 'ls']) {
      const real = spawnSync('/bin/sh', ['-c', `command -v ${f}`]).stdout.toString().trim()
      if (real.startsWith('/')) spawnSync('ln', ['-s', real, join(semSudo, f)])
    }
    const script = SCRIPT_SONDAGEM.replace('PATH=/usr/sbin:/usr/bin:/sbin:/bin', `PATH=${pasta}:${semSudo}`)
    const saida = analisarSaida(roda(script).stdout)
    return Object.fromEntries(saida.eventos.filter((e) => e.codigo.startsWith('sudo')).map((e) => [e.codigo, e.estado]))
  } finally {
    rmSync(pasta, { recursive: true, force: true })
  }
}

test('V6: `sudo -n` passa = sem senha; exige senha = pede; tty = incompatível; sem sudo = indisponível', { skip: !linux }, () => {
  assert.deepEqual(sondagemComSudo('exit 0'), { sudo: 'ok', sudotty: 'ok', sudosemsenha: 'ok' })
  assert.deepEqual(sondagemComSudo('echo "sudo: a password is required" >&2; exit 1'), { sudo: 'ok', sudotty: 'ok', sudosemsenha: 'pulado' })
  assert.deepEqual(sondagemComSudo('echo "sudo: a terminal is required to read the password" >&2; exit 1'), { sudo: 'ok', sudotty: 'falha', sudosemsenha: 'falha' })
  assert.deepEqual(sondagemComSudo('echo "sudo: sorry, you must have a tty to run sudo" >&2; exit 1'), { sudo: 'ok', sudotty: 'falha', sudosemsenha: 'falha' })
  assert.deepEqual(sondagemComSudo('echo "sudo: outra coisa" >&2; exit 1'), { sudo: 'ok', sudotty: 'ok', sudosemsenha: 'falha' })
  assert.deepEqual(sondagemComSudo(null), { sudo: 'falha', sudotty: 'pulado', sudosemsenha: 'falha' })
})
