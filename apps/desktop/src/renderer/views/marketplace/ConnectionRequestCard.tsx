import { useState } from 'react'
import type { Backend, Entry } from '@shared/models'
import { Button, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { AgentSheet, AgentTile } from './AgentSheet'
import { mesmaOrigem, origemDe } from '../fork/meus-mcp/estado'
import {
  errorText, market, needsSignIn,
  type ConnectionRequest,
} from './market-models'
import { FormSheet, ServiceLogo } from './pieces'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'

import { pluginsFor } from './plugins'
import { cru, ErroFrase, frase, textoDe, type Frase } from '../../lib/fork-i18n/frase'
import { PedidoSeguro } from '../fork/cofre-seguro/PedidoSeguro'
import { ehPedidoSeguro } from '../fork/cofre-seguro/estado'

/**
 * MX-209: login e chave viram o "pedido seguro" (views/fork/cofre-seguro): o valor é digitado numa janela própria
 * e nunca passa por este cartão nem pelo chat. Os demais pedidos (agente, app, conector) seguem aqui.
 */
export function ConnectionRequestCard({ entry }: { entry: Entry }) {
  const request = entry.data.connectionRequest as unknown as ConnectionRequest
  return ehPedidoSeguro(request) ? <PedidoSeguro entry={entry} /> : <CartaoDePedido entry={entry} />
}

/** Credentials go through the setup API; only the request's state is a chat entry. */
function CartaoDePedido({ entry }: { entry: Entry }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const request = entry.data.connectionRequest as unknown as ConnectionRequest
  const [agent, setAgent] = useState<Backend | null>(null)
  const [busy, setBusy] = useState(false)
  // Lição da R1 do #572: o erro escrito aqui fica como chave + parâmetros e é traduzido ao mostrar.
  const [error, setError] = useState<Frase | null>(null)
  const isAgent = request.kind === 'agent'
  const pending = request.status === 'pending'
  // D175: a connector request from the retired catalog (no connectorId) can't be completed from here: no
  // lookup, no install, no substitute. The connection is added by hand in Extensões → Meus MCP.
  const catalogOnly = !isAgent && !request.connectorId

  const connect = async () => {
    const client = store.client
    if (!client) return
    setBusy(true)
    setError(null)
    try {
      if (isAgent) {
        // The host completes the card and resends the messages once the sheet finds it signed in.
        const backend = store.hello?.backends.find((b) => b.id === request.backend)
        if (backend) setAgent(backend)
        else setError(frase('extensoes.pedido.abrirParaEntrar', { titulo: request.title }))
      } else if (request.connectorId) {
        // The saved connector, on the Hub, link and generation the gesture started on.
        const id = request.connectorId
        const origin = origemDe(store)
        if (!origin) throw new ErroFrase(frase('extensoes.mcp.semConexao'))
        const connector = (await market.connectors(origin.client)).find((c) => c.id === id)
        if (!mesmaOrigem(store, origin)) throw new ErroFrase(frase('extensoes.pedido.conexaoMudou'))
        if (connector && needsSignIn(connector)) await pluginsFor(store).signIn(id)
        if (!mesmaOrigem(store, origin)) throw new ErroFrase(frase('extensoes.pedido.conexaoMudou'))
        await market.finishConnectionRequest(origin.client, entry.id, { connectorId: id })
      }
    } catch (e) {
      setError(e instanceof ErroFrase ? e.frase : cru(errorText(e)))
    } finally {
      setBusy(false)
    }
  }

  return (
    // D454 (fatia 8a): o cartão declara o idioma escolhido; o título e o texto do pedido vêm do bot (dir="auto").
    <VStack gap={12} className="request-card idioma-superficie" {...superficieIdioma(idioma)}>
      <HStack gap={14}>
        <RequestLogo request={request} />
        <VStack gap={2} grow>
          <Text size="body" weight="semibold" tone="text" truncate dir="auto">{request.title}</Text>
          {entry.data.text ? <Text size="subheadline" tone="secondary" lines={2} title={entry.data.text} dir="auto">{entry.data.text}</Text> : null}
        </VStack>
        {pending && catalogOnly ? (
          <HStack as="span" gap={6} className="request-card-trailing request-card-status">{t('extensoes.pedido.manual')}</HStack>
        ) : pending ? (
          <HStack gap={6} className="request-card-trailing">
            {busy ? <Spinner /> : null}
            <Button disabled={busy} onClick={() => void connect()}>
              {isAgent ? t('extensoes.pedido.entrar') : t('extensoes.pedido.conectar')}
            </Button>
          </HStack>
        ) : (
          <HStack as="span" gap={6} className="request-card-trailing request-card-status">
            {request.status === 'ready' ? <Icon name="checkmark" size={11} weight="semibold" /> : null}
            {request.status === 'ready' ? (isAgent ? t('extensoes.pedido.entrou') : t('extensoes.pedido.conectado')) : t('extensoes.pedido.cancelado')}
          </HStack>
        )}
      </HStack>
      {pending && catalogOnly ? (
        <Text size="caption" tone="secondary">{t('extensoes.pedido.catalogoRetirado')}</Text>
      ) : null}
      {error ? <Text size="footnote" tone="danger">{textoDe(error, idioma)}</Text> : null}
      <FormSheet language={superficieIdioma(idioma)} open={agent !== null} onClose={() => setAgent(null)} width={560} height={560} fixed>
        {agent ? <AgentSheet key={agent.id} initial={agent} botId={entry.botId} recheck /> : null}
      </FormSheet>
    </VStack>
  )
}

/** The agent or service a request is for (login and secret requests are drawn by PedidoSeguro). */
function RequestLogo({ request }: { request: ConnectionRequest }) {
  const store = useStore()
  if (request.kind === 'agent') {
    const backend = store.hello?.backends.find((b) => b.id === request.backend)
    return <AgentTile registry={backend?.registry ?? request.backend} icon={26} tile={46} radius={12} />
  }
  return <ServiceLogo name={request.title} registryName={request.registryName} />
}
