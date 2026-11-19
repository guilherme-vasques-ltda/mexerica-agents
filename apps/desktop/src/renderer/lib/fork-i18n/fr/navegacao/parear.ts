import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/parear.ts'

// Fatia 1 (D454): navegacao/parear (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.parear.qr": "Code d’appairage pour {computador}",
  "navegacao.parear.semRede": "Aucune adresse réseau trouvée et « Accès de partout » est désactivé. Connectez-vous au Wi-Fi ou activez l'option.",
  "navegacao.parear.mesmaRede": "« Accès de partout » est désactivé : l’iPhone n’atteint {computador} que sur le même réseau.",
  "navegacao.parear.valeAte": "Valable une fois, jusqu’à {hora}",
  "navegacao.parear.novo": "Nouveau code",
  "navegacao.parear.copiar": "Copier le lien d’appairage",
  "navegacao.parear.semHost": "L'hôte de cet ordinateur n'est pas connecté.",
  "navegacao.parear.esteComputador": "cet ordinateur",
}
