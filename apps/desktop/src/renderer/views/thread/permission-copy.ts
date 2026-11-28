import type { Tradutor } from '../../lib/fork-i18n/estado'

// Textos em conversa.aprovacao.* (lib/fork-i18n/<idioma>/conversa/aprovacao.ts).
type Pedido = 'captura' | 'memoria' | 'comando' | 'arquivos' | 'web' | 'leitura' | 'ferramenta'

const pedido = (toolKind?: string): Pedido => {
  switch (toolKind) {
    case 'mac_local_screen_action':
      return 'captura'
    case 'mcp_memory':
      return 'memoria'
    case 'execute':
    case 'open_local_command':
      return 'comando'
    case 'edit':
    case 'delete':
    case 'move':
      return 'arquivos'
    case 'fetch':
      return 'web'
    case 'read':
    case 'search':
      return 'leitura'
    default:
      return 'ferramenta'
  }
}

/** Texto de apresentação; o nome do servidor não comprova sua origem. */
export const permissionHeadline = (t: Tradutor, toolKind?: string) => t(`conversa.aprovacao.pedido.${pedido(toolKind)}`)

const ACOES = ['mcp_memory', 'execute', 'edit', 'delete', 'move', 'fetch', 'read', 'search'] as const
type Acao = (typeof ACOES)[number]
const ehAcao = (k: string | undefined): k is Acao => (ACOES as readonly string[]).includes(k ?? '')

export const actionLabel = (t: Tradutor, toolKind?: string) => {
  if (toolKind === 'mac_local_screen_action') return t('conversa.aprovacao.acao.captura')
  const kind = toolKind === 'open_local_command' ? 'execute' : toolKind
  return t(ehAcao(kind) ? `conversa.aprovacao.acao.${kind}` : 'conversa.aprovacao.acao.ferramenta')
}

export function permissionDetail(t: Tradutor, toolKind: string | undefined, title: string | undefined) {
  if (toolKind !== 'mcp_memory' || !title) return title ?? ''
  const technicalName = title.replace(/^Autorizar /, '').replace(/: memory$/, '')
  return technicalName.startsWith('mcp.') ? t('conversa.aprovacao.nomeTecnico', { nome: technicalName }) : ''
}

export const capturePermissionNote = (t: Tradutor, toolKind?: string) => toolKind === 'mac_local_screen_action'
  ? t('conversa.aprovacao.notaCaptura')
  : ''

export const memoryPermissionNote = (t: Tradutor, toolKind?: string) => toolKind === 'mcp_memory'
  ? t('conversa.aprovacao.notaMemoria')
  : ''

/** Com autor, a frase inteira vem do catálogo ("{autor} quer rodar um comando"), sem montar pedaços. */
export function permissionTitle(t: Tradutor, toolKind: string | undefined, author: string | null | undefined) {
  return author ? t(`conversa.aprovacao.pedidoDe.${pedido(toolKind)}`, { autor: author }) : permissionHeadline(t, toolKind)
}
