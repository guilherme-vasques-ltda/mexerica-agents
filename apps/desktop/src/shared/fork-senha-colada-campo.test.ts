// MX-214: quarentena da colagem, vínculo ao editor e bloqueio. Os segredos são montados em tempo de teste.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { bloqueiaEnvio, devolverTrechos, editorDe, guardarEscrita, itensLivres, pendenciaVale, resolverNaoESenha, tratarColagem, type Pendencia } from './fork-senha-colada-campo.ts'

const chave = ['gh', 'p_', 'aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1dE3fG5'].join('')
const stripe = ['sk', '_live_', 'Qw3Er5Ty7Ui9Op1As3Df5Gh7'].join('')
const privada = [['-----BEGIN ', 'OPENSSH PRIVATE KEY', '-----'].join(''), 'aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1dE3fG5hJ7kL9mN1pQ3sT5vW7yZ9b', ['-----END ', 'OPENSSH PRIVATE KEY', '-----'].join('')].join('\n')

test('colar só a chave: o campo fica como estava e o trecho sai dele', () => {
  const r = tratarColagem('a senha é ', 10, 10, chave)!
  assert.equal(r.texto, 'a senha é ')
  assert.deepEqual(r.pedacos, [{ valor: chave, posicao: 10 }])
  assert.equal(r.cursor, 10)
  assert.equal(r.inserido, '')
})

test('P1: duas chaves na mesma colagem — NENHUMA entra no texto, no inserido nem em qualquer parte gravável', () => {
  const colado = `A=${stripe}\nB=${chave}\nfim`
  const r = tratarColagem('oi: ', 4, 4, colado)!
  for (const segredo of [stripe, chave]) {
    assert.ok(!r.texto.includes(segredo), 'texto do rascunho')
    assert.ok(!r.inserido.includes(segredo), 'o que seria marcado como colado')
  }
  assert.equal(r.texto, 'oi: A=\nB=\nfim')
  assert.deepEqual(r.pedacos.map((p) => p.valor), [stripe, chave])
})

test('P1: chave privada + token na mesma colagem — os dois ficam em quarentena', () => {
  const colado = `${privada}\nTOKEN=${chave}\nobrigado`
  const r = tratarColagem('', 0, 0, colado)!
  assert.equal(r.texto, '\nTOKEN=\nobrigado')
  assert.deepEqual(r.pedacos.map((p) => p.valor), [privada, chave])
  assert.ok(!r.texto.includes('PRIVATE KEY') && !r.texto.includes(chave))
})

test('a colagem substitui a seleção; colagem comum não é tratada', () => {
  assert.equal(tratarColagem('antes SELECAO depois', 6, 13, chave)!.texto, 'antes  depois')
  assert.equal(tratarColagem('', 0, 0, 'https://exemplo.com/pagina'), null)
})

test('"Não é senha" devolve TODOS os trechos, na ordem original (ida e volta idêntica)', () => {
  for (const [campo, ini, fim, colado] of [
    ['a senha é ', 10, 10, chave],
    ['oi: ', 4, 4, `use ${chave} agora`],
    ['antes SELECAO depois', 6, 13, `A=${stripe}\nB=${chave}\nfim`],
    ['', 0, 0, `${privada}\nTOKEN=${chave}\nobrigado`],
    ['x', 1, 1, `${chave}${stripe}`],
  ] as const) {
    const r = tratarColagem(campo, ini, fim, colado)!
    const volta = devolverTrechos(r.texto, r.pedacos)
    assert.equal(volta.texto, campo.slice(0, ini) + colado + campo.slice(fim))
    assert.equal(volta.texto.slice(volta.inicio, volta.fim).startsWith(r.pedacos[0]!.valor), true)
  }
})

test('devolver com o texto encolhido: vai para o fim, sem quebrar', () => {
  const r = tratarColagem('a senha é ', 10, 10, chave)!
  assert.equal(devolverTrechos('a', r.pedacos).texto, `a${chave}`)
})

