// Stub de rede só para testes (--import): registra cada chamada fetch em
// MEXERICA_MOTOR_FETCH_LOG (JSONL com url, headers mascarados e corpo) e
// responde 401 ecoando a credencial recebida — como uma API que devolve a
// chave no corpo do erro. Assim as provas conferem destino, credencial (só o
// resumo), pedido e contagem de chamadas sem rede. MEXERICA_TEST_SUCCESS=1
// responde no formato Gemini; MEXERICA_TEST_SUCCESS_COMPLETIONS=1 responde no
// formato Chat Completions (SSE).
import { appendFileSync } from "node:fs";
import { createHash } from "node:crypto";

const log = process.env.MEXERICA_MOTOR_FETCH_LOG;
const calls = [];
// O log é artefato de saída: o VALOR do cabeçalho de credencial nunca entra nele, só o resumo que a prova confere.
const OCULTOS = ["authorization", "x-api-key", "x-goog-api-key"];
const ocultados = (headers) => Object.fromEntries(Object.entries(headers).map(([nome, valor]) => [nome, OCULTOS.includes(nome) ? `sha256-${createHash("sha256").update(String(valor)).digest("hex").slice(0, 16)}` : valor]));
globalThis.fetch = async (input, init) => {
	const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
	const headers = Object.fromEntries(new Headers(init?.headers ?? input?.headers ?? {}));
	const body = typeof init?.body === "string" ? init.body : undefined;
	calls.push({ url, headers });
	if (log) appendFileSync(log, JSON.stringify({ url, headers: ocultados(headers), body }) + "\n");
	const auth = headers.authorization ?? headers["x-goog-api-key"] ?? "sem-credencial";
	if (process.env.MEXERICA_TEST_STDERR_SPLIT === "1") {
		process.stderr.write(auth.slice(0, Math.floor(auth.length / 2)));
		process.stderr.write(auth.slice(Math.floor(auth.length / 2)));
	}
	if (process.env.MEXERICA_TEST_SUCCESS === "1") {
		const event = { candidates: [{ content: { role: "model", parts: [{ text: "resposta simulada" }] }, finishReason: "STOP", index: 0 }], usageMetadata: { promptTokenCount: 1, candidatesTokenCount: 1, totalTokenCount: 2 } };
		return new Response(`data: ${JSON.stringify(event)}\n\n`, { status: 200, headers: { "content-type": "text/event-stream" } });
	}
	if (process.env.MEXERICA_TEST_SUCCESS_COMPLETIONS === "1") {
		const uso = { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 };
		const eventos = [
			{ id: "chatcmpl-1", object: "chat.completion.chunk", created: 1, model: "modelo", choices: [{ index: 0, delta: { role: "assistant", content: "resposta simulada" }, finish_reason: null }] },
			{ id: "chatcmpl-1", object: "chat.completion.chunk", created: 1, model: "modelo", choices: [{ index: 0, delta: {}, finish_reason: "stop" }], usage: uso },
		];
		// Sem o tipo genérico: isto é um .mjs, lido como JavaScript puro.
		const bytes = new TextEncoder().encode(eventos.map((e) => `data: ${JSON.stringify(e)}\n\n`).join("") + "data: [DONE]\n\n");
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
