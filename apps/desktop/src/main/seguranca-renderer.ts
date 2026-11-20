import { isLocalRendererURL } from './fork-chat-sender.ts'

interface Solicitante {
  janelaPrincipal: boolean
  framePrincipal: boolean
  url: string | undefined
}

export function origemDoChat(renderer: string, solicitante: Solicitante): boolean {
  return solicitante.janelaPrincipal && solicitante.framePrincipal && isLocalRendererURL(renderer)
    && solicitante.url?.split('#')[0] === renderer.split('#')[0]
}

export function permiteNoChat(
  renderer: string,
  solicitante: Solicitante,
  permissao: string,
  tipos: readonly string[] = [],
  origem?: string,
): boolean {
  if (!origemDoChat(renderer, solicitante)) return false
  if (permissao === 'clipboard-sanitized-write') return true
  if (permissao !== 'media' || tipos.length !== 1 || tipos[0] !== 'audio') return false
  const esperado = new URL(renderer)
  const origemEsperada = esperado.protocol === 'file:' ? 'file://' : esperado.origin
  return origem === origemEsperada || origem === `${origemEsperada}/`
}
