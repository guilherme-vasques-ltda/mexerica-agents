import type { WebContents } from 'electron'

// Local ao MAIN. Não é Authority, HumanReviewProof ou isolamento do sistema operacional.
// Sem remoção: navegação e eventos do renderer não tornam esta instância geral novamente.
const privateContexts = new WeakSet<WebContents>()

/** Reservado à futura criação dedicada no MAIN, antes de expor o contexto. Sem IPC público. */
export function markPrivateContext(contents: WebContents): void {
  privateContexts.add(contents)
}

export function isGeneralContext(contents: WebContents): boolean {
  return !privateContexts.has(contents) && !contents.isDestroyed()
}

/** Consulta a lista atual em cada emissão, preservando a identidade dos objetos. */
export function generalWindowProvider<T extends { webContents: WebContents }>(windows: () => T[]): () => T[] {
  return () => windows().filter(window => isGeneralContext(window.webContents))
}
