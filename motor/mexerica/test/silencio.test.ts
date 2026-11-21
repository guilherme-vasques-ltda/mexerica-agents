// Prazo de silêncio do fornecedor com rede falsa.
import assert from "node:assert/strict";
import { test } from "node:test";
import { normalizeContext, type AssistantMessage, type Model } from "@earendil-works/pi-ai";
import { streamSimple } from "@earendil-works/pi-ai/api/openai-responses.ts";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { comPrazoDeSilencio, semRedirecionar, SilencioDoFornecedor } from "../src/silencio.ts";

const MODEL: Model<"openai-responses"> = {
	id: "gpt-5", name: "GPT-5", api: "openai-responses", provider: "openai", baseUrl: "https://api.example.invalid/v1",
	input: ["text"], reasoning: true, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 400000, maxTokens: 128000,
};
const sse = (eventos: object[]) => new TextEncoder().encode(eventos.map((e) => `data: ${JSON.stringify(e)}\n\n`).join(""));
const nunca = () => new Promise<never>(() => {});
const corpoQueTrava = (inicio: Uint8Array) =>
	new ReadableStream<Uint8Array>({
		start(controller) {
			controller.enqueue(inicio);
		},
	});

async function turno(fetch: typeof globalThis.fetch, signal?: AbortSignal): Promise<AssistantMessage> {
	const stream = streamSimple(MODEL, normalizeContext({ messages: [{ role: "user", content: "oi", timestamp: 1 }] }), { apiKey: "chave-ficticia-de-teste", fetch, signal });
	return (await stream.result()) as AssistantMessage;
}

test("corpo que para no meio encerra o turno com erro de silêncio", async () => {
	const inicio = sse([{ type: "response.created", response: { id: "r", status: "in_progress", output: [] } }]);
	const fetch = comPrazoDeSilencio(async () => new Response(corpoQueTrava(inicio), { status: 200, headers: { "content-type": "text/event-stream" } }), 200);
	const antes = Date.now();
	const mensagem = await turno(fetch);
	assert.equal(mensagem.stopReason, "error");
	assert.match(mensagem.errorMessage ?? "", /sem responder/);
	assert.ok(Date.now() - antes < 5000);
});

test("fornecedor que nem responde os cabeçalhos também é cortado", async () => {
	const fetch = comPrazoDeSilencio(nunca, 200);
	await assert.rejects(fetch("https://api.example.invalid/v1/responses"), SilencioDoFornecedor);
});

test("cancelar pelo sinal de um Request, sem init, cancela o pedido de baixo", async () => {
	let recebido: AbortSignal | null | undefined;
	const fetch = comPrazoDeSilencio((_input, init) => {
		recebido = init?.signal;
		return new Promise<Response>((_resolve, reject) => init?.signal?.addEventListener("abort", () => reject(init.signal?.reason)));
	}, 5000);
	const dono = new AbortController();
	const pedido = fetch(new Request("https://api.example.invalid/v1/responses", { signal: dono.signal }));
	dono.abort(new Error("cancelado pelo dono"));
	await assert.rejects(pedido, /cancelado pelo dono/);
	assert.equal(recebido?.aborted, true);
});

test("sair sem drenar o corpo, cancelando de fora, não deixa o prazo armado", async () => {
	const ativos = new Set<unknown>();
	const { setTimeout: armar, clearTimeout: desarmar } = globalThis;
	globalThis.setTimeout = ((...args: Parameters<typeof setTimeout>) => {
		const relogio = armar(...args);
		ativos.add(relogio);
		return relogio;
	}) as typeof setTimeout;
	globalThis.clearTimeout = ((relogio: Parameters<typeof clearTimeout>[0]) => {
		ativos.delete(relogio);
		desarmar(relogio);
	}) as typeof clearTimeout;
	try {
		const inicio = new TextEncoder().encode("data: {}\n\n");
		const fetch = comPrazoDeSilencio(async () => new Response(corpoQueTrava(inicio), { status: 200 }), 60_000);
		const dono = new AbortController();
		const resposta = await fetch("https://api.example.invalid/v1/responses", { signal: dono.signal });
		const leitor = resposta.body!.getReader();
		await leitor.read();
		assert.equal(ativos.size, 1, "o prazo do silêncio está armado enquanto a resposta corre");
		dono.abort();
		assert.equal(ativos.size, 0, "cancelar de fora desarma o prazo");
	} finally {
		globalThis.setTimeout = armar;
		globalThis.clearTimeout = desarmar;
	}
});