test('P2: a pendência bloqueia o envio e só vale para o editor a que pertence', () => {
  assert.equal(bloqueiaEnvio(null), false)
  const pend: Pendencia = { editor: 'rascunho', pedacos: [{ valor: chave, posicao: 0 }] }
  assert.equal(bloqueiaEnvio(pend), true)
  assert.equal(pendenciaVale(pend, 'rascunho'), true)
  // Cancelou ou trocou a edição: a pendência não vale mais (o Composer a descarta; nunca restaura lá).
  assert.equal(pendenciaVale(pend, editorDe('m1')), false)
  const daFila: Pendencia = { editor: editorDe('m1'), pedacos: pend.pedacos }
  assert.equal(pendenciaVale(daFila, editorDe('m1')), true)
  assert.equal(pendenciaVale(daFila, editorDe('m2')), false)
  assert.equal(pendenciaVale(daFila, editorDe(null)), false)
  assert.equal(pendenciaVale(null, 'rascunho'), false)
})

test('P2: "Não é senha" depois de cancelar ou trocar a edição da fila descarta o trecho (não restaura no rascunho nem em outra linha)', () => {
  // Colou a chave editando a mensagem m1 da fila; o campo (da fila) ficou sem a chave.
  const r = tratarColagem('ajuste: ', 8, 8, chave)!
  const pend: Pendencia = { editor: editorDe('m1'), pedacos: r.pedacos }
  // Mesmo editor: devolve.
  const igual = resolverNaoESenha(pend, editorDe('m1'), r.texto)
  assert.equal(igual.acao === 'devolver' && igual.texto, `ajuste: ${chave}`)
  // Cancelou (voltou ao rascunho) ou abriu outra linha: descarta, sem texto para ninguém.
  assert.deepEqual(resolverNaoESenha(pend, 'rascunho', 'texto do rascunho'), { acao: 'descartar' })
  assert.deepEqual(resolverNaoESenha(pend, editorDe('m2'), 'outra mensagem'), { acao: 'descartar' })
  // E o contrário: pendência do rascunho não vai parar numa edição da fila.
  assert.deepEqual(resolverNaoESenha({ editor: 'rascunho', pedacos: r.pedacos }, editorDe('m1'), 'x'), { acao: 'descartar' })
})

test('P2: o campo congelado mantém as âncoras — inserir/remover antes da âncora não é possível; se mudar por fora, o resultado ainda é seguro', () => {
  const r = tratarColagem('abc def', 3, 3, `${chave}`)!
  // Campo encolheu por fora (ex.: rotina): âncora limitada ao fim, nada se perde nem se duplica.
  const volta = resolverNaoESenha({ editor: 'rascunho', pedacos: r.pedacos }, 'rascunho', 'a')
  assert.equal(volta.acao === 'devolver' && volta.texto, `a${chave}`)
})

test('P2 rodada 3: com a pendência aberta nenhum caminho altera o campo (comando, menção, ditado, rotina) e a restauração é exata', () => {
  // Um "campo" mínimo: o rascunho e a escrita guardada, como o Composer faz.
  let rascunho = '/'
  let pend: Pendencia | null = null
  const escrever = guardarEscrita(() => pend !== null, (texto: string) => { rascunho = texto })
  // Colou só a chave com "/" no campo: o texto continua "/", o menu de comandos continuaria aberto.
  const r = tratarColagem(rascunho, 1, 1, chave)!
  escrever(r.texto)
  pend = { editor: 'rascunho', pedacos: r.pedacos }
  assert.equal(rascunho, '/')
  // Menu e menções somem enquanto há pendência; sem ela, aparecem.
  assert.deepEqual(itensLivres(pend, ['fast', 'skill-x']), [])
  assert.deepEqual(itensLivres(null, ['fast', 'skill-x']), ['fast', 'skill-x'])
  // Escolher comando (Tab/clique), mencionar, ditado, rotina: todos escrevem pelo mesmo caminho e são recusados.
  for (const tentativa of ['/skill-x ', '', '@Byte ', '/ ditado dito', '/\nrotina']) escrever(tentativa)
  assert.equal(rascunho, '/', 'o campo não mudou')
  // "Não é senha": descongela, devolve exatamente a colagem original na posição original.
  const volta = resolverNaoESenha(pend, 'rascunho', rascunho)
  assert.equal(volta.acao, 'devolver')
  pend = null
  if (volta.acao === 'devolver') escrever(volta.texto)
  assert.equal(rascunho, `/${chave}`)
})
