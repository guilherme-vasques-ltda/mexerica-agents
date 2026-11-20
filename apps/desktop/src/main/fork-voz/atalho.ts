import { criarGuardaPressao, validarAtalho, type Plataforma } from '../../shared/fork-voz-entrada.ts'
import type { PedidoAtalhoVoz, ResultadoAtalhoVoz } from '../../shared/fork-voz.ts'

// o atalho global que abre ou encerra a chamada de voz. Registrar o atalho NÃO abre o microfone nem escuta nada:
// o sistema só avisa que a combinação foi apertada. O processo principal manda `voiceToggle` ao chat, que decide
// (shared/fork-voz-entrada) e abre a mesma chamada do botão. A preferência mora na tela; o main só registra o que ela pede.

export interface AtalhoGlobal {
  register(acelerador: string, aoApertar: () => void): boolean
  unregister(acelerador: string): void
  isRegistered(acelerador: string): boolean
}

export interface DependenciasAtalhoVoz {
  global: AtalhoGlobal
  plataforma: Plataforma
  /** Traz o chat à frente e entrega o comando à tela. */
  enviar(comando: { kind: 'voiceToggle' }): void
  agora?: () => number
}

const pedido = (v: unknown): v is PedidoAtalhoVoz =>
  typeof v === 'object' && v !== null && typeof (v as PedidoAtalhoVoz).ativo === 'boolean' && typeof (v as PedidoAtalhoVoz).acelerador === 'string'

export function criarAtalhoVoz({ global, plataforma, enviar, agora = Date.now }: DependenciasAtalhoVoz) {
  let atual: string | null = null
  const aceita = criarGuardaPressao(agora)

  const liberar = () => {
    if (atual) global.unregister(atual)
    atual = null
  }

  return {
    definir(entrada: unknown): ResultadoAtalhoVoz {
      if (!pedido(entrada)) return { ok: false, motivo: 'invalido' }
      liberar()
      if (!entrada.ativo) return { ok: true, acelerador: null }
      const valido = validarAtalho(entrada.acelerador, plataforma)
      if (!valido.ok) return valido
      try {
        // Combinação que outro app (ou a Ilha) já registrou: não toma nem divide.
        if (global.isRegistered(valido.acelerador)) return { ok: false, motivo: 'em-uso' }
        if (!global.register(valido.acelerador, () => { if (aceita()) enviar({ kind: 'voiceToggle' }) })) return { ok: false, motivo: 'em-uso' }
      } catch {
        return { ok: false, motivo: 'indisponivel' }
      }
      atual = valido.acelerador
      return { ok: true, acelerador: valido.acelerador }
    },
    liberar,
  }
}
