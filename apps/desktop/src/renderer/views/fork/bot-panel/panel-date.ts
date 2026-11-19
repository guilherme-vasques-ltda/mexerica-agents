/** Datas das notas no painel, sem alterar a formatação do chat ou da lista de bots. */
export const panelDate = (at: number) => new Date(at).toLocaleDateString('pt-BR', { day: 'numeric', month: 'short', year: 'numeric' })
