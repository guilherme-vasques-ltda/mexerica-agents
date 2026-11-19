// Os atalhos que os rótulos e os atributos de acessibilidade mostram, por plataforma. Quem rotula chama estas
// funções; os rótulos não têm `if` de plataforma próprio.
//
// Onde cada atalho existe de verdade:
// - busca: no app e na Web, pelo teclado (ChatWindow, teclaDeBusca): ⌘K no Mac, Ctrl+K fora dele;
// - barra lateral: só no app do Mac, pelo menu do Electron (main/index.ts, ⌃⌘S). Na Web não há tecla que a
//   recolha, então o rótulo e o atributo não o citam; fora do Mac o menu não tem este comando.

export type AtalhoApp = 'busca' | 'barraLateral'

/** O atalho do app nesta plataforma, como o rótulo mostra; `null` quando não há um. */
export function atalhoDoApp(id: AtalhoApp, mac: boolean, web = false): string | null {
  if (id === 'barraLateral') return mac && !web ? '⌃⌘S' : null
  return mac ? '⌘K' : 'Ctrl+K'
}

/** O mesmo atalho no formato de `aria-keyshortcuts`; `null` quando não há um (o atributo some). */
export function atalhoAria(id: AtalhoApp, mac: boolean, web = false): string | null {
  if (id === 'barraLateral') return mac && !web ? 'Control+Meta+S' : null
  return mac ? 'Meta+K' : 'Control+K'
}

/** O texto com o atalho entre parênteses; sem atalho nesta plataforma, o texto puro. */
export function comAtalho(texto: string, id: AtalhoApp, mac: boolean, web = false): string {
  const atalho = atalhoDoApp(id, mac, web)
  return atalho ? `${texto} (${atalho})` : texto
}

/** A tecla do atalho de busca: ⌘K no Mac, Ctrl+K fora dele. Outro modificador (Alt, Shift) deixa de ser o atalho. */
export function teclaDeBusca(e: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>, mac: boolean): boolean {
  const modificador = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
  return modificador && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'k'
}

/**
 * Se a plataforma é o Mac. No app, pelo sistema; na Web, pelo navegador, porque o stub de plataforma da Web diz
 * `linux` para qualquer navegador.
 */
export function macNoAparelho(web: boolean, plataforma: string, navegador: string): boolean {
  return web ? /Mac|iPhone|iPad|iPod/.test(navegador) : plataforma === 'darwin'
}

/** `macNoAparelho` com o que esta janela sabe (o bridge e o navegador). */
export const macDesteAparelho = (): boolean => macNoAparelho(webDesteAparelho(), window.codync.platform, navigator.platform)

/**
 * Esta janela roda na Web (o bridge marca `web`); no app do Electron, não. Sem janela (testes no Node, texto montado
 * fora da tela), é falso: como `ehMac` em frases.ts, não lança.
 */
export const webDesteAparelho = (): boolean => globalThis.window?.codync?.web === true
