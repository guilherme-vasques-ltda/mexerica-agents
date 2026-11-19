// Isolamento bidirecional onde só cabe texto (título, aria-label, linha de status, barra): o equivalente de
// `<bdi>`/`dir="ltr"` em caracteres Unicode. Sem ele, em árabe, um caminho, host ou código no meio da frase tem a ordem
// trocada na tela, e a pessoa lê (e autoriza) outra coisa. No DOM, prefira o elemento (`dir="ltr"` + `.bidi-isolado`,
// `<bdi>` para nome). Não use em texto que a pessoa copia para o terminal: o comando vai num elemento próprio.

/** Código, comando, caminho, host, e-mail, porta: sempre da esquerda para a direita e isolado (LRI … PDI). */
export const ltr = (valor: string | number): string => `⁦${valor}⁩`

/** Nome ou texto livre do usuário: direção do próprio texto e isolado da frase (FSI … PDI, como `dir="auto"`). */
export const isolado = (valor: string | number): string => `⁨${valor}⁩`

/**
 * Direção de um nome do usuário que não é frase (nome de arquivo, id): a dele (`auto`) se tem letra; só dígitos e
 * símbolos ("2026-11-08") não têm letra forte e herdariam a do pai, que em RTL inverte os blocos: `ltr`.
 */
export const direcaoDoNome = (valor: string): 'auto' | 'ltr' => (/\p{L}/u.test(valor) ? 'auto' : 'ltr')

