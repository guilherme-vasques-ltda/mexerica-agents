// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
// D71: account implementation and session boundaries belong to the fork.
export { AccountService, isMainEnvironment } from './fork-account/service'
export type { CloudSeal, DeviceBinding, DeviceRevocation } from './fork-account/service'
