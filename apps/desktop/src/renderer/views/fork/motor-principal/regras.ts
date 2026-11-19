// DON-865, fatia 4 (D372/D373/D375): "Usar também na conversa principal", na seção Motor do editor do bot. Mudar o
// padrão do bot só vale para conversas novas; este botão leva o padrão à principal pelo MESMO pedido da Conversa (C9):
// `sessionEngineSet` com `sessionId: null`. Só existe com o anúncio `engine` do Hub; sem ele, nada (nunca `updateBot`).
import { engineDaSessao, erroDaTroca, precisaConfirmar, type Escolha } from '../seletor-modelo/regras.ts'

/** A projeção `engine` da principal, como a C9 a lê. */
export type EngineDaPrincipal = NonNullable<ReturnType<typeof engineDaSessao>>

/** `oculto`: sem anúncio ou sem `engine` legível; `igual`: a principal já usa o padrão; `pronto`: o botão age. */
export type EstadoDoBotao = 'oculto' | 'igual' | 'pronto'

const mesmasOpcoes = (a: Record<string, string>, b: Record<string, string>) => {
  const ka = Object.keys(a)
  return ka.length === Object.keys(b).length && ka.every((k) => a[k] === b[k])
}

export function estadoDoBotao(anunciado: boolean, principal: EngineDaPrincipal | null, padrao: Escolha): EstadoDoBotao {
  if (!anunciado || !principal) return 'oculto'
  const igual = principal.backend === padrao.backend && principal.model === padrao.model && principal.fastMode === padrao.fastMode
    && (principal.accountId ?? null) === (padrao.accountId ?? null) && mesmasOpcoes(principal.modelOptions, padrao.modelOptions)
  return igual ? 'igual' : 'pronto'
}

/** O pedido, idêntico ao do chip da C9: principal = `sessionId: null`, CAS pela revisão lida, `fastMode` booleano. */
export function pedidoNaPrincipal(botId: string, principal: EngineDaPrincipal, padrao: Escolha) {
  return {
    botId, sessionId: null, expectedRevision: principal.revision,
    engine: { accountId: padrao.accountId ?? null, backend: padrao.backend, model: padrao.model, modelOptions: { ...padrao.modelOptions }, fastMode: padrao.fastMode },
  }
}

/** Pede a confirmação D375 nas mesmas condições da C9 (mesmo provedor, outro modelo, 50 mil tokens ou mais). */
export const confirmarNaPrincipal = (principal: EngineDaPrincipal, padrao: Escolha, usado: number | null) => precisaConfirmar(principal, padrao, usado)

/** Grava pela única via (`sessionEngineSet`). Sem anúncio, recusa antes de chamar o Hub: não há caminho alternativo. */
export async function usarNaPrincipal(client: { call: (method: string, body: unknown) => Promise<unknown> }, anunciado: boolean, botId: string, principal: EngineDaPrincipal, padrao: Escolha) {
  if (!anunciado) throw new Error('engine_unavailable')
  await client.call('sessionEngineSet', pedidoNaPrincipal(botId, principal, padrao))
}

/** O texto da nota: o erro é a CHAVE do catálogo devolvida por `erroDaTroca` (C9), traduzida aqui; sem erro, "Pronto". */
export function textoDaNota(t: (chave: ReturnType<typeof erroDaTroca> | 'botSettings.motor.pronto') => string, erro: ReturnType<typeof erroDaTroca> | null): string {
  return t(erro ?? 'botSettings.motor.pronto')
}
