import { createContext, useCallback, useContext, useEffect, useState, type Dispatch, type SetStateAction } from 'react'

/**
 * O que a tela de Ajustes guarda enquanto está aberta: rascunhos e escolhas que a página desmontada perderia.
 * O SettingsView é o dono do mapa; fechar Ajustes o descarta.
 *
 * A chave diz de quem é o estado: inclua o computador/Hub (e o Space, quando há) em toda chave, senão um rascunho
 * de um Hub aparece no outro. Quando a chave muda enquanto o componente vive, o hook devolve o estado da chave nova.
 *
 * Exceções de propósito (segredos): a chave digitada na Voz (VozPage, ChaveDoAparelho), a chave de API de
 * Contas de IA (ChaveDeApi) e o código de conexão (ConectarConta) não são guardados aqui. O renderer só tem a
 * chave durante a digitação no próprio campo, e a regra de segredos do repositório não admite mais que isso.
 */
export type EstadoGuardado = Map<string, unknown>

export const EstadoPaginas = createContext<EstadoGuardado | null>(null)

/** Um valor com a chave de que ele é. */
export interface Estado<T> { readonly chave: string; readonly valor: T }

/** O estado de `chave`: o guardado no pai, ou o inicial. Pura: é o que o hook usa ao abrir e ao trocar de chave. */
export function abrirEstado<T>(guardado: EstadoGuardado | null, chave: string, inicial: T | (() => T)): Estado<T> {
  if (guardado?.has(chave)) return { chave, valor: guardado.get(chave) as T }
  return { chave, valor: typeof inicial === 'function' ? (inicial as () => T)() : inicial }
}

/** Uma mudança de valor sobre `chave`. Se o estado atual é de outra chave, a mudança parte do estado da chave nova. */
export function escreverEstado<T>(atual: Estado<T>, chave: string, guardado: EstadoGuardado | null, inicial: T | (() => T), mudanca: SetStateAction<T>): Estado<T> {
  const base = atual.chave === chave ? atual : abrirEstado(guardado, chave, inicial)
  const valor = typeof mudanca === 'function' ? (mudanca as (anterior: T) => T)(base.valor) : mudanca
  return { chave, valor }
}

/**
 * `useState` que sobrevive à troca de página: o valor vive no mapa do pai sob `chave`, e a página, ao voltar,
 * começa do que estava. Fora de Ajustes (sem o provedor), é um `useState` comum.
 */
export function useEstadoDaPagina<T>(chave: string, inicial: T | (() => T)): [T, Dispatch<SetStateAction<T>>] {
  const guardado = useContext(EstadoPaginas)
  const [estado, setEstado] = useState<Estado<T>>(() => abrirEstado(guardado, chave, inicial))
  // Chave nova (outro Hub, outro Space): o estado dela, nunca o da chave anterior. Ajusta no render: o React
  // refaz o render antes de mostrar, então ninguém vê o valor antigo.
  let atual = estado
  if (estado.chave !== chave) {
    atual = abrirEstado(guardado, chave, inicial)
    setEstado(atual)
  }
  useEffect(() => { guardado?.set(atual.chave, atual.valor) }, [guardado, atual])
  const setValor = useCallback<Dispatch<SetStateAction<T>>>(
    (mudanca) => setEstado((anterior) => escreverEstado(anterior, chave, guardado, inicial, mudanca)),
    [chave, guardado],
  )
  return [atual.valor, setValor]
}
