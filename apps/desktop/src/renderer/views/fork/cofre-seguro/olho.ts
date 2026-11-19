import { OLHO_MS } from '../../../../shared/fork-cofre-seguro.ts'

// MX-154 (decisão do dono): o olho "Mostrar" deixa o valor visível por no máximo 10 s; esconde sozinho, ao sair do
// campo (blur) e ao fechar. Controlador puro, sem DOM: a janela liga os eventos a ele e os testes o exercitam.
export interface OlhoOpcoes {
  aoMudar: (visivel: boolean) => void
  ms?: number
  agendar?: (fn: () => void, ms: number) => unknown
  cancelar?: (id: unknown) => void
}

export class Olho {
  visivel = false
  private id: unknown = null
  private readonly ms: number
  private readonly agendar: (fn: () => void, ms: number) => unknown
  private readonly cancelar: (id: unknown) => void

  private readonly opcoes: OlhoOpcoes

  constructor(opcoes: OlhoOpcoes) {
    this.opcoes = opcoes
    this.ms = opcoes.ms ?? OLHO_MS
    this.agendar = opcoes.agendar ?? ((fn, ms) => setTimeout(fn, ms))
    this.cancelar = opcoes.cancelar ?? ((id) => clearTimeout(id as ReturnType<typeof setTimeout>))
  }

  mostrar() {
    this.parar()
    this.id = this.agendar(() => this.esconder(), this.ms)
    this.mudar(true)
  }

  /** Blur, fechar a janela, o tempo acabar ou o toque no olho: tudo volta a esconder. */
  esconder() {
    this.parar()
    this.mudar(false)
  }

  alternar() {
    if (this.visivel) this.esconder()
    else this.mostrar()
  }

  private parar() {
    if (this.id !== null) this.cancelar(this.id)
    this.id = null
  }

  private mudar(visivel: boolean) {
    if (this.visivel === visivel) return
    this.visivel = visivel
    this.opcoes.aoMudar(visivel)
  }
}
