import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { HostClient } from '../../../client/host-client.ts'
import { market } from '../../marketplace/market-models.ts'
import { lerRetornoLogin, RETORNO_MALFORMADO } from './retorno-login.ts'

// D70 (M3-B): só o repasse do `iss` e a recusa de retorno ambíguo. Sem Hub, conta ou provedor real.
const HUB = 'O retorno do login não confirmou o serviço esperado; login recusado. Inicie o login novamente.'

test('iss segue ao Hub decodificado uma vez, sem aparar nem normalizar', async () => {
  const volta = 'codync://oauth?state=s1&code=c1&iss=HTTPS%3A%2F%2FAuth.Exemplo%3A443%2Ft%2F%2520+'
  const retorno = lerRetornoLogin(volta)
  assert.deepEqual(retorno, { state: 's1', code: 'c1', error: null, iss: 'HTTPS://Auth.Exemplo:443/t/%20 ' })
  const chamadas: [string, unknown][] = []
  const client = { call: async (method: string, body: unknown) => (chamadas.push([method, body]), { connector: { id: 'x' } }) } as unknown as HostClient
  await market.finishConnectorSignIn(client, retorno)
  assert.deepEqual(chamadas, [['connectorSignInFinish', { state: 's1', code: 'c1', error: null, iss: 'HTTPS://Auth.Exemplo:443/t/%20 ' }]])
  // Sem iss: ausência explícita; o Hub decide pela política guardada na tentativa.
  assert.equal(lerRetornoLogin('codync://oauth?state=s2&error=access_denied').iss, null)
})

test('campo repetido ou retorno inválido recusa em vez de escolher o primeiro valor', () => {
  for (const volta of [
    'codync://oauth?state=s&code=c&iss=https%3A%2F%2Fa&iss=https%3A%2F%2Fb',
    'codync://oauth?state=s&state=t&code=c',
    'codync://oauth?state=s&code=c&code=d',
    'codync://oauth?state=s&error=a&error=b',
    'codync://oauth?state=s&error_description=a&error_description=b',
    'não é url',
  ]) {
    assert.throws(() => lerRetornoLogin(volta), { message: RETORNO_MALFORMADO }, volta)
  }
})

test('recusa do Hub chega intacta ao chamador da tela', async () => {
  const client = { call: async () => { throw new Error(HUB) } } as unknown as HostClient
  await assert.rejects(market.finishConnectorSignIn(client, lerRetornoLogin('codync://oauth?state=s&code=c')), { message: HUB })
})
