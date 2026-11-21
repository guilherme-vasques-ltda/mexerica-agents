import assert from 'node:assert/strict'
import { test } from 'node:test'
import { salvarPlano } from './planos-codigo.ts'
import type { HostClient } from '../../../client/host-client'

test('salvar o plano limpa o campo antes de esperar o Hub e manda a chave sem espaços', async () => {
  const eventos: string[] = []
  let corpo: unknown
  const client = {
    call: async (metodo: string, body: unknown) => {
      eventos.push(`chamada:${metodo}`)
      corpo = body
      return { ok: true, estado: 'modelos_no_catalogo', modelos: [] }
    },
  } as unknown as HostClient
  const r = await salvarPlano(client, 'opencode-go', '  chave-ficticia-de-teste  ', () => eventos.push('campo limpo'))
  assert.deepEqual(eventos, ['campo limpo', 'chamada:planoCodigoSalvar'])
  assert.deepEqual(corpo, { plano: 'opencode-go', chave: 'chave-ficticia-de-teste' })
  assert.equal(r.ok, true)
})
