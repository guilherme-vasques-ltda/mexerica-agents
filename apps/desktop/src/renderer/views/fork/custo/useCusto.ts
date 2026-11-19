// DON-908 (C8): lê `donnaCostReport` de um Hub, como a C6 e a C2: resposta de outra conexão (Hub, reconexão,
// principal) ou de um pedido mais velho é descartada. O estado guardado leva a identidade em que foi lido (origem,
// corpo e client): com outra identidade, o render já devolve "lendo", sem esperar efeito (nem um quadro do anterior).
import { useCallback, useEffect, useRef, useState } from 'react'
import type { BotStore } from '../../../store/bot-store'
import { codigoFalha, corpoDo, daMesmaLeitura, lerRelatorio, RPC_CUSTO, type Falha, type Pedido, type RelatorioCusto } from './modelo'

export type EstadoCusto = { kind: 'lendo' } | { kind: 'pronto'; relatorio: RelatorioCusto } | { kind: 'falha'; falha: Falha }
const LENDO: EstadoCusto = { kind: 'lendo' }

/** `ativo` false: não pede (balão fechado). `ler` relê na hora (Atualizar, abrir o balão). */
export function useCusto(store: BotStore, pedido: Pedido, ativo: boolean) {
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  const hubId = store.hello?.hostId ?? null
  const corpo = JSON.stringify(corpoDo(pedido))
  const chave = JSON.stringify([hubId, store.principalEpoch, corpo])
  const [guardado, setGuardado] = useState<{ chave: string; client: unknown; valor: EstadoCusto } | null>(null)
  const [lendoEm, setLendoEm] = useState<{ chave: string; client: unknown; valor: true } | null>(null)
  const seq = useRef(0)

  const ler = useCallback(() => {
    if (!client || !hubId) return
    const minha = ++seq.current
    const id = { chave, client }
    setLendoEm({ ...id, valor: true })
    client.call(RPC_CUSTO, JSON.parse(corpo) as Record<string, string>).then((raw) => lerRelatorio(raw, pedido, hubId)).then(
      (relatorio) => { if (seq.current === minha) setGuardado({ ...id, valor: { kind: 'pronto', relatorio } }) },
      (e: unknown) => { if (seq.current === minha) setGuardado({ ...id, valor: { kind: 'falha', falha: codigoFalha(e) } }) },
    ).finally(() => { if (seq.current === minha) setLendoEm(null) })
    // `pedido` é o mesmo que `corpo` (dentro de `chave`) descreve.
  }, [client, hubId, chave, corpo])

  // Outra identidade ou balão fechado: a resposta em voo do pedido anterior é descartada.
  useEffect(() => { if (ativo) ler(); return () => { seq.current++ } }, [ler, ativo])

  return {
    estado: daMesmaLeitura(guardado, chave, client) ?? LENDO,
    lendo: daMesmaLeitura(lendoEm, chave, client) ?? false,
    ler,
    online: !!client,
  }
}
