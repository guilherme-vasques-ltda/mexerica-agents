// MX-145 E2: o cadastro de contas do Hub (`donnaEngineAccounts`) só para dar nome e tipo às contas do painel de uso.
// Falha ou resposta fora do formato viram `falha`: o painel mostra o ID curto e os números ficam. Nada vai para o
// mirror nem para cache. Como o custo (`useCusto`), a leitura vale só para a mesma identidade: Hub, época da
// principal (resync) e client (reconexão). Outra identidade, ou sem conexão, o render já devolve "lendo" (nunca o
// cadastro velho junto de um relatório novo), e a resposta em voo de uma leitura anterior é descartada. As regras
// estão em `LeituraCadastro` (testável sem React); aqui só a ligação ao ciclo do React.
import { useCallback, useEffect, useState, useSyncExternalStore } from 'react'
import type { BotStore } from '../../../store/bot-store'
import { engineSetup } from '../../../store/fork-engine-setup'
import { chaveDoCadastro, lerCadastro } from './modelo'
import { LeituraCadastro, type EstadoCadastro } from './leituraCadastro'

export type { EstadoCadastro }

/** `ativo` false: não pede (a aba não usa o nome da conta). `recarregar` relê (Atualizar). */
export function useCadastroContas(store: BotStore, ativo: boolean): { estado: EstadoCadastro; recarregar: () => void } {
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  const hubId = store.hello?.hostId ?? null
  const chave = chaveDoCadastro(hubId, store.principalEpoch)
  const [leitura] = useState(() => new LeituraCadastro())
  useSyncExternalStore(leitura.assinar, leitura.foto, leitura.foto)

  const recarregar = useCallback(() => {
    if (!client || !hubId) return
    leitura.ler(chave, client, () => engineSetup.accounts(client).then(lerCadastro))
  }, [leitura, client, hubId, chave])

  useEffect(() => { if (ativo) recarregar(); return () => leitura.soltar() }, [leitura, recarregar, ativo])
  useEffect(() => { if (!client) leitura.esquecer() }, [leitura, client])

  return { estado: leitura.estado(chave, client), recarregar }
}
