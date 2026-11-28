// `donnaUsageHistory` num Hub, no padrão de `useCusto`: resposta de outra conexão (Hub, reconexão, principal), de outra
// faixa ou de um pedido mais velho é descartada. Só lê o que o Hub já gravou: não consulta o provedor.
import { useCallback, useEffect, useRef, useState } from 'react'
import type { BotStore } from '../../../store/bot-store'
import { codigoFalha, daMesmaLeitura, type Falha } from './modelo'
import { lerHistorico, RPC_HISTORICO, type FaixaHistorico, type HistoricoCotas } from './historico'

export type EstadoHistorico = { kind: 'lendo' } | { kind: 'pronto'; historico: HistoricoCotas } | { kind: 'falha'; falha: Falha }
const LENDO: EstadoHistorico = { kind: 'lendo' }

export function useHistorico(store: BotStore, faixa: FaixaHistorico, ativo: boolean) {
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  const hubId = store.hello?.hostId ?? null
  const chave = JSON.stringify([hubId, store.principalEpoch, faixa])
  const [guardado, setGuardado] = useState<{ chave: string; client: unknown; valor: EstadoHistorico } | null>(null)
  const [lendoEm, setLendoEm] = useState<{ chave: string; client: unknown; valor: true } | null>(null)
  const seq = useRef(0)

  const ler = useCallback(() => {
    if (!client || !hubId) return
    const minha = ++seq.current
    const identidade = { chave, client }
    setLendoEm({ ...identidade, valor: true })
    client.call(RPC_HISTORICO, { range: faixa }).then((raw) => lerHistorico(raw, faixa)).then(
      (historico) => { if (seq.current === minha) setGuardado({ ...identidade, valor: { kind: 'pronto', historico } }) },
      (e: unknown) => { if (seq.current === minha) setGuardado({ ...identidade, valor: { kind: 'falha', falha: codigoFalha(e) } }) },
    ).finally(() => { if (seq.current === minha) setLendoEm(null) })
  }, [client, hubId, chave, faixa])

  useEffect(() => { if (ativo) ler(); return () => { seq.current++ } }, [ler, ativo])

  return {
    estado: daMesmaLeitura(guardado, chave, client) ?? LENDO,
    lendo: daMesmaLeitura(lendoEm, chave, client) ?? false,
    ler,
  }
}
