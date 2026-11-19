import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ehComputerId, jsonEstrito, TETO_DADO, validarDado } from './esquemas.ts'
import { classificarEstado812 } from './sondagem.ts'
import { AnalisadorQuadros, analisarSaida, LIMITE_TOTAL, quadroDado, quadroEvento } from './quadros.ts'

// Exemplos fictícios: nenhum servidor, IP, nome ou segredo reais.
const ID_REAL = 'q3VhV0n1xYz8bK2mPdR4tA'
const HASH = 'ab'.repeat(32)
const SAUDE = JSON.stringify({ name: 'servidor-teste', computerId: ID_REAL, signKey: 'chave-publica-ficticia', boxKey: null, version: '0.1.0', port: 19422, token: 'token_ficticio-123', running: true })
const analisar = (s: string) => analisarSaida(Buffer.from(s, 'latin1'))

test('computerId no formato real do Hub: 22 caracteres base64url, último em A/Q/g/w', () => {
  assert.equal(ehComputerId(ID_REAL), true)
  for (const fim of ['A', 'Q', 'g', 'w']) assert.equal(ehComputerId('q3VhV0n1xYz8bK2mPdR4t' + fim), true, fim)
  assert.equal(ehComputerId('q3VhV0n1xYz8bK2mPdR4tB'), false, 'último caractere fora de A/Q/g/w')
  assert.equal(ehComputerId('q3VhV0n1xYz8bK2mPdR4t'), false, '21 caracteres')
  assert.equal(ehComputerId('q3VhV0n1xYz8bK2mPdR4tAA'), false, '23 caracteres')
  assert.equal(ehComputerId('q3VhV0n1xYz8bK2mPdR4t='), false, 'padding')
  assert.equal(ehComputerId('q3VhV0n1xYz8bK2mPdR4t+'), false)
  assert.equal(ehComputerId('q3VhV0n1xYz8bK2mPdR4t/'), false)
  assert.equal(ehComputerId('123e4567-e89b-12d3-a456-426614174000'), false, 'UUID com hífens')
  assert.equal(ehComputerId(undefined), false)
})

test('quadro válido: eventos e mensagens de dados, na ordem', () => {
  const r = analisar(
    quadroEvento('so', 'ok') + quadroEvento('linger', 'pulado') + quadroDado('uid', '1000 -') + quadroDado('arq', 'x86_64')
    + quadroDado('host', `${ID_REAL} 1.2.3`) + quadroDado('estado812', 'limpo-comprovado') + quadroDado('tam', '0') + quadroDado('hashparcial', HASH)
    + quadroDado('total', `90 ${HASH}`) + quadroDado('saude', SAUDE),
  )
  assert.equal(r.falha, null)
  assert.deepEqual(r.eventos, [{ codigo: 'so', estado: 'ok' }, { codigo: 'linger', estado: 'pulado' }])
  assert.equal(r.descartadas, 0)
  assert.deepEqual(r.dados.map((d) => d.tipo), ['uid', 'arq', 'host', 'estado812', 'tam', 'hashparcial', 'total', 'saude'])
  const saude = r.dados.find((d) => d.tipo === 'saude')
  assert.ok(saude && saude.tipo === 'saude')
  assert.equal(JSON.stringify(saude).includes('token'), false, 'o token nunca sai do analisador')
})

test('outras linhas são descartadas e só contadas; nada delas aparece no resultado', () => {
  const r = analisar(
    'ls: erro qualquer\n\x1b[31mvermelho\x1b[0m\n' + quadroEvento('sudo', 'ok') + 'MEX1 E codigoinventado ok\nMEX1 E so talvez\nMEX1 E so ok \nMEX1 E so ok\r\n'
    + 'MEX1 FIM tam\n' + 'mex1 e so ok\n' + quadroEvento('relogio', 'falha'),
  )
  assert.equal(r.falha, null)
  assert.deepEqual(r.eventos, [{ codigo: 'sudo', estado: 'ok' }, { codigo: 'relogio', estado: 'falha' }])
  assert.equal(r.descartadas, 8)
  assert.equal(JSON.stringify(r).includes('vermelho'), false)
})

