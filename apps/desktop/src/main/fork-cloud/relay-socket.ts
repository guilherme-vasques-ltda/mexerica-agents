import { createRelaySocket } from '../../shared/relay-protocol/relay-socket.ts'
import { Channel, Handshake } from './channel.ts'
import { nodeBackend } from '../../shared/relay-protocol/backend-node.ts'
import { createProtocol } from '../../shared/relay-protocol/protocol.ts'

/**
 * Calço da conexão de saída do aparelho isomórfica (`src/shared/relay-protocol/`): a API
 * pública histórica deste módulo, ligada ao backend `node:crypto` e às classes do canal do
 * calço `./channel.ts`. A implementação vive no pacote compartilhado; o desktop não muda
 * de comportamento.
 */

const { RelayLink: RelayLinkClass } = createRelaySocket({
  proto: createProtocol(nodeBackend),
  channel: { Channel, Handshake },
})

export const RelayLink = RelayLinkClass
export type RelayLink = InstanceType<typeof RelayLink>

export {
  DEFAULT_TIMINGS,
  browserSocket,
  deviceRelayURL,
  nodeSocket,
} from '../../shared/relay-protocol/relay-socket.ts'
export type {
  LinkState,
  RelayLinkOptions,
  RetryCause,
  SocketFactory,
  SocketLike,
  StopReason,
  Timings,
} from '../../shared/relay-protocol/relay-socket.ts'
