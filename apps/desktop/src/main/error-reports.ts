// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { HostController } from './host-controller'

/** Relatórios de erro não são encaminhados ao host nem a serviços externos. */
export function registerErrorReports(_host: HostController) {
  process.on('unhandledRejection', (reason) => {
    console.error('Unhandled rejection in the main process:', reason)
  })
}
