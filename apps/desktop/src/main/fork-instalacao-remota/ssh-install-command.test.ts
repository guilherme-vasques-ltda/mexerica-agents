import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import {
  ambienteSsh, AlvoInvalido, argumentosLeituraRoot, argumentosSondagem, COMANDO_LEITURA_COM_SENHA, COMANDO_LEITURA_SEM_SENHA, COMANDO_SONDAGEM,
  entregarSenha, textosFixosIntegros, validarAlvo, type Alvo,
} from './ssh-install-command.ts'

const H = '/Users/user'
const alvo: Alvo = { host: 'servidor-teste.example', port: 2222, user: 'admin', identityFile: '/Users/user/.ssh/id_teste' }
const SENTINELA = 'Sentinela-Senha-9f3a!'

const todos = (a: Alvo) => [
  argumentosSondagem(a, H), argumentosLeituraRoot(a, H, 'com-senha'), argumentosLeituraRoot(a, H, 'sem-senha'),
]

test('formato fixo: -T, BatchMode, sem encaminhamentos, known_hosts do app, -- antes do destino', () => {
  for (const args of todos(alvo)) {
    assert.equal(args[0], '-T')
    for (const o of ['BatchMode=yes', 'ForwardAgent=no', 'ForwardX11=no', 'ClearAllForwardings=yes', 'StrictHostKeyChecking=yes', 'ProxyCommand=none', 'ProxyJump=none', 'ControlPath=none', 'PermitLocalCommand=no', 'LocalCommand=none']) {
      assert.ok(args.includes(o), o)
    }
    assert.ok(args.some((a) => a.startsWith('UserKnownHostsFile=') && a.includes('/Users/user/.mexerica/ssh_known_hosts')))
    const i = args.indexOf('--')
    assert.equal(args.length - i, 3, 'depois do -- só o destino e o comando remoto')
    assert.equal(args[i + 1], alvo.host)
    assert.deepEqual(args.slice(args.indexOf('-p'), args.indexOf('-p') + 2), ['-p', '2222'])
    assert.deepEqual(args.slice(args.indexOf('-l'), args.indexOf('-l') + 2), ['-l', 'admin'])
    assert.ok(!args.includes('-t') && !args.includes('-tt') && !args.includes('-A') && !args.includes('-X'))
  }
  assert.equal(argumentosSondagem(alvo, H).at(-1), COMANDO_SONDAGEM)
  assert.equal(argumentosLeituraRoot(alvo, H, 'com-senha').at(-1), COMANDO_LEITURA_COM_SENHA)
  assert.equal(argumentosLeituraRoot(alvo, H, 'sem-senha').at(-1), COMANDO_LEITURA_SEM_SENHA)
})