test("resposta lenta mas viva não é cortada", async () => {
	const partes = [
		{ type: "response.created", response: { id: "r", status: "in_progress", output: [] } },
		{ type: "response.output_item.added", output_index: 0, item: { type: "message", id: "m", role: "assistant", status: "in_progress", content: [] } },
		{ type: "response.output_text.delta", output_index: 0, content_index: 0, item_id: "m", delta: "devagar" },
		{ type: "response.output_item.done", output_index: 0, item: { type: "message", id: "m", role: "assistant", status: "completed", content: [{ type: "output_text", text: "devagar", annotations: [] }] } },
		{ type: "response.completed", response: { id: "r", status: "completed", output: [], usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2 } } },
	];
	const corpo = new ReadableStream<Uint8Array>({
		async start(controller) {
			for (const parte of partes) {
				await new Promise((r) => setTimeout(r, 120));
				controller.enqueue(sse([parte]));
			}
			controller.close();
		},
	});
	const fetch = comPrazoDeSilencio(async () => new Response(corpo, { status: 200, headers: { "content-type": "text/event-stream" } }), 300);
	const mensagem = await turno(fetch);
	assert.equal(mensagem.stopReason, "stop", mensagem.errorMessage);
});

test("cancelar continua sendo cancelar, não silêncio", async () => {
	const parar = new AbortController();
	const fetch = comPrazoDeSilencio((_i, init) => new Promise((_r, reject) => init?.signal?.addEventListener("abort", () => reject(init.signal?.reason))), 10_000);
	const pedido = fetch("https://api.example.invalid/v1/responses", { signal: parar.signal });
	parar.abort(new Error("cancelado pelo usuário"));
	await assert.rejects(pedido, /cancelado pelo usuário/);
});


test("pedido já cancelado não deixa rejeição solta", async () => {
	const soltas: unknown[] = [];
	const anotar = (motivo: unknown) => soltas.push(motivo);
	process.on("unhandledRejection", anotar);
	try {
		const parar = new AbortController();
		parar.abort(new Error("cancelado antes de começar"));
		const fetch = comPrazoDeSilencio(async () => {
			throw new Error("o fetch recusou o sinal cancelado");
		}, 10_000);
		await assert.rejects(fetch("https://api.example.invalid/v1/responses", { signal: parar.signal }), /cancelado antes de começar/);
		await new Promise((resolve) => setTimeout(resolve, 20));
		assert.deepEqual(soltas, []);
	} finally {
		process.off("unhandledRejection", anotar);
	}
});

const ouvir = (servidor: Server) =>
	new Promise<number>((resolve) => servidor.listen(0, "127.0.0.1", () => resolve((servidor.address() as AddressInfo).port)));

test("redirecionamento nunca leva a chave a outro endereço", async () => {
	const recebidos: (string | undefined)[] = [];
	const outro = createServer((pedido, resposta) => {
		recebidos.push(pedido.headers["x-api-key"] as string | undefined);
		resposta.end("{}");
	});
	const portaOutro = await ouvir(outro);
	const fornecedor = createServer((_pedido, resposta) => {
		resposta.writeHead(307, { location: `http://127.0.0.1:${portaOutro}/roubar` });
		resposta.end();
	});
	const porta = await ouvir(fornecedor);
	try {
		const url = `http://127.0.0.1:${porta}/v1/messages`;
		const init = { method: "POST", headers: { "x-api-key": "chave-ficticia-de-teste" }, body: "{}" };
		// Sem a trava, o fetch segue o 307 e manda o cabeçalho da chave ao outro endereço.
		await globalThis.fetch(url, init);
		assert.deepEqual(recebidos, ["chave-ficticia-de-teste"]);
		recebidos.length = 0;
		await assert.rejects(comPrazoDeSilencio(semRedirecionar, 2000)(url, init));
		assert.deepEqual(recebidos, [], "o pedido com a chave não pode chegar ao outro endereço");
	} finally {
		fornecedor.close();
		outro.close();
	}
});
