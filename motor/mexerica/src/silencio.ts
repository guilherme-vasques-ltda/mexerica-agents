// Prazo de silêncio na leitura do fornecedor: se a resposta (cabeçalhos ou corpo) passa `prazoMs` sem chegar nenhum
// byte, o pedido é cortado com erro e o turno termina, em vez de esperar para sempre. Vale só para a conversa HTTP com
// o fornecedor: a espera por uma permissão do Hub acontece entre pedidos e não conta aqui.
type Fetch = typeof globalThis.fetch;

/** Prazo padrão: maior que a pausa de raciocínio de um modelo, menor que deixar o turno parado sem fim. */
export const SILENCIO_MS = 180_000;

export class SilencioDoFornecedor extends Error {
	constructor(prazoMs: number) {
		super(`o fornecedor ficou ${Math.round(prazoMs / 1000)} s sem responder; o pedido foi encerrado`);
		this.name = "SilencioDoFornecedor";
	}
}

export function comPrazoDeSilencio(base: Fetch, prazoMs = SILENCIO_MS): Fetch {
	return async (input, init) => {
		const corte = new AbortController();
		const externo = init?.signal ?? undefined;
		const repassar = () => corte.abort(externo?.reason);
		if (externo?.aborted) repassar();
		else externo?.addEventListener("abort", repassar, { once: true });
		let relogio: NodeJS.Timeout | undefined;
		const silencio = new SilencioDoFornecedor(prazoMs);
		const rearmar = () => {
			clearTimeout(relogio);
			relogio = setTimeout(() => corte.abort(silencio), prazoMs);
		};
		const soltar = () => {
			clearTimeout(relogio);
			externo?.removeEventListener("abort", repassar);
		};
		// Uma promessa que perde para o corte, mesmo que quem está do outro lado ignore o sinal.
		const ouCorte = <T>(promessa: Promise<T>): Promise<T> =>
			new Promise<T>((resolve, reject) => {
				const cortar = () => reject(corte.signal.reason);
				// A promessa recebida é sempre observada, mesmo quando o corte já aconteceu: rejeição solta derruba o Node.
				corte.signal.addEventListener("abort", cortar, { once: true });
				promessa.then(resolve, reject).finally(() => corte.signal.removeEventListener("abort", cortar));
				if (corte.signal.aborted) cortar();
			});

		rearmar();
		let resposta: Response;
		try {
			resposta = await ouCorte(base(input, { ...init, signal: corte.signal }));
		} catch (erro) {
			soltar();
			throw erro;
		}
		if (!resposta.body) {
			soltar();
			return resposta;
		}
		rearmar();
		const leitor = resposta.body.getReader();
		const corpo = new ReadableStream<Uint8Array>({
			async pull(controller) {
				try {
					const { done, value } = await ouCorte(leitor.read());
					if (done) {
						soltar();
						controller.close();
						return;
					}
					rearmar();
					controller.enqueue(value);
				} catch (erro) {
					soltar();
					leitor.cancel(erro).catch(() => {});
					controller.error(erro);
				}
			},
			cancel(motivo) {
				soltar();
				return leitor.cancel(motivo);
			},
		});
		return new Response(corpo, { status: resposta.status, statusText: resposta.statusText, headers: resposta.headers });
	};
}
