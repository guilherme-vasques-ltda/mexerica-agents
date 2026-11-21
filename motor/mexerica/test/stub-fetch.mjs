// Stub de rede só para testes (--import): registra cada chamada fetch em
// MEXERICA_MOTOR_FETCH_LOG (JSONL com url, headers e corpo) e responde 401
// ecoando a credencial recebida — como uma API que devolve a chave no corpo do
// erro. Assim as provas conferem destino, credencial, pedido e contagem de
// chamadas sem rede. MEXERICA_TEST_SUCCESS=1 responde no formato Gemini;
// MEXERICA_TEST_SUCCESS_RESPONSES=1 responde no formato Responses (SSE).
import { appendFileSync } from "node:fs";

const log = process.env.MEXERICA_MOTOR_FETCH_LOG;
const calls = [];
globalThis.fetch = async (input, init) => {
	const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
	const headers = Object.fromEntries(new Headers(init?.headers ?? input?.headers ?? {}));
	const body = typeof init?.body === "string" ? init.body : undefined;
	calls.push({ url, headers });
	if (log) appendFileSync(log, JSON.stringify({ url, headers, body }) + "\n");
	const auth = headers.authorization ?? headers["x-goog-api-key"] ?? "sem-credencial";
	if (process.env.MEXERICA_TEST_STDERR_SPLIT === "1") {
		process.stderr.write(auth.slice(0, Math.floor(auth.length / 2)));
		process.stderr.write(auth.slice(Math.floor(auth.length / 2)));
	}
	if (process.env.MEXERICA_TEST_SUCCESS === "1") {
		const event = { candidates: [{ content: { role: "model", parts: [{ text: "resposta simulada" }] }, finishReason: "STOP", index: 0 }], usageMetadata: { promptTokenCount: 1, candidatesTokenCount: 1, totalTokenCount: 2 } };
		return new Response(`data: ${JSON.stringify(event)}\n\n`, { status: 200, headers: { "content-type": "text/event-stream" } });
	}
	if (process.env.MEXERICA_TEST_SUCCESS_RESPONSES === "1") {
		const uso = { input_tokens: 1, output_tokens: 1, total_tokens: 2, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } };
		const fim = { type: "message", id: "msg_1", role: "assistant", status: "completed", content: [{ type: "output_text", text: "resposta simulada", annotations: [] }] };
		const eventos = [
			{ type: "response.created", response: { id: "resp_1", status: "in_progress", output: [] } },
			{ type: "response.output_item.added", output_index: 0, item: { type: "message", id: "msg_1", role: "assistant", status: "in_progress", content: [] } },
			{ type: "response.output_text.delta", output_index: 0, content_index: 0, item_id: "msg_1", delta: "resposta simulada" },
			{ type: "response.output_item.done", output_index: 0, item: fim },
			{ type: "response.completed", response: { id: "resp_1", status: "completed", output: [fim], usage: uso } },
		];
		// Sem o tipo genérico: isto é um .mjs, lido como JavaScript puro.
		const bytes = new TextEncoder().encode(eventos.map((e) => `data: ${JSON.stringify(e)}\n\n`).join(""));
		const stream = new ReadableStream({
			start(controller) {
				controller.enqueue(bytes);
				controller.close();
			},
		});
		return new Response(stream, { status: 200, headers: { "content-type": "text/event-stream" } });
	}
	return new Response(JSON.stringify({ error: { message: `chave recusada: ${auth}` } }), { status: 401 });
};
