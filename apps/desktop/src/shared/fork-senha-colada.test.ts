// MX-214: o detector de senha colada. Todo exemplo positivo é FICTÍCIO e montado aqui, em tempo de teste, a partir
// de um gerador com semente: nenhum segredo (real ou parecido com real) fica escrito no arquivo, para o Gitleaks
// não reclamar e para ninguém confundir o teste com uma chave de verdade.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { acharSegredoColado, acharSegredosColados, entropia } from './fork-senha-colada.ts'

function gerador(semente: number) {
  let s = semente >>> 0
  return (alfabeto: string, n: number) => {
    let r = ''
    for (let i = 0; i < n; i++) {
      s = (Math.imul(s, 1664525) + 1013904223) >>> 0
      r += alfabeto[(s >>> 8) % alfabeto.length]
    }
    return r
  }
}
const sorteio = gerador(214)
const MIN = 'abcdefghijklmnopqrstuvwxyz'
const MAI = MIN.toUpperCase()
const NUM = '0123456789'
const ALNUM = MIN + MAI + NUM
const URLSAFE = ALNUM + '-_'
/** Garante, para cada classe que o alfabeto tem (minúscula, maiúscula, dígito), ao menos um caractere dela. */
const aleatorio = (alfabeto: string, n: number) => {
  const exigidas = [/[a-z]/, /[A-Z]/, /[0-9]/].filter((r) => r.test(alfabeto))
  for (;;) {
    const v = sorteio(alfabeto, n)
    if (exigidas.every((r) => r.test(v))) return v
  }
}

const monta = (...partes: string[]) => partes.join('')
const achou = (t: string) => acharSegredoColado(t)
const trecho = (t: string) => { const r = achou(t); return r ? t.slice(r.inicio, r.fim) : null }

test('prefixos conhecidos são detectados e o trecho cobre só a chave', () => {
  const casos: [string, string][] = [
    ['sk-', aleatorio(URLSAFE, 48)],
    ['sk-ant-api03-', aleatorio(URLSAFE, 60)],
    ['ghp_', aleatorio(ALNUM, 36)],
    ['gho_', aleatorio(ALNUM, 36)],
    ['github_pat_', aleatorio(ALNUM + '_', 40)],
    ['xoxb-', `${aleatorio(NUM, 11)}-${aleatorio(ALNUM, 24)}`],
    ['xoxp-', `${aleatorio(NUM, 11)}-${aleatorio(ALNUM, 24)}`],
    ['AKIA', aleatorio(MAI + NUM, 16)],
    ['AIza', aleatorio(URLSAFE, 35)],
    ['glpat-', aleatorio(URLSAFE, 20)],
  ]
  for (const [prefixo, corpo] of casos) {
    const chave = monta(prefixo, corpo)
    assert.equal(trecho(chave), chave, prefixo)
    assert.equal(trecho(`minha chave é ${chave} ok?`), chave, `no meio: ${prefixo}`)
    assert.equal(trecho(`TOKEN="${chave}"`), chave, `entre aspas: ${prefixo}`)
    assert.equal(achou(chave)?.tipo, 'prefixo-conhecido')
  }
})

test('JWT (eyJ…) é detectado', () => {
  const jwt = monta('eyJ', aleatorio(URLSAFE, 20), '.', 'eyJ', aleatorio(URLSAFE, 40), '.', aleatorio(URLSAFE, 43))
  assert.equal(trecho(jwt), jwt)
  assert.equal(trecho(`Authorization: Bearer ${jwt}`), jwt)
})

test('chave privada: bloco inteiro, mesmo em várias linhas, e pedaço sem o fim', () => {
  const corpo = Array.from({ length: 4 }, () => aleatorio(ALNUM + '+/', 64)).join('\n')
  const comeco = monta('-----BEGIN ', 'OPENSSH PRIVATE KEY', '-----')
  const fim = monta('-----END ', 'OPENSSH PRIVATE KEY', '-----')
  const bloco = `${comeco}\n${corpo}\n${fim}`
  assert.equal(trecho(`olha:\n${bloco}\nobrigado`), bloco)
  assert.equal(achou(bloco)?.tipo, 'chave-privada')
  const rsa = monta('-----BEGIN ', 'RSA PRIVATE KEY', '-----', '\n', corpo)
  assert.equal(trecho(rsa), rsa)
  // Certificado público não é segredo.
  assert.equal(achou(monta('-----BEGIN ', 'CERTIFICATE', '-----\n', corpo, '\n-----END ', 'CERTIFICATE', '-----')), null)
})

