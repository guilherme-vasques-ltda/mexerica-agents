// As chaves do estado de Lugares no pai (estado-pagina.ts). Lugares monta um bloco por Hub ao mesmo tempo: o Hub
// entra em toda chave, para um Hub nunca mostrar o rascunho ou a seleção do outro. Sem tipo de React: testável.

/** A lugar selecionado neste Hub. */
export const chaveSelecionado = (hubId: string) => `lugares/${hubId}/selecionado`

/** O rascunho de um lugar deste Hub. */
export const chaveRascunho = (hubId: string, placeId: string) => `lugares/${hubId}/rascunho/${placeId}`

/** O nome ou o recurso digitados no cadastro deste Hub. */
export const chaveCadastro = (hubId: string, campo: 'nome' | 'recurso') => `lugares/${hubId}/cadastrar/${campo}`
