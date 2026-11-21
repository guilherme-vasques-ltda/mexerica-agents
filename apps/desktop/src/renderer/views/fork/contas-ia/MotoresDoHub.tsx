import { useEffect, useRef, useState } from 'react'
import type { Backend } from '../../../../shared/models.ts'
import { CardSection, IconButton, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { AgentSheet, AgentTile, NodeNotice } from '../../marketplace/AgentSheet'
import { EmptyState, FormSheet, MarketBadge } from '../../marketplace/pieces'
import { SetupTerminalView } from '../sob-demanda/telas'
import { folhaNoHub, gruposDeMotores, motorEstado, precisaNode } from './motores'
import { motoresFor } from './motores-store'
import { useTextosContas } from './use-textos'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { isolado } from '../../../../shared/bidi.ts'
import './contas-ia.css'

// D169/D164 (docs/reference/redesenho/components/ContasIa): os motores que o Hub suporta, com instalação, Node.js e
// login, na mesma página das contas. A folha e o terminal são os existentes (AgentSheet/SetupTerminal); nada instala ou
// entra sozinho, e a lista é a do Hub (`hello.backends`), relida só ao abrir, por pedido ou ao voltar de uma folha.

/** Identidade estável de um link (objeto) para a chave da origem. */
const linkIds = new WeakMap<object, number>()
let nextLink = 0
const linkId = (client: object | null) => {
  if (!client) return 0
  if (!linkIds.has(client)) linkIds.set(client, ++nextLink)
  return linkIds.get(client)!
}

type Aberto = { kind: 'motor'; backend: Backend; hubId: string; from: HTMLElement | null } | { kind: 'node'; hubId: string; from: HTMLElement | null }

export function MotoresDoHub() {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const store = useStore()
  const motores = useModel(motoresFor(store))
  const [aberto, setAberto] = useState<Aberto | null>(null)
  const online = store.connection.kind === 'online' && !store.mismatch
  // A leitura é cercada por Hub, conexão e geração. A folha mantém o terminal na reconexão do mesmo Hub.
  const origem = `${store.hello?.hostId ?? ''}|${linkId(store.connectedClient)}|${store.principalEpoch}`
  const hubId = store.hello?.hostId ?? ''
  const folha = folhaNoHub(aberto, hubId)
  const backends = store.hello?.backends ?? []
  const { instalados, paraInstalar } = gruposDeMotores(backends)
  const returnTo = useRef<HTMLElement | null>(null)

  // Abrir a página (ou outro Hub nela) relê uma vez; sair cancela a leitura em voo.
  useEffect(() => {
    if (online) void motores.refresh(store)
    return () => motores.cancel()
  }, [motores, online, origem])

  // Trocar de Hub desmonta a folha; uma oscilação do link não encerra o terminal assistido.
  useEffect(() => {
    if (aberto && !folha) setAberto(null)
  }, [aberto, folha])

  const abrir = (next: Aberto) => {
    returnTo.current = next.from
    setAberto(next)
  }
  const fechar = () => {
    const voltou = folha !== null
    setAberto(null)
    // Voltar de uma folha ou do terminal relê para mostrar o que mudou, e o foco volta à linha que abriu.
    if (voltou && online) void motores.refresh(store)
    requestAnimationFrame(() => {
      const from = returnTo.current
      if (from?.isConnected && (document.activeElement === document.body || !document.activeElement)) from.focus()
    })
  }

  const linha = (b: Backend) => {
    const estado = motorEstado(b, idioma)
    return (
      <button key={b.id} className="contas-linha contas-linha-abrir motores-linha" aria-label={`${isolado(b.name)}, ${estado.selo}. ${estado.detalhe}${estado.semAprovacao ? `. ${estado.semAprovacao.selo}: ${estado.semAprovacao.texto}` : ''}`}
        onClick={(e) => abrir({ kind: 'motor', backend: b, hubId, from: e.currentTarget })}>
        <AgentTile registry={b.registry ?? b.id} icon={18} tile={32} radius={9} />
        <VStack as="span" gap={3} grow shrink>
          <HStack as="span" gap={6} shrink>
            <Text tone="text" weight="medium" truncate dir="auto" title={isolado(b.name)}>{b.name}</Text>
            <MarketBadge label={estado.selo} tone={estado.tom === 'pronto' ? 'added' : 'secondary'} icon={estado.tom === 'pronto' ? 'checkmark' : estado.tom === 'atencao' ? 'exclamationmark' : undefined} />
            {estado.semAprovacao ? <MarketBadge label={estado.semAprovacao.selo} icon="exclamationmark.triangle" title={estado.semAprovacao.texto} /> : null}
          </HStack>
          <Text size="caption" tone="secondary" truncate title={estado.detalhe}>{estado.detalhe}</Text>
        </VStack>
        <Icon name="chevron.right" size={10} weight="semibold" color="var(--text-tertiary, var(--ink-3))" />
      </button>
    )
  }

  return (
    <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <CardSection
        title={textos.motoresTitulo}
        accessory={online ? (
          motores.state === 'loading' ? <Spinner size={12} /> : <IconButton title={textos.motoresAtualizar} icon="arrow.clockwise" onClick={() => void motores.refresh(store)} />
        ) : null}
        footer={textos.motoresRodape}
      >
        {!online ? <Text tone="secondary">{textos.motoresDesconectado}</Text> : null}
        {online && motores.state === 'error' ? (
          <HStack gap={8} role="status">
            <Text size="caption" tone="secondary" grow>{textos.motoresErro}</Text>
            <button className="capsule-action" onClick={() => void motores.refresh(store)}>{textos.tentarDeNovo}</button>
          </HStack>
        ) : null}
        {online && precisaNode(backends) ? <NodeNotice onAdd={() => abrir({ kind: 'node', hubId, from: document.activeElement as HTMLElement | null })} /> : null}
        {online && !instalados.length && !paraInstalar.length ? (
          motores.state === 'loading' ? <Text tone="secondary" role="status">{textos.motoresCarregando}</Text>
            : <EmptyState icon="terminal" title={textos.motoresVazioTitulo} detail={textos.motoresVazioDetalhe} />
        ) : null}
        {online && instalados.length ? <Text size="caption" weight="semibold" tone="secondary" className="motores-grupo">{textos.motoresInstalados}</Text> : null}
        {online ? instalados.map(linha) : null}
        {online && paraInstalar.length ? <Text size="caption" weight="semibold" tone="secondary" className="motores-grupo">{textos.motoresParaInstalar}</Text> : null}
        {online ? paraInstalar.map(linha) : null}
      </CardSection>

      <FormSheet language={superficieIdioma(idioma)} open={folha !== null} onClose={fechar} width={560} height={560} fixed>
        {folha?.kind === 'motor' ? <AgentSheet key={`${folha.hubId}|${folha.backend.id}`} initial={folha.backend} />
          : folha?.kind === 'node' ? <SetupTerminalView backend={null} step="node" back={fechar} /> : null}
      </FormSheet>
    </div>
  )
}
