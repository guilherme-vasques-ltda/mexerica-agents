// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
// D71: preserve the upstream account entrypoint with a short fork hook.
export { AccountSession, account, useAccount, exitUnresolved, exitActions } from './fork-account-session'
export type { AccountInfo, ExitKind, ExitAction, ExitIntent } from './fork-account-session'
