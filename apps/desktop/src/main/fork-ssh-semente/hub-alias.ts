import { DONNABOT_HUB_HOST } from '../../shared/ssh.ts'

// Alias do Hub no ~/.ssh/config: padrão genérico. `MEXERICA_HUB_SSH_ALIAS` troca por outro alias,
// desde que seja inteiro de [A-Za-z0-9._-] e não comece com hífen. Qualquer outro valor (vazio, espaço,
// metacaractere, hífen inicial) volta ao padrão: o alias vai para o perfil e para o comando mostrado ao usuário.
// Só o main lê o ambiente; o renderer usa o padrão da shared/ssh.ts.
const ALIAS_VALIDO = /^[A-Za-z0-9._-]+$/

export const hubSSHAlias = (env: Record<string, string | undefined>): string => {
  const valor = env.MEXERICA_HUB_SSH_ALIAS
  return valor && ALIAS_VALIDO.test(valor) && !valor.startsWith('-') ? valor : DONNABOT_HUB_HOST
}