test('conteúdo de dados com linhas parecidas com eventos não vira evento (o tamanho manda)', () => {
  const r = analisar(quadroDado('saude', SAUDE.replace('servidor-teste', 'MEX1 E so ok')))
  assert.equal(r.falha, null)
  assert.equal(r.eventos.length, 0)
  assert.equal(r.dados.length, 1)
})

test('cabeçalho de dados inválido falha a fase inteira', () => {
  for (const cab of ['MEX1 D tam', 'MEX1 D tam x', 'MEX1 D desconhecido 3', 'MEX1 D tam 007', 'MEX1 D tam -1', 'MEX1 D  tam 3', 'MEX1 D tam 3 ']) {
    const r = analisar(quadroEvento('so', 'ok') + cab + '\n123\nMEX1 FIM tam\n')
    assert.equal(r.falha, 'cabecalho-invalido', cab)
    assert.deepEqual(r.eventos, [], 'eventos lidos antes não valem')
  }
})

test('teto por tipo: n acima do teto (ou zero) falha sem esperar os bytes', () => {
  assert.equal(analisar('MEX1 D tam 0\n\nMEX1 FIM tam\n').falha, 'teto-excedido')
  for (const [tipo, teto] of Object.entries(TETO_DADO)) {
    const a = new AnalisadorQuadros()
    a.empurrar(Buffer.from(`MEX1 D ${tipo} ${teto + 1}\n`))
    assert.equal(a.falha, 'teto-excedido', tipo)
  }
})

test('FIM ausente, errado ou de outro tipo; n menor ou maior que o conteúdo', () => {
  assert.equal(analisar('MEX1 D tam 3\n123\n').falha, 'saida-truncada')
  assert.equal(analisar('MEX1 D tam 3\n123\nMEX1 FIM hashparcial\n').falha, 'fim-ausente')
  assert.equal(analisar('MEX1 D tam 3\n123\nMEX1 FIM tam').falha, 'saida-truncada')
  assert.equal(analisar('MEX1 D tam 2\n123\nMEX1 FIM tam\n').falha, 'fim-ausente', 'n menor')
  assert.equal(analisar('MEX1 D tam 4\n123\nMEX1 FIM tam\n').falha, 'fim-ausente', 'n maior')
  assert.equal(analisar('MEX1 D tam 3\n123MEX1 FIM tam\n').falha, 'fim-ausente', 'sem o LF depois do conteúdo')
})

test('gramática de cada tipo: valores fora do formato falham a fase', () => {
  const ruins: [string, string][] = [
    ['tam', '007'], ['tam', '-1'], ['tam', '1e3'], ['tam', ' 1'],
    ['hashparcial', 'AB'.repeat(32)], ['hashparcial', 'ab'.repeat(31)], ['hashparcial', 'zz'.repeat(32)],
    ['uid', '1000'], ['uid', '1000 1001 1002'], ['uid', '01000 -'], ['uid', '4294967295 -'], ['uid', 'a -'], ['uid', '1000 --'],
    ['arq', 'x86'], ['arq', 'arm64'], ['arq', 'X86_64'],
    ['host', `${ID_REAL}x 1.2.3`], ['host', `${ID_REAL} 1.2`], ['host', `${ID_REAL} 01.2.3`], ['host', 'q3VhV0n1xYz8bK2mPdR4tB 1.2.3'], ['host', '123e4567-e89b-12d3-a456-426614174000 1.2.3'],
    ['estado812', 'limpo'], ['estado812', 'LIMPO-COMPROVADO'], ['estado812', 'incompleto '],
    ['total', `5 ${HASH} x`], ['total', `-5 ${HASH}`], ['total', '5 abc'],
    ['saude', '[]'], ['saude', 'nao e json'],
  ]
  for (const [tipo, conteudo] of ruins) {
    const r = analisar(quadroDado(tipo as 'tam', conteudo))
    assert.equal(r.falha, 'gramatica-invalida', `${tipo}: ${conteudo}`)
    assert.deepEqual(r.dados, [])
  }
})

