import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/parear.ts'

// Fatia 1 (D454): navegacao/parear (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.parear.qr": "Pairing code for {computador}",
  "navegacao.parear.semRede": "No network address found and “Reach from anywhere” is off. Connect to Wi-Fi or turn it on.",
  "navegacao.parear.mesmaRede": "“Reach from anywhere” is off, so the iPhone only reaches {computador} on the same network.",
  "navegacao.parear.valeAte": "Works once, until {hora}",
  "navegacao.parear.novo": "New code",
  "navegacao.parear.copiar": "Copy pairing link",
  "navegacao.parear.semHost": "This computer's host isn't connected.",
  "navegacao.parear.esteComputador": "this computer",
}
