// #753: carregar uma parte sob demanda sem derrubar a janela. Se o arquivo dela não carregar (ex.: sumiu num deploy da
// web), devolve a parte `indisponivel` em vez de rejeitar: um `lazy()` rejeitado derruba a raiz do React e a janela
// inteira some, com a conversa, o rascunho e os anexos na tela. O espaço da parte mostra o aviso com "Tentar de novo" e orienta recarregar se a falha continuar (não há recarga automática: ver docs/reference/web/shell.md).
export async function carregarOuIndisponivel<C>(carregar: () => Promise<C>, indisponivel: C): Promise<{ default: C }> {
  try {
    return { default: await carregar() }
  } catch {
    return { default: indisponivel }
  }
}
