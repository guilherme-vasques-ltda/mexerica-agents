// Nova tentativa do pedido ao fornecedor. Só o pedido que o fornecedor recusou ANTES de qualquer processamento ou
// stream, na mesma conta e na mesma rota: a repetição acontece na abertura da resposta HTTP, então nada de resposta
// parcial, geração incerta ou ferramenta já executada é repetido, nenhuma conta é trocada e nenhuma rota nova é gasta.
// Resposta que começou a chegar nunca é refeita (o ajudante só olha o erro da abertura, não o corpo); cancelar o
// turno durante a espera encerra a repetição sem chamada nova (`abortableSleep` no ajudante).

/** Pedidos por turno: o primeiro e no máximo duas repetições. */
export const MAX_TENTATIVAS = 3;
/** Teto de cada espera pedida pelo fornecedor: acima disso o pedido falha na hora, com a mensagem do teto. */
export const ESPERA_MAXIMA_MS = 30_000;
/** Teto de espera do turno inteiro: as repetições são poucas e cada uma espera o que o fornecedor pediu, até o teto. */
export const TETO_DE_ESPERA_DO_TURNO_MS = (MAX_TENTATIVAS - 1) * ESPERA_MAXIMA_MS;

/** Recusa definitiva: repetir o mesmo pedido só gastaria uma chamada e devolveria o mesmo erro. 400 (pedido
 * inválido, inclusive estouro de contexto), 401/403 (credencial e autoridade), 404 (modelo ou rota inexistente),
 * 413 e 422 (tamanho e formato que o fornecedor não aceita). */
export const STATUS_SEM_REPETICAO: readonly number[] = [400, 401, 403, 404, 413, 422];

/** Opções do pedido ao fornecedor: o laço de repetição do Pi respeita `retry-after` dentro destes limites. */
export const OPCOES_DE_REPETICAO = {
	maxRetries: MAX_TENTATIVAS - 1,
	maxRetryDelayMs: ESPERA_MAXIMA_MS,
	noRetryStatuses: STATUS_SEM_REPETICAO,
} as const;
