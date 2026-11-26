import { HostClient } from '../../../client/host-client'

/** As contas de IA são do dono e globais no Hub: a leitura delas e o lote para todos os bots vão sem o Space da janela.
 *  A troca de um bot usa o cliente vinculado, com o Space e o Hub dele. */
export const clientePessoal = (client: HostClient): HostClient =>
  client.spaceId ? new HostClient(client.transport, undefined, client.boundHubId) : client
