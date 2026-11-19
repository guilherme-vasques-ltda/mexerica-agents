import { createChannel } from '../../shared/relay-protocol/channel.ts'
import { nodeBackend } from '../../shared/relay-protocol/backend-node.ts'
import { createProtocol } from '../../shared/relay-protocol/protocol.ts'

/**
 * Calço do canal E2E isomórfico (`src/shared/relay-protocol/`): a API pública histórica
 * deste módulo, ligada ao backend `node:crypto`. A implementação vive no pacote
 * compartilhado; o desktop não muda de comportamento.
 */

const classes = createChannel(createProtocol(nodeBackend))

export const Channel = classes.Channel
export const Handshake = classes.Handshake
export type Channel = InstanceType<typeof Channel>
export type Handshake = InstanceType<typeof Handshake>

export {
  ChannelBroken,
  MAX_INCOMING,
  MAX_OUTGOING,
  MAX_REQUESTS,
  MAX_SUBSCRIPTIONS,
} from '../../shared/relay-protocol/channel.ts'
export type {
  CallResult,
  ChannelWire,
  HandshakeOutcome,
  RejectCode,
  Subscription,
} from '../../shared/relay-protocol/channel.ts'
