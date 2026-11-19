// #753: a função única que diz o que há em andamento na página (rascunho; anexo, upload ou envio só em memória).
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { ComposerDrafts } from './composer-drafts.ts'
import { haEstadoSoEmMemoria, haTrabalhoEmAndamento, pendenciaAtual, registrarEstadoEmMemoria } from './estado-so-em-memoria.ts'

const armazenamento = () => { const m = new Map<string, string>(); return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => { m.set(k, v) } } }

test('rascunho escrito conta como trabalho em andamento; anexo também é só em memória; vazio ou aposentado não conta', () => {
  assert.deepEqual(pendenciaAtual(), { rascunho: false, soEmMemoria: false })
  const r = new ComposerDrafts(armazenamento(), 'codync-drafts-v1-x')
  r.set('bot', '   ')
  assert.equal(haTrabalhoEmAndamento(), false, 'só espaços não é rascunho')
  r.set('bot', 'texto')
  assert.deepEqual(pendenciaAtual(), { rascunho: true, soEmMemoria: false })
  assert.equal(haEstadoSoEmMemoria(), false, 'o texto vai para o disco')
  r.setFiles('bot', [{ id: 'a', name: 'a.txt', data: new Uint8Array([1]) }])
  assert.deepEqual(pendenciaAtual(), { rascunho: true, soEmMemoria: true })
  r.set('bot', '')
  r.setFiles('bot', [])
  assert.equal(haTrabalhoEmAndamento(), false)
  r.set('bot', 'de novo')
  r.retire()
  assert.equal(haTrabalhoEmAndamento(), false, 'rascunho aposentado não conta')
})

test('outros donos registram e cancelam; consulta que falha conta como tudo (na dúvida, não recarregar)', () => {
  let upload = true
  const parar = registrarEstadoEmMemoria(() => ({ soEmMemoria: upload }))
  assert.deepEqual(pendenciaAtual(), { rascunho: false, soEmMemoria: true })
  upload = false
  assert.equal(haTrabalhoEmAndamento(), false)
  const pararFalha = registrarEstadoEmMemoria(() => { throw new Error('quebrou') })
  assert.deepEqual(pendenciaAtual(), { rascunho: true, soEmMemoria: true })
  pararFalha()
  parar()
  assert.equal(haTrabalhoEmAndamento(), false)
})

test('a loja registra upload em curso, arquivos guardados para Reenviar e mensagens em envio', () => {
  const fonte = readFileSync(new URL('../renderer/store/bot-store.ts', import.meta.url), 'utf8')
  assert.match(fonte, /registrarEstadoEmMemoria\(\(\) => this\.retired \? \{\} : \{\n\s*soEmMemoria: this\.uploadsEmCurso > 0 \|\| this\.outgoingFiles\.size > 0 \|\|\n\s*\[\.\.\.this\.entries\.values\(\)\]\.some\(lista => lista\.some\(e => e\.id\.startsWith\('local-'\) && e\.data\.status !== 'failed'\)\),/)
  assert.match(fonte, /override retire\(\) \{ this\.pararConsultaMemoria\(\);/)
})
