// Servidor de mentira no formato da API Responses, só para testes (--import): nada sai da máquina. O cenário vem de
// MEXERICA_TEST_CENARIO — ok, 429_duas_vezes, retry_longo, 401_de_retry, ferramenta_429, queda_de_conexao. Cada chamada (url, método,
// cabeçalhos e o corpo do pedido) vai para MEXERICA_MOTOR_FETCH_LOG, com a credencial só em resumo. Toda recusa repete
// a credencial recebida no corpo, como uma API que devolve a chave no erro: a saída do motor não pode contê-la.
import { appendFileSync } from "node:fs";
import { createHash } from "node:crypto";

const log = process.env.MEXERICA_MOTOR_FETCH_LOG;
const cenario = process.env.MEXERICA_TEST_CENARIO ?? "ok";
const OCULTOS = ["authorization", "x-api-key", "x-goog-api-key"];
const ocultados = (headers) =>
	Object.fromEntries(
		Object.entries(headers).map(([nome, valor]) => [
			nome,
			OCULTOS.includes(nome) ? `sha256-${createHash("sha256").update(String(valor)).digest("hex").slice(0, 16)}` : valor,
		]),
	);

const sse = (eventos) =>
	new Response(eventos.map((evento) => `data: ${JSON.stringify(evento)}\n\n`).join(""), {
		status: 200,
		headers: { "content-type": "text/event-stream" },
	});

const texto = (conteudo) =>
	sse([
		{ type: "response.created", response: { id: "resp-1", status: "in_progress", output: [] } },
		{ type: "response.output_item.added", output_index: 0, item: { type: "message", id: "msg-1", content: [] } },
		{ type: "response.output_text.delta", output_index: 0, delta: conteudo },
		{
			type: "response.output_item.done",
			output_index: 0,
			item: { type: "message", id: "msg-1", content: [{ type: "output_text", text: conteudo }] },
		},
		{
			type: "response.completed",
			response: {
				id: "resp-1",
				status: "completed",
				output: [],
				usage: { input_tokens: 3, output_tokens: 2, total_tokens: 5, input_tokens_details: { cached_tokens: 0 } },
			},
		},
	]);

const ferramenta = () =>
	sse([
		{ type: "response.created", response: { id: "resp-1", status: "in_progress", output: [] } },
		{
			type: "response.output_item.added",
			output_index: 0,
			item: { type: "function_call", id: "fc-1", call_id: "call-1", name: "efeito", arguments: "" },
		},
		{ type: "response.function_call_arguments.delta", output_index: 0, delta: '{"vezes":1}' },
		{
			type: "response.output_item.done",
			output_index: 0,
			item: { type: "function_call", id: "fc-1", call_id: "call-1", name: "efeito", arguments: '{"vezes":1}' },
		},
		{ type: "response.completed", response: { id: "resp-1", status: "completed", output: [] } },
	]);

let chamadas = 0;
globalThis.fetch = async (input, init) => {
	const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
	const method = (init?.method ?? input?.method ?? "GET").toUpperCase();
	const headers = Object.fromEntries(new Headers(init?.headers ?? input?.headers ?? {}));
	const body = typeof init?.body === "string" ? init.body : undefined;
	if (log) appendFileSync(log, JSON.stringify({ url, method, headers: ocultados(headers), body }) + "\n");
	const credencial = headers.authorization ?? headers["x-api-key"] ?? headers["x-goog-api-key"] ?? "sem-credencial";
	const recusa = (status, extras, motivo) =>
		new Response(JSON.stringify({ error: { message: `${motivo}: ${credencial}` } }), {
			status,
			headers: { "content-type": "application/json", ...extras },
		});
	chamadas += 1;
	switch (cenario) {
		case "429_duas_vezes":
			return chamadas <= 2 ? recusa(429, { "retry-after": "1" }, "limite de uso") : texto("resposta simulada");
		case "retry_longo":
			return recusa(429, { "retry-after": "120" }, "limite de uso");
		case "401_de_retry":
			return recusa(401, { "x-should-retry": "true" }, "chave recusada");
		case "ferramenta_429":
			if (chamadas === 1) return ferramenta();
			return chamadas === 2 ? recusa(429, { "retry-after": "1" }, "limite de uso") : texto("resposta simulada");
		// A conexão cai depois da saída do pedido: o fornecedor pode tê-lo processado.
		case "queda_de_conexao":
			throw new TypeError("fetch failed");
		default:
			return texto("resposta simulada");
	}
};
