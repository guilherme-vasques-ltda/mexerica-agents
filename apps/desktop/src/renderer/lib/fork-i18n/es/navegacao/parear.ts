import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/parear.ts'

// Fatia 1 (D454): navegacao/parear (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.parear.qr": "Código de emparejamiento de {computador}",
  "navegacao.parear.semRede": "No se encontró ninguna dirección de red y “Acceder desde cualquier lugar” está desactivado. Conéctate a una Wi-Fi o actívalo.",
  "navegacao.parear.mesmaRede": "“Acceder desde cualquier lugar” está desactivado, así que el iPhone solo llega a {computador} en la misma red.",
  "navegacao.parear.valeAte": "Vale una vez, hasta las {hora}",
  "navegacao.parear.novo": "Nuevo código",
  "navegacao.parear.copiar": "Copiar enlace de emparejamiento",
  "navegacao.parear.semHost": "El host de este ordenador no está conectado.",
  "navegacao.parear.esteComputador": "este ordenador",
}
