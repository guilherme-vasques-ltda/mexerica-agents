#!/usr/bin/env node
// Comando único da web local: sobe o Hub (donnabot-host) servindo a build web na mesma porta
// em 127.0.0.1 e abre o navegador. A sessão nasce no navegador com o token local do Hub — o
// token nunca passa por este script: só apontamos o caminho do arquivo, sem ler nem imprimir.
//
// Uso: npm run web [-- --port 19423] [--build] [--no-open]
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'

const args = process.argv.slice(2)
const temFlag = (nome) => args.includes(nome)
const valorFlag = (nome) => {
  const i = args.indexOf(nome)
  return i >= 0 && i + 1 < args.length ? args[i + 1] : undefined
}

// --port é validado aqui mesmo: nada de passar NaN ou 0 adiante ao Hub.
const temPortFlag = temFlag('--port')
const portaBruta = valorFlag('--port')
if (temPortFlag && portaBruta === undefined) {
  console.error('--port sem valor. Use: --port <inteiro entre 1 e 65535>.')
  process.exit(1)
}
if (portaBruta !== undefined) {
  const portaInformada = Number(portaBruta)
  if (!Number.isInteger(portaInformada) || portaInformada < 1 || portaInformada > 65535) {
    console.error(`--port inválido: "${portaBruta}" não é um inteiro entre 1 e 65535.`)
    process.exit(1)
  }
}
const porta = portaBruta !== undefined ? Number(portaBruta) : 19423
const app = resolve(import.meta.dirname, '..')
const webDir = join(app, 'dist/web')

if (temFlag('--build')) {
  const build = spawn(process.execPath, [join(app, 'node_modules/vite/bin/vite.js'), 'build', '--config', 'vite.web.config.ts'], { cwd: app, stdio: 'inherit' })
  // Espera a build terminar antes de continuar (o Hub serve a pasta pronta).
  await new Promise((resolvePromise, rejectPromise) => {
    build.on('exit', (code) => (code === 0 ? resolvePromise() : rejectPromise(new Error(`build:web falhou (código ${code})`))))
    build.on('error', rejectPromise)
  })
}

if (!existsSync(join(webDir, 'index.html'))) {
  console.error('Sem dist/web/index.html. Rode `npm run build:web` (ou passe --build).')
  process.exit(1)
}

const binName = process.platform === 'win32' ? 'donnabot-host.exe' : 'donnabot-host'
const caminhosHost = [
  process.env.MEXERICA_HOST_BIN,
  join(app, `../../host/target/release/${binName}`),
  join(app, `../../host/target/debug/${binName}`),
].filter((c) => typeof c === 'string' && c.length > 0)
const hostBin = caminhosHost.find((c) => existsSync(c))
if (!hostBin) {
  console.error('Binário do Hub não encontrado. Compile com `cargo build --bin donnabot-host` na pasta host/ (ou defina MEXERICA_HOST_BIN).')
  process.exit(1)
}

const host = spawn(hostBin, ['serve', '--bind', '127.0.0.1', '--port', String(porta), '--web-dir', webDir], { stdio: ['ignore', 'inherit', 'pipe'] })
host.on('error', (error) => { console.error(String(error?.message ?? error)); process.exit(1) })

// O stderr segue aparecendo em tempo real, mas também fica guardado para diagnosticar
// uma saída imediata (binário sem suporte a --web-dir).
let textoStderr = ''
if (host.stderr) {
  host.stderr.setEncoding('utf8')
  host.stderr.on('data', (pedaco) => {
    textoStderr += pedaco
    process.stderr.write(pedaco)
  })
}

const endereco = `http://127.0.0.1:${porta}/`
let encerrando = false
// O código sai sincronamente antes do encerramento gentil: um encerro antecipado nunca
// transforma uma saída não-zero em 0.
const sair = (codigo) => { encerrando = true; process.exitCode = codigo; host.kill('SIGTERM'); setTimeout(() => process.exit(codigo), 150).unref() }
process.on('SIGINT', () => sair(0))
process.on('SIGTERM', () => sair(0))
host.on('exit', (code) => {
  if (encerrando) return
  // Últimos pedaços do stderr podem chegar logo após a saída; esperar um instante antes de decidir.
  setTimeout(() => {
    if (textoStderr.includes('--web-dir')) {
      console.error('Este binário recusou --web-dir: parece a build somente-socket-Unix (feature `open`).')
      console.error('A Web local exige o binário padrão `donnabot-host`, compilado com `cargo build --bin donnabot-host` na pasta host/ (sem a feature `open`).')
    }
    process.exit(code ?? 1)
  }, 250)
})

// Aguarda o Hub responder (health) antes de apontar o navegador para ele. Cada tentativa
// carrega um timeout próprio: uma conexão travada não pode estourar o prazo total de 60 s.
const limite = Date.now() + 60_000
for (;;) {
  try {
    const resposta = await fetch(`http://127.0.0.1:${porta}/health`, {
      signal: AbortSignal.timeout(Math.min(5000, Math.max(100, limite - Date.now()))),
    })
    if (resposta.ok) break
  } catch { /* ainda subindo */ }
  if (Date.now() > limite) {
    console.error('O Hub não respondeu em /health a tempo.')
    sair(1)
    break
  }
  await new Promise((esperar) => setTimeout(esperar, 250))
}

// Só o endereço e o caminho do arquivo de token: o conteúdo do token nunca é lido aqui.
console.log(endereco)
const homeMexerica = (process.env.MEXERICA_HOME ?? '').trim()
if (homeMexerica) {
  console.log(`Token local do Hub: ${join(homeMexerica, 'token')}`)
} else {
  console.log(`Token local do Hub: ${join(homedir(), '.mexerica/token')} (build dev: ${join(homedir(), '.mexerica-dev/token')})`)
}

if (!temFlag('--no-open')) {
  const abrir = process.platform === 'darwin' ? ['open', [endereco]]
    : process.platform === 'win32' ? ['cmd', ['/c', 'start', '', endereco]]
      : ['xdg-open', [endereco]]
  try { spawn(abrir[0], abrir[1], { stdio: 'ignore' }).on('error', () => {}) } catch { /* sem navegador: o endereço está acima */ }
}
