import type { EstadoChaves, VozBridge } from '../../../../shared/fork-voz'
import { Observable } from '../../../lib/observable'

// voz-04: o estado das chaves deste aparelho, como o main o conta (situação de cada provedor, sem a chave).
// Relido a cada aviso do main e a cada resposta de ação; uma leitura antiga nunca cobre uma mais nova.

export class EstadoVozAparelho extends Observable {
  /** null enquanto o main não respondeu. */
  estado: EstadoChaves | null = null
  private geracao = 0
  private observadores = 0
  private parar: (() => void) | null = null

  constructor(private readonly ponte: () => VozBridge) { super() }

  observar(): () => void {
    if (this.observadores++ === 0) {
      this.parar = this.ponte().onChange(this.atualizar)
      this.atualizar()
    }
    let ativo = true
    return () => {
      if (!ativo) return
      ativo = false
      if (--this.observadores > 0) return
      this.parar?.()
      this.parar = null
    }
  }

  atualizar = () => {
    const geracao = ++this.geracao
    void this.ponte().estado().then((e) => {
      if (geracao !== this.geracao) return
      this.estado = e
      this.changed()
    }).catch(() => {})
  }

  /** A resposta de uma ação já traz o estado novo. */
  aplicar(e: EstadoChaves | null) {
    if (!e) return this.atualizar()
    this.geracao++
    this.estado = e
    this.changed()
  }
}

let unico: EstadoVozAparelho | null = null
/** O mesmo estado para a tela Voz e para a folha da chamada. */
export const estadoVozAparelho = () => (unico ??= new EstadoVozAparelho(() => window.codync.voz))

/** Para a chamada: o estado agora, ou null se o main não respondeu (a chamada não liga sem saber). */
export async function chavesDoAparelho(ponte: VozBridge = window.codync.voz): Promise<EstadoChaves['chaves'] | null> {
  try {
    return (await ponte.estado()).chaves
  } catch {
    return null
  }
}