test('V6: com `sudo -n` aprovado o comando não lê senha; com senha usa `sudo -S -k` e prompt vazio', () => {
  assert.match(COMANDO_LEITURA_SEM_SENHA, /^\/usr\/bin\/sudo -n \/bin\/sh -c '/)
  assert.doesNotMatch(COMANDO_LEITURA_SEM_SENHA, /sudo -S/)
  assert.match(COMANDO_LEITURA_COM_SENHA, /^\/usr\/bin\/sudo -S -k -p '' \/bin\/sh -c '/)
})

test('o comando remoto é constante: não muda com host, usuário, porta nem chave', () => {
  const outro: Alvo = { host: 'outro', port: null, user: null, identityFile: null }
  assert.equal(argumentosSondagem(outro, H).at(-1), argumentosSondagem(alvo, H).at(-1))
  assert.equal(argumentosLeituraRoot(outro, H, 'com-senha').at(-1), argumentosLeituraRoot(alvo, H, 'com-senha').at(-1))
  assert.ok(!argumentosSondagem(outro, H).includes('-p') && !argumentosSondagem(outro, H).includes('-l') && !argumentosSondagem(outro, H).includes('-i'))
})

test('recusa host, usuário, porta e chave fora do formato (com `;`, `$()`, `-oProxyCommand=`, quebra de linha)', () => {
  const hostis = ['', '-oProxyCommand=touch /tmp/x', '-o', 'a;b', 'a b', '$(id)', '`id`', 'a|b', 'a&b', 'a\nb', 'a\u0000b', 'a/b', 'a:b', "a'b", 'a"b', 'x'.repeat(254)]
  for (const host of hostis) assert.equal(validarAlvo({ ...alvo, host }), 'host', JSON.stringify(host))
  for (const user of ['', '-l', 'a b', 'a;b', '$(id)', '1abc', 'a\nb', 'x'.repeat(65)]) assert.equal(validarAlvo({ ...alvo, user }), 'usuario', JSON.stringify(user))
  for (const port of [0, 65536, -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) assert.equal(validarAlvo({ ...alvo, port }), 'porta', String(port))
  for (const identityFile of ['', 'relativo/chave', '-oProxyCommand=x', '/a\nb', '/a\u0000b']) assert.equal(validarAlvo({ ...alvo, identityFile }), 'chave', JSON.stringify(identityFile))
  for (const host of ['a;b', '-oProxyCommand=x']) {
    assert.throws(() => argumentosSondagem({ ...alvo, host }, H), AlvoInvalido)
    assert.throws(() => argumentosLeituraRoot({ ...alvo, host }, H, 'sem-senha'), AlvoInvalido)
  }
  assert.throws(() => argumentosSondagem(alvo, '/home/user"b'), AlvoInvalido, 'pasta com aspas quebraria o UserKnownHostsFile')
  assert.throws(() => argumentosSondagem(alvo, 'relativo'), AlvoInvalido)
})

test('fuzz: texto hostil nunca vira opção, comando remoto nem segundo elemento fora do lugar', () => {
  let semente = 987654321
  const rnd = () => { semente = (semente * 1103515245 + 12345) & 0x7fffffff; return semente }
  const pecas = ['a', 'B', '9', '.', '-', '_', ';', '$(', ')', '`', ' ', '\n', "'", '"', '|', '&', '-o', 'ProxyCommand=', '=', '/', '\\', '\u0000', '${', '*', '~', '#']
  const remotos = new Set([COMANDO_SONDAGEM, COMANDO_LEITURA_COM_SENHA, COMANDO_LEITURA_SEM_SENHA])
  let aceitos = 0
  for (let i = 0; i < 6000; i++) {
    const gera = () => Array.from({ length: 1 + (rnd() % 10) }, () => pecas[rnd() % pecas.length]).join('')
    const candidato: Alvo = { host: gera(), port: rnd() % 3 === 0 ? null : rnd() % 70000, user: rnd() % 2 === 0 ? null : gera(), identityFile: rnd() % 2 === 0 ? null : '/' + gera() }
    if (validarAlvo(candidato) !== null) {
      assert.throws(() => argumentosSondagem(candidato, H), AlvoInvalido)
      continue
    }
    aceitos++
    for (const args of todos(candidato)) {
      const i = args.indexOf('--')
      assert.equal(args[i + 1], candidato.host, 'o host é um elemento próprio, depois do --')
      assert.ok(!candidato.host.startsWith('-'))
      assert.ok(remotos.has(args[i + 2]!), 'o comando remoto é um dos três fixos')
      assert.equal(args.length, i + 3)
      // O texto do usuário só aparece como valor das opções -l, -p, -i (e depois do --), nunca no comando remoto.
      for (const [indice, a] of args.entries()) {
        if (indice > i) continue
        if (candidato.user !== null && a === candidato.user) assert.equal(args[indice - 1], '-l')
        if (candidato.identityFile !== null && a === candidato.identityFile) assert.equal(args[indice - 1], '-i')
      }
      assert.ok(!args[i + 2]!.includes(candidato.host) || candidato.host.length < 3, 'host não entra no comando remoto')
    }
  }
  assert.ok(aceitos > 50, `o fuzz precisa também exercitar entradas aceitas (${aceitos})`)
})

test('senha: a API de argumentos não recebe senha; sentinela não aparece em argv nem em ambiente', () => {
  assert.equal(argumentosLeituraRoot.length, 3, 'host/pasta/modo (+ pasta de dados opcional): nenhum parâmetro de senha')
  const ambiente = ambienteSsh({ PATH: '/usr/bin', HOME: H, SSH_AUTH_SOCK: '/tmp/agente', SSH_ASKPASS: '/x', SSH_ASKPASS_REQUIRE: 'force', SENHA: SENTINELA, MEXERICA_SENHA: SENTINELA, TOKEN: 'segredo' })
  assert.deepEqual(Object.keys(ambiente).sort(), ['HOME', 'LC_ALL', 'PATH', 'SSH_AUTH_SOCK'])
  assert.ok(!JSON.stringify(ambiente).includes(SENTINELA))
  for (const args of todos(alvo)) assert.ok(!args.join('\n').includes(SENTINELA))
  assert.deepEqual(Object.keys(ambienteSsh({})), ['LC_ALL'])
})

test('entregarSenha: vai só ao stdin com \\n; zera só DEPOIS da escrita assíncrona terminar', async () => {
  const senha = Buffer.from(SENTINELA)
  let copiaVista: Buffer | null = null
  let pronto: ((erro?: Error | null) => void) | null = null
  const emVoo = entregarSenha(senha, (b, cb) => { copiaVista = b; pronto = cb })
  await new Promise((r) => setImmediate(r))
  assert.equal((copiaVista as unknown as Buffer).toString(), SENTINELA + '\n', 'escrita pendente: a cópia ainda está íntegra')
  assert.ok(senha.some((b) => b !== 0), 'o buffer do chamador ainda não foi zerado')
  let terminou = false
  void emVoo.then(() => { terminou = true })
  await new Promise((r) => setImmediate(r))
  assert.equal(terminou, false, 'sem callback a entrega não termina')
  pronto!(null)
  await emVoo
  assert.ok(senha.every((b) => b === 0), 'o buffer do chamador foi zerado')
  assert.ok((copiaVista as unknown as Buffer).every((b) => b === 0), 'a cópia entregue foi zerada')
})

test('entregarSenha: recusa formato inválido antes de alocar; zera mesmo em erro de escrita', async () => {
  for (const ruim of [Buffer.alloc(0), Buffer.alloc(1025, 65), Buffer.from('a\nb'), Buffer.from('a\rb'), Buffer.from('a\u0000b')]) {
    let chamou = false
    await assert.rejects(entregarSenha(ruim, () => { chamou = true }), RangeError)
    assert.equal(chamou, false)
    assert.ok(ruim.every((b) => b === 0), 'zerada mesmo na recusa')
  }
  const erro = Buffer.from('x')
  let vista: Buffer | null = null
  await assert.rejects(entregarSenha(erro, (b, cb) => { vista = b; setImmediate(() => cb(new Error('pipe fechado'))) }), /pipe fechado/)
  assert.ok(erro.every((b) => b === 0) && (vista as unknown as Buffer).every((b) => b === 0), 'zerada mesmo se a escrita falhar')
  const lancou = Buffer.from('x')
  await assert.rejects(entregarSenha(lancou, () => { throw new Error('boom') }), /boom/)
  assert.ok(lancou.every((b) => b === 0))
  await entregarSenha(Buffer.alloc(1024, 65), (_b, cb) => cb(), )
})

test('ssh -G: PermitLocalCommand=no vence ssh_config herdado com LocalCommand e proxies', { skip: spawnSync('ssh', ['-V']).error !== undefined }, () => {
  const pasta = mkdtempSync(join(tmpdir(), 'mex-ssh-g-'))
  try {
    const cfg = join(pasta, 'config')
    writeFileSync(cfg, 'Host *\n  PermitLocalCommand yes\n  LocalCommand touch /tmp/nao-deve-rodar\n  ProxyCommand nc %h %p\n  ForwardAgent yes\n  LocalForward 1 127.0.0.1:2\n')
    const sem = spawnSync('ssh', ['-G', '-F', cfg, 'host-teste'], { encoding: 'utf8' }).stdout
    assert.match(sem, /^permitlocalcommand yes$/m, 'a configuração sintética liga o comando local')
    const args = argumentosSondagem({ host: 'host-teste', port: null, user: null, identityFile: null }, H)
    const i = args.indexOf('--')
    const com = spawnSync('ssh', ['-G', '-F', cfg, ...args.slice(0, i), 'host-teste'], { encoding: 'utf8' }).stdout
    assert.match(com, /^permitlocalcommand no$/m)
    assert.doesNotMatch(com, /^localcommand touch/m)
    assert.doesNotMatch(com, /^proxycommand nc/m)
    assert.match(com, /^forwardagent no$/m)
    assert.doesNotMatch(com, /^localforward/m)
  } finally {
    rmSync(pasta, { recursive: true, force: true })
  }
})

test('os textos fixos batem com o hash esperado', () => {
  assert.equal(textosFixosIntegros(), true)
})