test('saude: JSON estrito (chave repetida, chave desconhecida, profundidade, tipos, computerId inválido)', () => {
  const base = JSON.parse(SAUDE) as Record<string, unknown>
  assert.ok(validarDado('saude', SAUDE))
  assert.equal(validarDado('saude', SAUDE.replace('"running":true', '"running":true,"running":false')), null, 'chave repetida')
  assert.equal(validarDado('saude', JSON.stringify({ ...base, extra: 1 })), null, 'chave desconhecida')
  assert.equal(validarDado('saude', JSON.stringify({ ...base, name: { a: { b: { c: { d: 1 } } } } })), null, 'profundidade')
  assert.equal(validarDado('saude', JSON.stringify({ ...base, computerId: '123e4567-e89b-12d3-a456-426614174000' })), null)
  assert.equal(validarDado('saude', JSON.stringify({ ...base, port: '19422' })), null)
  assert.equal(validarDado('saude', JSON.stringify({ ...base, port: 70000 })), null)
  assert.equal(validarDado('saude', JSON.stringify({ ...base, token: 'tem espaço' })), null)
  assert.equal(validarDado('saude', SAUDE + ' lixo'), null)
  assert.equal(validarDado('saude', JSON.stringify({ ...base, version: '1.2' })), null)
  assert.throws(() => jsonEstrito('{"a":1,"a":2}'))
  assert.throws(() => jsonEstrito('[[[[[1]]]]]'))
  assert.deepEqual(jsonEstrito('[[[[1]]]]'), [[[[1]]]])
})

test('limite total de 64 KiB por processo', () => {
  const a = new AnalisadorQuadros()
  const linha = Buffer.from('x'.repeat(100) + '\n')
  let falhou = false
  for (let i = 0; i < 1000 && !falhou; i++) {
    a.empurrar(linha)
    falhou = a.falha !== null
  }
  assert.equal(a.falha, 'estouro-total')
  assert.ok(LIMITE_TOTAL === 65536)
  const r = a.finalizar()
  assert.deepEqual([r.eventos, r.dados], [[], []])
})

test('linha enorme sem LF é descartada sem crescer a memória e a saída seguinte continua válida', () => {
  const a = new AnalisadorQuadros()
  a.empurrar(Buffer.from('y'.repeat(5000)))
  a.empurrar(Buffer.from('y'.repeat(5000) + '\n' + quadroEvento('so', 'ok')))
  const r = a.finalizar()
  assert.equal(r.falha, null)
  assert.deepEqual(r.eventos, [{ codigo: 'so', estado: 'ok' }])
  assert.equal(r.descartadas, 1)
})

test('em pedaços de 1 byte o resultado é o mesmo que de uma vez', () => {
  const texto = quadroEvento('so', 'ok') + 'lixo\n' + quadroDado('uid', '1000 998') + quadroDado('saude', SAUDE) + quadroEvento('systemd', 'falha')
  const inteiro = analisar(texto)
  const a = new AnalisadorQuadros()
  for (const b of Buffer.from(texto, 'latin1')) a.empurrar(Buffer.from([b]))
  assert.deepEqual(a.finalizar(), inteiro)
})

test('fuzz: bytes aleatórios nunca lançam e só produzem eventos e dados bem formados', () => {
  let semente = 12345
  const rnd = () => { semente = (semente * 1103515245 + 12345) & 0x7fffffff; return semente }
  const pecas = ['MEX1 E ', 'MEX1 D ', 'MEX1 FIM ', 'so', 'tam', 'uid', 'ok', 'falha', ' ', '\n', '\r', '1', '0', '99', '-', 'x86_64', '\u0000', '\xff', ID_REAL]
  for (let i = 0; i < 3000; i++) {
    let s = ''
    for (let j = rnd() % 14; j >= 0; j--) s += pecas[rnd() % pecas.length]
    const r = analisar(s)
    for (const e of r.eventos) assert.match(`MEX1 E ${e.codigo} ${e.estado}`, /^MEX1 E [a-z0-9]+ (ok|pulado|falha)$/)
    if (r.falha) assert.deepEqual([r.eventos, r.dados], [[], []])
  }
})

