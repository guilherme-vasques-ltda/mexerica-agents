import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/ilha.ts'

// Fatia 1 (D454): navegacao/ilha (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.ilha.botAssistente": "{bot}, bot assistant",
  "navegacao.ilha.abrirConversa": "Ouvrir la conversation",
  "navegacao.ilha.nadaMostrar": "Aucun travail à afficher.",
  "navegacao.ilha.nadaRodando": "Rien en cours pour l'instant.",
  "navegacao.ilha.indisponiveis": "Certains bots ont un état indisponible.",
  "navegacao.ilha.mensagemBot": "Écrire à un bot",
  "navegacao.ilha.terminou": "{motor} · Terminé",
  "navegacao.ilha.parou": "{bot} s’est arrêté.",
  "navegacao.ilha.outroBot": "Choisir un autre bot",
  "navegacao.ilha.mensagemPara": "Message à {bot}",
  "navegacao.ilha.mensagemParaPlaceholder": "Message à {bot}…",
  "navegacao.ilha.enviar": "Envoyer",
  "navegacao.ilha.mostrarBots": "Afficher les bots",
  "navegacao.ilha.aba.bots": "Bots",
  "navegacao.ilha.trabalhandoBadge": "Au travail",
  "navegacao.ilha.pronto": "Terminé",
  "navegacao.ilha.falhou": "Échec",
  "navegacao.ilha.abrir": "Ouvrir {bot}",
  "navegacao.ilha.usoAria": "Utilisation : {provedor} {janela} {pct} pour cent",
  "navegacao.ilha.semUso": "Aucune utilisation pour l'instant.",
  "navegacao.ilha.semUsoDetalhe": "Les limites de Claude Code et de Codex s'affichent ici dès qu'un bot les utilise.",
  "navegacao.ilha.ok": "OK",
  "navegacao.ilha.trabalhando.one": "{n} au travail",
  "navegacao.ilha.trabalhando.many": "{n} au travail",
  "navegacao.ilha.trabalhando.other": "{n} au travail",
}