test('trecho isolado, longo, de alta entropia e com mistura de classes', () => {
  const senha = aleatorio(ALNUM + '!@#$%&*', 24)
  assert.equal(trecho(senha), senha)
  assert.equal(achou(senha)?.tipo, 'alta-entropia')
  const token = aleatorio(ALNUM, 32)
  assert.equal(trecho(`  ${token}\n`), token)
  assert.equal(trecho(`API_KEY=${token}`), token)
  assert.equal(trecho(`"${token}"`), token)
  const base64 = aleatorio(ALNUM + '+/', 40) + '=='
  assert.equal(trecho(base64), base64)
})

test('sem falso positivo: URL, e-mail, caminho, hash, telefone, palavras e identificadores', () => {
  const hex = (n: number) => sorteio('0123456789abcdef', n)
  const comuns = [
    'https://github.com/example-org/mexerica/pull/730',
    'https://exemplo.com.br/painel/relatorios?mes=11&ano=2026#resumo',
    'www.exemplo.com.br/contato',
    'financeiro@exemplo.com',
    'maria.souza+notas@empresa-exemplo.com.br',
    '/tmp/exemplo/projetos/mexerica/apps/desktop/src/renderer/views/thread/Composer.tsx',
    'apps/desktop/src/shared/composer-segments.ts',
    '~/Documentos/Relatório Final.pdf'.replace(' ', '-'),
    'C:\\exemplo\\documentos\\planilha-final.xlsx',
    './node_modules/.bin/tsc',
    hex(7), hex(12), hex(40), hex(64),
    '123e4567-e89b-12d3-a456-426614174000',
    '+5511987654321',
    '(11) 98765-4321',
    '11987654321',
    'paralelepípedo',
    'anticonstitucionalissimamente',
    'useSyncExternalStore',
    'getUserByIdAndEmail2',
    'get_user_by_id_v2_final',
    'sk-learn-algoritmo-de-agrupamento-simples',
    'gh-pages-publicacao-automatica-do-site',
    'task-management-system-with-long-name',
    'Preciso que você revise o relatório de novembro e me mande até sexta-feira.',
    'oi',
    '',
    '   ',
  ]
  for (const t of comuns) assert.equal(achou(t), null, t)
})

test('sem falso positivo: código colado com várias linhas', () => {
  const codigo = [
    "import { useState } from 'react'",
    '',
    'export function Contador({ inicial }: { inicial: number }) {',
    '  const [valor, setValor] = useState(inicial)',
    "  const rotulo = `Valor atual: ${valor} de ${inicial}`",
    '  return <button onClick={() => setValor((v) => v + 1)}>{rotulo}</button>',
    '}',
  ].join('\n')
  assert.equal(achou(codigo), null)
  const cola = aleatorio(ALNUM, 32)
  // Várias linhas sem chave privada nem prefixo conhecido: a entropia só vale para um trecho isolado.
  assert.equal(achou(`linha 1\n${cola}\nlinha 3`), null)
  assert.equal(achou(`${cola}\n${cola}`), null)
})

test('prefixo conhecido ainda vale dentro de um arquivo .env colado', () => {
  const chave = monta('ghp_', aleatorio(ALNUM, 36))
  const env = `PORTA=3000\nTOKEN=${chave}\nDEBUG=false`
  assert.equal(trecho(env), chave)
})

test('o detector não guarda estado entre chamadas', () => {
  const chave = monta('sk-', aleatorio(URLSAFE, 48))
  for (let i = 0; i < 3; i++) assert.equal(trecho(chave), chave)
  assert.equal(achou('texto comum'), null)
  assert.equal(trecho(chave), chave)
})

