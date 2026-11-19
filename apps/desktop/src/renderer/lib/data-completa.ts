// Auditoria D249, F8: a data completa da dica da mensagem em pt-BR, como o resto do app,
// e não no idioma do sistema ("4 de nov. de 2026, 00:41").
const formato = new Intl.DateTimeFormat('pt-BR', { dateStyle: 'medium', timeStyle: 'short' })

export function dataCompleta(at: number): string {
  return formato.format(at)
}
