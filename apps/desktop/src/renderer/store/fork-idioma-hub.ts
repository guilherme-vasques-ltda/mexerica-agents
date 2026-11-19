// D395 (DON-917): o idioma escolhido no app vai ao Hub, para o Berga responder nele. Vai ao conectar e a
// cada troca; o Hub valida, guarda só o idioma deste app e o carimba nas mensagens que ele mandar.
// Hub antigo, sem o método: a Donna segue em português, sem erro na tela. Sem import de prefs/DOM: a
// fonte do idioma é registrada pela entrada do renderer (`main.tsx`), e o espelho só registra o cliente.

export interface ClienteIdioma {
  call<T>(method: string, body: Record<string, unknown>): Promise<T>
}

let fonte: (() => string) | null = null
const clientes = new Set<() => ClienteIdioma | null>()

export function enviarIdioma(cliente: ClienteIdioma | null): void {
  if (!cliente || !fonte) return
  void cliente.call('donnaAppLanguageSave', { language: fonte() }).catch(() => {})
}

/** O espelho de cada computador: recebe o idioma a cada troca. Devolve como sair. */
export function acompanharIdioma(cliente: () => ClienteIdioma | null): () => void {
  clientes.add(cliente)
  return () => void clientes.delete(cliente)
}

/** A entrada do renderer liga a fonte: o idioma atual e o aviso de troca. */
export function registrarIdioma(atual: () => string, aoTrocar: (aviso: () => void) => () => void): () => void {
  fonte = atual
  const parar = aoTrocar(() => {
    for (const cliente of clientes) enviarIdioma(cliente())
  })
  return () => {
    parar()
    fonte = null
  }
}
