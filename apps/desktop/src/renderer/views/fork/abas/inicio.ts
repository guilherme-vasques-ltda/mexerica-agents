/** Estado inicial da barra de sessões: sem sessões nasce retraída; a escolha salva do usuário (true/false) sempre vence. */
export function tabsStartExpanded(sessions: number, saved: boolean | null): boolean {
  return saved ?? sessions > 0
}
