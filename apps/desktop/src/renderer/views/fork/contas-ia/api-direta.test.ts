import assert from 'node:assert/strict'
import { test } from 'node:test'
import { amostraDeModelos, chaveDoErro, enderecoPronto, mostrarApiDireta, salvarChave, salvarEndereco } from './api-direta.ts'
import { CATALOGOS } from '../../../lib/fork-i18n/index.ts'
import type { HostClient } from '../../../client/host-client'

test('API direta só aparece com o motor ligado e no Hub deste computador', () => {
  assert.equal(mostrarApiDireta([{ id: 'mexerica' }], true), true)
  assert.equal(mostrarApiDireta([{ id: 'mexerica' }], false), false)
  assert.equal(mostrarApiDireta([{ id: 'claude' }], true), false)
})

test('todo erro do teste tem frase nos seis idiomas, e erro desconhecido cai na falha do fornecedor', () => {
  for (const erro of ['chave_invalida', 'sem_permissao', 'limite', 'fornecedor_falhou', 'sem_resposta', 'resposta_invalida', 'sem_chave', 'sem_lista', 'destino_recusado']) {
    const chave = chaveDoErro(erro)
    for (const catalogo of Object.values(CATALOGOS)) assert.ok(catalogo[chave], `${erro} sem texto`)
  }
  assert.equal(chaveDoErro('qualquer_outro'), 'contas.apiDireta.erro.fornecedorFalhou')
})

test('a amostra de modelos mostra os primeiros e conta o resto', () => {
  const modelos = ['a', 'b', 'c', 'd', 'e', 'f'].map((id) => ({ id: `x/${id}`, name: id.toUpperCase() }))
  assert.deepEqual(amostraDeModelos(modelos), { nomes: 'A, B, C, D', restantes: 2 })
  assert.deepEqual(amostraDeModelos(modelos.slice(0, 2)), { nomes: 'A, B', restantes: 0 })
})

test('salvar limpa o campo antes de esperar o Hub e manda a chave sem espaços', async () => {
  const eventos: string[] = []
  let corpo: unknown
  const client = {
    call: async (metodo: string, body: unknown) => {
      eventos.push(`chamada:${metodo}`)
      corpo = body
      return { ok: true, modelos: [] }
    },
  } as unknown as HostClient
  const r = await salvarChave(client, 'openai', '  chave-ficticia-de-teste  ', () => eventos.push('campo limpo'))
  assert.deepEqual(eventos, ['campo limpo', 'chamada:motorChaveSalvar'])
  assert.deepEqual(corpo, { fornecedor: 'openai', chave: 'chave-ficticia-de-teste' })
  assert.equal(r.ok, true)
})

test('endereço próprio: https sempre; http só com a opção local; nome e chave obrigatórios', () => {
  const base = { nome: 'Serviço', url: 'https://api.example.invalid/v1', formato: 'responses' as const, local: false, chave: 'chave-ficticia-1', modelo: '' }
  assert.equal(enderecoPronto(base), true)
  assert.equal(enderecoPronto({ ...base, url: 'http://127.0.0.1:11434/v1' }), false)
  assert.equal(enderecoPronto({ ...base, url: 'http://127.0.0.1:11434/v1', local: true }), true)
  assert.equal(enderecoPronto({ ...base, url: 'não é endereço' }), false)
  assert.equal(enderecoPronto({ ...base, nome: ' ' }), false)
  assert.equal(enderecoPronto({ ...base, chave: '' }), false)
})

test('salvar endereço limpa a chave do campo antes de esperar o Hub', async () => {
  const eventos: string[] = []
  let corpo: Record<string, unknown> = {}
  const client = {
    call: async (metodo: string, body: Record<string, unknown>) => {
      eventos.push(`chamada:${metodo}`)
      corpo = { ...body }
      return { ok: true, modelos: [] }
    },
  } as unknown as HostClient
  await salvarEndereco(client, { nome: ' Serviço ', url: ' https://api.example.invalid/v1 ', formato: 'chat-completions', local: false, chave: ' chave-ficticia-2 ', modelo: '' }, () => eventos.push('campo limpo'))
  assert.deepEqual(eventos, ['campo limpo', 'chamada:motorEnderecoSalvar'])
  assert.deepEqual(corpo, { nome: 'Serviço', url: 'https://api.example.invalid/v1', formato: 'chat-completions', local: false, chave: 'chave-ficticia-2', modelo: undefined })
})
