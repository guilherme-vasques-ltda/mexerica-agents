import { pref } from '../../../lib/prefs'

/** Barra05: escolhas do painel direito e das abas, guardadas no aparelho. */
export const panelPrefs = {
  /** Painel fixo (empurra o chat); solto, abre ao passar o mouse na borda. */
  pinned: pref('rightPanelPinned', false, (v): v is boolean => typeof v === 'boolean'),
  /** O "Agora" aberto ou recolhido. */
  nowOpen: pref('rightPanelNowOpen', true, (v): v is boolean => typeof v === 'boolean'),
  /** As abas de sessão à mostra (‹‹) ou recolhidas na pílula do bot (››); null = o usuário ainda não escolheu (abas/inicio.ts). */
  tabsOpen: pref<boolean | null>('chatTabsOpen', null, (v): v is boolean => typeof v === 'boolean'),
}

/** Largura do painel fixo e do solto (protótipo: 332 px). */
export const PANEL_WIDTH = 332
