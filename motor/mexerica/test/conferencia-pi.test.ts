// Conferência da cópia do Pi (API Responses) com rede falsa: nada sai da máquina e nenhuma chave real é usada.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Agent } from "@earendil-works/pi-agent-core";
import { normalizeContext, type AssistantMessage, type AssistantMessageEvent, type Context, type Model } from "@earendil-works/pi-ai";
import { streamSimple } from "@earendil-works/pi-ai/api/openai-responses.ts";

const MODEL: Model<"openai-responses"> = {
	id: "gpt-5",
	name: "GPT-5",
	api: "openai-responses",
	provider: "openai",
	baseUrl: "https://api.example.invalid/v1",
	input: ["text"],
	reasoning: true,
	cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
	contextWindow: 400000,
	maxTokens: 128000,
};

type Evento = Record<string, unknown>;

const USO = { input_tokens: 1, output_tokens: 1, total_tokens: 2, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } };

/** Uma resposta de texto em eventos da API Responses; `delta: null` omite o campo do trecho. */
function eventosDeTexto(opts: { texto?: string; faseNoInicio?: string; faseNoFim?: string; delta?: string | null } = {}): Evento[] {
	const texto = opts.texto ?? "olá";
	const fim = { type: "message", id: "msg_1", role: "assistant", status: "completed", content: [{ type: "output_text", text: texto, annotations: [] }], ...(opts.faseNoFim ? { phase: opts.faseNoFim } : {}) };
	const delta = opts.delta === null ? {} : { delta: opts.delta ?? texto };
	return [
		{ type: "response.created", response: { id: "resp_1", status: "in_progress", output: [] } },
		{ type: "response.output_item.added", output_index: 0, item: { type: "message", id: "msg_1", role: "assistant", status: "in_progress", content: [], ...(opts.faseNoInicio ? { phase: opts.faseNoInicio } : {}) } },
		{ type: "response.output_text.delta", output_index: 0, content_index: 0, item_id: "msg_1", ...delta },
		{ type: "response.output_item.done", output_index: 0, item: fim },
		{ type: "response.completed", response: { id: "resp_1", status: "completed", output: [fim], usage: USO } },
	];
}

/** Corpo SSE; com `fecharConexao: false` o servidor manda tudo e nunca fecha, como um proxy que segura a conexão. */
function respostaSse(eventos: Evento[], fecharConexao = true): Response {
	const bytes = new TextEncoder().encode(eventos.map((e) => `data: ${JSON.stringify(e)}\n\n`).join(""));
	const body = new ReadableStream<Uint8Array>({
		start(controller) {
			controller.enqueue(bytes);
			if (fecharConexao) controller.close();
		},
	});
	return new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } });
}

const contexto = (messages: Context["messages"]) => normalizeContext({ systemPrompt: "Você é um bot.", messages });
const usuario = (text: string) => ({ role: "user" as const, content: text, timestamp: Date.now() });

async function rodar(fetch: typeof globalThis.fetch, messages: Context["messages"], signal?: AbortSignal) {
	const eventos: AssistantMessageEvent[] = [];
	const stream = streamSimple(MODEL, contexto(messages), { apiKey: "chave-ficticia-de-teste", fetch, signal });
	for await (const evento of stream) eventos.push(evento);
	return { eventos, mensagem: (await stream.result()) as AssistantMessage };
}

test("A7: resposta termina no evento final mesmo se a conexão não fechar", async () => {
	const parar = new AbortController();
	const prazo = new Promise<"pendurou">((resolve) => setTimeout(() => resolve("pendurou"), 3000).unref());
	const fetch = (async () => respostaSse(eventosDeTexto(), false)) as typeof globalThis.fetch;
	const resultado = await Promise.race([rodar(fetch, [usuario("oi")], parar.signal), prazo]);
	parar.abort();
	assert.notEqual(resultado, "pendurou", "a resposta ficou pendurada depois do evento final");
	const { mensagem } = resultado as Awaited<ReturnType<typeof rodar>>;
	assert.equal(mensagem.stopReason, "stop");
	assert.deepEqual(mensagem.content.map((b) => (b.type === "text" ? b.text : b.type)), ["olá"]);
});


test("A6: trecho sem o campo de texto não vira a palavra «undefined»", async () => {
	const fetch = (async () => respostaSse(eventosDeTexto({ delta: null }))) as typeof globalThis.fetch;
	const { eventos, mensagem } = await rodar(fetch, [usuario("oi")]);
	const parciais = eventos.flatMap((e) => (e.type === "text_delta" ? [String(e.delta), JSON.stringify(e.partial.content)] : []));
	assert.ok(parciais.every((p) => !p.includes("undefined")), `texto «undefined» no meio da resposta: ${parciais.join(" | ")}`);
	assert.deepEqual(mensagem.content.map((b) => (b.type === "text" ? b.text : b.type)), ["olá"]);
});

test("A8: a fase do texto anunciada no início sobrevive ao replay", async () => {
	const corpos: unknown[] = [];
	let chamada = 0;
	const fetch = (async (_input: unknown, init?: RequestInit) => {
		corpos.push(JSON.parse(String(init?.body)));
		return respostaSse(chamada++ === 0 ? eventosDeTexto({ faseNoInicio: "final_answer" }) : eventosDeTexto({ texto: "de novo" }));
	}) as typeof globalThis.fetch;
	const primeira = await rodar(fetch, [usuario("oi")]);
	await rodar(fetch, [usuario("oi"), primeira.mensagem, usuario("e agora?")]);
	const entrada = (corpos[1] as { input: Evento[] }).input;
	const resposta = entrada.find((item) => item["type"] === "message" && item["role"] === "assistant");
	assert.equal(resposta?.["phase"], "final_answer", `fase perdida no replay: ${JSON.stringify(resposta)}`);
});

test("A5/G2: falha ao preparar o pedido vira erro do turno e o motor segue respondendo", async () => {
	let chamadas = 0;
	const fetch = (async () => {
		chamadas++;
		return respostaSse(eventosDeTexto());
	}) as typeof globalThis.fetch;
	const agent = new Agent({ streamFn: (model, context, options) => streamSimple(model as Model<"openai-responses">, context, { ...options, apiKey: "chave-ficticia-de-teste", fetch }) });
	agent.state.model = MODEL;
	// Histórico malformado: o adaptador quebra antes de qualquer rede.
	agent.state.messages = [{ role: "user", content: null, timestamp: 1 } as unknown as Context["messages"][number]];
	await agent.prompt("oi");
	const falha = agent.state.messages.at(-1) as AssistantMessage;
	assert.equal(falha.stopReason, "error");
	assert.ok(falha.errorMessage, "o erro do preparo tem de chegar como mensagem");
	assert.equal(chamadas, 0, "nenhum pedido sai com o contexto inválido");
	assert.equal(agent.state.isStreaming, false);

	agent.state.messages = [];
	await agent.prompt("de novo");
	const certa = agent.state.messages.at(-1) as AssistantMessage;
	assert.equal(certa.stopReason, "stop");
	assert.equal(chamadas, 1);
});

