// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { UpdateState } from './ipc'
import type { AppLanguage } from './fork-idioma.ts'
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'

/** The one update action every surface offers (Settings, the update card, the menu bar), in the app's language (D454). */
export function updateAction(u: UpdateState, idioma: AppLanguage): { label: string; enabled: boolean } {
  if (u.installing) return { label: traduzir(idioma, 'sistema.barra.reiniciando'), enabled: false }
  if (u.staged) return { label: traduzir(idioma, 'sistema.barra.reiniciarAtualizar'), enabled: true }
  if (u.downloadPercent !== null) return { label: traduzir(idioma, 'sistema.barra.baixando', { pct: u.downloadPercent }), enabled: false }
  if (u.checking) return { label: traduzir(idioma, 'sistema.barra.procurando'), enabled: false }
  return { label: u.availableVersion ? traduzir(idioma, 'sistema.barra.atualizarPara', { versao: u.availableVersion }) : traduzir(idioma, 'sistema.barra.procurarAtualizacoes'), enabled: u.supported }
}
