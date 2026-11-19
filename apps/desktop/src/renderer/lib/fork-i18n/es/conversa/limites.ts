import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/limites.ts'

// DON-903: conversa/limites (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'conversa.limites.titulo': 'Límites de uso',
  'conversa.limites.retomar': 'Reanudar después del límite',
  'conversa.limites.retomarDetalhe': 'Cuando se restablezca el límite de la cuenta, el bot continuará la solicitud que se detuvo.',
  'conversa.limites.soneca': 'Posponer hasta que se restablezca el límite',
  'conversa.limites.sonecaDetalhe': 'La conversación espera la hora de restablecimiento, sin intentos previos ni avisos mientras tanto.',
  'conversa.limites.rodape': 'Se aplica a todos los bots de este Hub. En cada conversación, «No reanudar» cancela solo esa espera.',
  'conversa.limites.carregando': 'Leyendo los ajustes…',
  'conversa.limites.semConexao': 'Sin conexión con el Hub. Los ajustes aparecerán cuando vuelva.',
  'conversa.limites.hubAntigo': 'Actualiza el Hub para cambiar los límites de uso.',
  'conversa.limites.falhaLer': 'No se pudieron leer los ajustes en este momento.',
  'conversa.limites.falhaGravar': 'No se pudo confirmar el guardado. Revisa los ajustes.',
  'conversa.limites.tentarDeNovo': 'Reintentar',
  'conversa.limites.naoRetomar': 'No reanudar',
  'conversa.limites.retomarAgora': 'Reanudar ahora',
  'conversa.limites.falhaCancelar': 'No se pudo confirmar la cancelación. Revisa la conversación antes de volver a intentarlo.',
  'conversa.limites.falhaRetomar': 'No se pudo reanudar ahora. Inténtalo de nuevo.',
}