test('entropia: repetição é baixa, sorteio é alto', () => {
  assert.equal(entropia(''), 0)
  assert.ok(entropia('aaaaaaaaaaaaaaaaaaaa') < 1)
  assert.ok(entropia(aleatorio(ALNUM, 32)) > 4)
})

test('MX-214 P2: provedores comuns (Stripe, Slack, npm, PyPI, Hugging Face, SendGrid, Shopify, GitHub…) sozinhos e em .env', () => {
  const hex = (n: number) => sorteio('0123456789abcdef', n)
  const casos: [string, string][] = [
    [monta('sk', '_live_'), aleatorio(ALNUM, 24)],
    [monta('sk', '_test_'), aleatorio(ALNUM, 24)],
    [monta('rk', '_live_'), aleatorio(ALNUM, 24)],
    [monta('whsec', '_'), aleatorio(ALNUM, 32)],
    ['xoxa-', `${aleatorio(NUM, 11)}-${aleatorio(ALNUM, 24)}`],
    ['xapp-', `1-${aleatorio(MAI + NUM, 11)}-${aleatorio(NUM, 13)}-${hex(40)}`],
    ['ASIA', aleatorio(MAI + NUM, 16)],
    ['ghs_', aleatorio(ALNUM, 36)],
    ['ghu_', aleatorio(ALNUM, 36)],
    ['ghr_', aleatorio(ALNUM, 36)],
    [monta('npm', '_'), aleatorio(ALNUM, 36)],
    [monta('pypi', '-'), aleatorio(URLSAFE, 70)],
    [monta('hf', '_'), aleatorio(ALNUM, 34)],
    [monta('SG', '.'), `${aleatorio(URLSAFE, 22)}.${aleatorio(URLSAFE, 43)}`],
    [monta('shp', 'at_'), hex(32)],
    [monta('ya29', '.'), aleatorio(URLSAFE, 60)],
  ]
  for (const [prefixo, corpo] of casos) {
    const chave = monta(prefixo, corpo)
    assert.equal(trecho(chave), chave, prefixo)
    assert.equal(trecho(`CHAVE=${chave}`), chave, `atribuição: ${prefixo}`)
    const env = `PORTA=3000\nNOME=loja exemplo\nCHAVE=${chave}\nDEBUG=false\n`
    assert.equal(trecho(env), chave, `.env: ${prefixo}`)
  }
})

test('MX-214 P1: acharSegredosColados devolve TODOS os trechos (duas chaves, chave privada + token), em ordem', () => {
  const a = monta('sk', '_live_', aleatorio(ALNUM, 24))
  const b = monta('ghp_', aleatorio(ALNUM, 36))
  const duas = `A=${a}\nB=${b}\n`
  assert.deepEqual(acharSegredosColados(duas).map((r) => duas.slice(r.inicio, r.fim)), [a, b])
  const corpo = Array.from({ length: 3 }, () => aleatorio(ALNUM + '+/', 64)).join('\n')
  const bloco = `${monta('-----BEGIN ', 'OPENSSH PRIVATE KEY', '-----')}\n${corpo}\n${monta('-----END ', 'OPENSSH PRIVATE KEY', '-----')}`
  const misto = `${bloco}\nTOKEN=${b}\nfim`
  assert.deepEqual(acharSegredosColados(misto).map((r) => misto.slice(r.inicio, r.fim)), [bloco, b])
  // Dois blocos de chave privada.
  const dois = `${bloco}\n${bloco}`
  assert.equal(acharSegredosColados(dois).length, 2)
  assert.deepEqual(acharSegredosColados('texto comum sem nada'), [])
})

test('MX-214 P2: .env e código de várias linhas sem chave de provedor não disparam (sem entropia em texto inteiro)', () => {
  const env = ['PORTA=3000', 'NOME=loja exemplo', `HASH=${sorteio('0123456789abcdef', 40)}`, `SENHA=${aleatorio(ALNUM, 24)}`].join('\n')
  assert.equal(achou(env), null)
})