test('cabeçalho de dados truncado (sem LF) ou acima de 256 bytes FALHA a fase; o que veio antes não vale', () => {
  const limpo = quadroDado('estado812', 'limpo-comprovado')
  for (const resto of ['MEX1 D estado812 10', 'MEX1 D', 'MEX1 D estado812 ', 'MEX1 D ' + 'x'.repeat(300), 'MEX1 D ' + 'x'.repeat(300) + '\n']) {
    const inteiro = analisar(limpo + resto)
    assert.equal(inteiro.falha, 'cabecalho-invalido', JSON.stringify(resto.slice(0, 20)))
    assert.deepEqual([inteiro.eventos, inteiro.dados], [[], []], 'o quadro limpo anterior não é conservado')
    assert.equal(classificarEstado812(inteiro), 'ilegivel')
    // Fragmentado byte a byte: o mesmo resultado.
    const a = new AnalisadorQuadros()
    for (const b of Buffer.from(limpo + resto, 'latin1')) a.empurrar(Buffer.from([b]))
    const r = a.finalizar()
    assert.equal(r.falha, 'cabecalho-invalido', 'fragmentado')
    assert.deepEqual(r.dados, [])
  }
  // Linha longa que NÃO é cabeçalho de dados continua só descartada.
  assert.equal(analisar(limpo + 'MEX1 E ' + 'x'.repeat(300)).falha, null)
  assert.equal(analisar(limpo + 'texto qualquer sem LF').falha, null)
})

test('prefixo de dados unificado: "MEX1 D" com ou sem espaço final, com tab, truncado ou estourado FALHA a fase, inclusive fragmentado', () => {
  const limpo = quadroDado('estado812', 'limpo-comprovado')
  for (const resto of ['MEX1 D\n', 'MEX1 D\tuid 3\n', 'MEX1 D \n', 'MEX1 Dx\n', 'MEX1 D', 'MEX1 D\tuid 3', 'MEX1 D\n'.repeat(2), 'MEX1 D\t' + 'x'.repeat(300), 'MEX1 D\t' + 'x'.repeat(300) + '\n']) {
    const inteiro = analisar(limpo + resto)
    assert.equal(inteiro.falha, 'cabecalho-invalido', JSON.stringify(resto.slice(0, 20)))
    assert.deepEqual([inteiro.eventos, inteiro.dados], [[], []], 'o quadro limpo anterior não é conservado')
    assert.equal(classificarEstado812(inteiro), 'ilegivel')
    const a = new AnalisadorQuadros()
    for (const b of Buffer.from(limpo + resto, 'latin1')) a.empurrar(Buffer.from([b]))
    const r = a.finalizar()
    assert.equal(r.falha, 'cabecalho-invalido', 'fragmentado ' + JSON.stringify(resto.slice(0, 20)))
    assert.deepEqual(r.dados, [])
  }
  // Linhas parecidas que NÃO começam com o prefixo continuam só descartadas.
  assert.equal(analisar(limpo + 'MEX1 d\n' + 'MEX1  D\n' + 'xMEX1 D\n').falha, null)
})

test('saude em UTF-8 estrito: o tamanho conta bytes (nome acentuado) e byte inválido falha', () => {
  const acentuada = SAUDE.replace('servidor-teste', 'servidor-é-日本')
  const r = analisarSaida(Buffer.from(quadroDado('saude', acentuada), 'utf8'))
  assert.equal(r.falha, null)
  const saude = r.dados[0]
  assert.ok(saude && saude.tipo === 'saude' && saude.saude.name === 'servidor-é-日本')
  const ruim = Buffer.concat([Buffer.from('MEX1 D arq 3\n'), Buffer.from([0xff, 0xfe, 0x41]), Buffer.from('\nMEX1 FIM arq\n')])
  assert.equal(analisarSaida(ruim).falha, 'gramatica-invalida')
  assert.deepEqual(validarDado('so', 'ubuntu 24.04'), { tipo: 'so', id: 'ubuntu', versao: '24.04' })
  for (const ruimSo of ['arch 1', 'debian', 'debian 13 x', 'debian 1x', 'debian  13']) assert.equal(validarDado('so', ruimSo), null, ruimSo)
})
