// Nova tentativa do pedido ao fornecedor. Só o pedido que o fornecedor recusou ANTES de qualquer processamento ou
// stream, na mesma conta e na mesma rota: assim nada de resposta parcial, ferramenta já executada ou troca de conta é
// repetido, e nenhuma rota nova é gasta. Resposta que começou a chegar nunca é refeita (o ajudante do Pi só olha a
// abertura da resposta, não o corpo); erro de transporte sem status HTTP também não: a conexão pode ter caído depois de
// o fornecedor receber o pedido, e repetir ali duplicaria geração e cobrança. Cancelar o turno durante a espera encerra
// a repetição sem chamada nova (`abortableSleep` no ajudante).

/** Pedidos por turno: o primeiro e no máximo duas repetições, em cada chamada ao fornecedor. */
export const MAX_TENTATIVAS = 3;
/** Teto de cada espera pedida pelo fornecedor: acima disso o pedido falha na hora, com a mensagem do teto. */
export const ESPERA_MAXIMA_MS = 30_000;
/** Teto de espera do turno inteiro, somando todas as chamadas dele (uma volta de ferramentas gasta o mesmo orçamento). */
export const TETO_DE_ESPERA_DO_TURNO_MS = (MAX_TENTATIVAS - 1) * ESPERA_MAXIMA_MS;

/** Recusa que o fornecedor usa para dizer que NÃO aceitou o pedido: 408 (ele desistiu de esperar), 409 (conflito) e
 * 429 (limite de uso). É a lista inteira: 4xx de pedido inválido, credencial, autoridade, modelo, tamanho e formato, e
 * qualquer 5xx, ficam de fora. Um 5xx pode vir de um pedido que o fornecedor já começou a processar, e repetir ali
 * duplicaria geração e cobrança. */
export const STATUS_DE_REPETICAO: readonly number[] = [408, 409, 429];

/** Opções de cada pedido ao fornecedor: o laço de repetição do Pi respeita `retry-after` dentro destes limites. */
export const OPCOES_DE_REPETICAO = {
	maxRetries: MAX_TENTATIVAS - 1,
	maxRetryDelayMs: ESPERA_MAXIMA_MS,
	retryStatuses: STATUS_DE_REPETICAO,
	// Erro sem status não prova que o fornecedor recusou antes de processar: não repete.
	retryWithoutStatus: false,
} as const;

/** Orçamento de espera de um turno: um objeto por prompt, compartilhado por todas as chamadas dele. */
export function orcamentoDoTurno(): { remainingMs: number } {
	return { remainingMs: TETO_DE_ESPERA_DO_TURNO_MS };
}
