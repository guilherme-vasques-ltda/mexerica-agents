import type { HostController } from './host-controller'

/** Relatórios de erro não são encaminhados ao host nem a serviços externos. */
export function registerErrorReports(_host: HostController) {
  process.on('unhandledRejection', (reason) => {
    console.error('Unhandled rejection in the main process:', reason)
  })
}
