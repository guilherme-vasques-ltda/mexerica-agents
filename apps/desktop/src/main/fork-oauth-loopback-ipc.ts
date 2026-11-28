// IPC do ouvinte de retorno do login de conector (fork-oauth-loopback.ts) no contexto geral do app.
import { identity } from './environment'
import { PORTA_DEV, PORTA_PRINCIPAL } from './fork-oauth-loopback'
import { registrarRetornoDoLogin } from './fork-oauth-loopback-registro'
import { isGeneralContext } from './fork-secret-box/contexts'

export function registerOAuthLoopbackIPC() {
  registrarRetornoDoLogin({
    porta: identity.environment === 'dev' ? PORTA_DEV : PORTA_PRINCIPAL,
    allowed: (e) => isGeneralContext(e.sender),
  })
}
