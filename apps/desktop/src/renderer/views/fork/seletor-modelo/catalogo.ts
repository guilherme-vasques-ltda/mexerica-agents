// DON-907 C9: o catálogo do seletor. Os motores vêm do `hello` (instalados e com login); os modelos de cada um, do
// próprio motor (`agentModels`), como no editor do bot. Lido só quando o seletor abre, e guardado por computador.
// Ainda não há tamanho de contexto nem lista de permitidos por conta (pendência da frente Modelos e contas, D370).
import { useEffect, useState } from 'react'
import type { BotStore } from '../../../store/bot-store'
import type { AgentModels } from '../../bots/agent-models'
import { separarPadrao, type Provedor } from './regras'

const cache = new Map<string, AgentModels | 'falhou'>()
const lendo = new Set<string>()
/** Geração de cada chave: uma leitura que começou antes de a lista mudar não escreve por cima. */
const geracoes = new Map<string, number>()
const listeners = new Set<() => void>()
const avisar = () => { for (const l of listeners) l() }
/** A lista sem o "Default" é feita uma vez por catálogo lido: a mesma lista a cada desenho (o filtro em memo depende disso). */
const separados = new WeakMap<AgentModels, ReturnType<typeof separarPadrao>>()
const separar = (catalogo: AgentModels) => {
  let lista = separados.get(catalogo)
  if (!lista) separados.set(catalogo, lista = separarPadrao(catalogo.models))
  return lista
}

/** Os motores que o seletor oferece: instalados, disponíveis e sem login pendente. */
export function provedoresDoHub(store: BotStore): Provedor[] {
  const backends = store.hello?.backends ?? []
  return backends.filter((b) => b.available && b.installed !== false && b.signedIn !== false && b.id !== 'custom').map((b) => {
    const lido = cache.get(`${store.computer.id}/${b.id}`)
    const catalogo = lido && lido !== 'falhou' ? lido : null
    const lista = catalogo ? separar(catalogo) : null
    return { id: b.id, name: b.name, registry: b.registry ?? b.id, models: lido === 'falhou' ? [] : (lista?.models ?? null), fastMode: catalogo?.fastMode === true, ...(lista?.padrao ? { padrao: lista.padrao } : {}), ...(lido === 'falhou' ? { falhou: true } : {}) }
  })
}

/** Lê os modelos de um motor uma vez por computador; uma falha mostra a lista vazia ("não lista modelos"). */
export function carregarModelos(store: BotStore, backend: string) {
  const key = `${store.computer.id}/${backend}`
  const client = store.client
  if (cache.has(key) || lendo.has(key) || !client) return
  lendo.add(key)
  const minha = (geracoes.get(key) ?? 0) + 1
  geracoes.set(key, minha)
  void client.call<AgentModels>('agentModels', { backend }, 300_000)
    .then((res) => { if (geracoes.get(key) === minha) cache.set(key, res) })
    .catch(() => { if (geracoes.get(key) === minha) cache.set(key, 'falhou') })
    .finally(() => { lendo.delete(key); avisar() })
}

/** A lista de um motor mudou no Hub (chave do Motor Mexerica salva, trocada ou removida): a próxima abertura relê. */
export function esquecerModelos(computerId: string, backend: string) {
  const key = `${computerId}/${backend}`
  cache.delete(key)
  geracoes.set(key, (geracoes.get(key) ?? 0) + 1)
  avisar()
}

/** Uma conexão nova (ou o chip montando de novo): a leitura que falhou neste computador e motor é tentada de novo. */
export function esquecerFalha(store: BotStore, backend: string) {
  const key = `${store.computer.id}/${backend}`
  if (cache.get(key) === 'falhou') cache.delete(key)
}

/** Ao abrir o seletor: uma leitura que falhou (Hub ocupado, rede) pode ser tentada de novo. */
export function esquecerFalhas() {
  for (const [key, valor] of cache) if (valor === 'falhou') cache.delete(key)
}

/** Redesenha quando um catálogo chega. */
export function useCatalogoMudou() {
  const [, setN] = useState(0)
  useEffect(() => {
    const l = () => setN((n) => n + 1)
    listeners.add(l)
    return () => void listeners.delete(l)
  }, [])
}
