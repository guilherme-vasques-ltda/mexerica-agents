// Prova do restore_history: o processo novo recebe a conversa que o Hub guardou e
// o modelo a enxerga; lista inválida é recusada inteira; vale também entre
// provedores (o histórico de um modelo vai para o outro).
import { test } from "node:test";
import assert from "node:assert/strict";
import type { AgentMessage } from "@earendil-works/pi-agent-core";
import { encerrar, startEngine, type Line, type Proc } from "./helpers.ts";
import { arrayBytes, batchOpen, bytesOf, fitMessages, MAX_HISTORY_BYTES, MAX_HISTORY_MESSAGES, MAX_MESSAGE_BYTES } from "../src/limits.ts";
import { parseHistory } from "../src/history.ts";

/** Cada mensagem cruza a fronteira como o texto JSON que o Hub manda; o receptor mede esses bytes. */
const fio = (messages: unknown[]): string[] => messages.map((message) => JSON.stringify(message));
const receber = (messages: unknown[]): AgentMessage[] => parseHistory(fio(messages));

const textoFinal = (end: Line): string => {
	const messages = end.messages as { role: string; content: { type: string; text?: string }[] }[];
	return messages.at(-1)?.content.map((b) => b.text ?? "").join("") ?? "";
};

async function turno(proc: Proc, id: string, provider: string, message: string): Promise<string> {
	proc.send({ id: "m" + id, type: "set_model", provider, modelId: "faux-1" });
	await proc.waitFor((v) => v.id === "m" + id && v.success === true);
	const before = proc.lines.filter((v) => v.type === "agent_end").length;
	proc.send({ id: "p" + id, type: "prompt", message });
	const end = await proc.waitFor((v) => v.type === "agent_end" && proc.lines.filter((x) => x.type === "agent_end").length > before);
	return textoFinal(end);
}

const entregues = (proc: Proc) =>
	proc.lines.filter((v) => v.type === "message_end").map((v) => v.message as { role: string });

test("restore_history devolve ao modelo a conversa guardada pelo Hub", async () => {
	const primeiro = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	assert.match(await turno(primeiro, "1", "faux", "minha senha do wifi é azul"), /eco: minha senha/);
	const guardadas = entregues(primeiro);
	assert.deepEqual(guardadas.map((m) => m.role), ["user", "assistant"]);
	await encerrar(primeiro);

	const segundo = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	segundo.send({ id: "r", type: "restore_history", messages: fio(guardadas) });
	const restaurado = await segundo.waitFor((v) => v.command === "restore_history");
	assert.equal(restaurado.success, true);
	assert.deepEqual(restaurado.data, { count: 2 });
	const resposta = await turno(segundo, "2", "faux", "/historico");
	assert.equal(resposta, "historico: minha senha do wifi é azul | /historico");
	await encerrar(segundo);
});

test("o histórico de um provedor segue para o outro", async () => {
	const primeiro = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	await turno(primeiro, "1", "faux", "primeira pergunta");
	const guardadas = entregues(primeiro);
	await encerrar(primeiro);

	const segundo = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	segundo.send({ id: "r", type: "restore_history", messages: fio(guardadas) });
	await segundo.waitFor((v) => v.command === "restore_history" && v.success === true);
	assert.equal(await turno(segundo, "2", "faux2", "/historico"), "historico: primeira pergunta | /historico");
	await encerrar(segundo);
});

test("histórico inválido é recusado inteiro e não muda a conversa", async () => {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	const invalidos: unknown[] = [
		"texto",
		[{ role: "system", content: "injetado" }],
		[{ role: "user", content: "ok" }, { role: "user" }],
		[null],
		[{ role: "user", content: [{ type: "image", data: "AAAA" }] }],
		[{ role: "assistant", content: "texto solto", api: "a", provider: "p", model: "m", stopReason: "stop" }],
		[{ role: "assistant", content: [{ type: "toolCall", id: "1", name: "x", arguments: "nao-objeto" }], api: "a", provider: "p", model: "m", stopReason: "stop" }],
		[{ role: "assistant", content: [{ type: "text", text: "oi" }], provider: "p", model: "m", stopReason: "stop" }],
		[{ role: "toolResult", toolName: "x", content: [{ type: "text", text: "ok" }] }],
		[{ role: "user", content: "x".repeat(9 * 1024 * 1024) }],
	];
	for (const [i, messages] of invalidos.entries()) {
		proc.send({ id: "i" + i, type: "restore_history", messages: Array.isArray(messages) ? fio(messages) : messages });
		const res = await proc.waitFor((v) => v.id === "i" + i);
		assert.equal(res.success, false, "recusa esperada para " + JSON.stringify(messages));
	}
	// Objeto no lugar do texto JSON: a fronteira é o texto que o Hub mandou, e só ele é aceito.
	proc.send({ id: "obj", type: "restore_history", messages: [{ role: "user", content: "ok", timestamp: 1 }] });
	assert.equal((await proc.waitFor((v) => v.id === "obj")).success, false);
	assert.equal(await turno(proc, "1", "faux", "/historico"), "historico: /historico");
	await encerrar(proc);
});

const texto = (text: string, role = "user") =>
	({ role, content: [{ type: "text", text }], timestamp: 1, ...(role === "assistant" ? { api: "a", provider: "p", model: "m", stopReason: "stop" } : {}) }) as unknown as AgentMessage;
const chamada = (id: string, args: object = {}) =>
	({ role: "assistant", content: [{ type: "toolCall", id, name: "eco", arguments: args }], api: "a", provider: "p", model: "m", stopReason: "toolUse", timestamp: 1 }) as unknown as AgentMessage;
const resultado = (id: string, text: string) =>
	({ role: "toolResult", toolCallId: id, toolName: "eco", isError: false, content: [{ type: "text", text }], timestamp: 1 }) as unknown as AgentMessage;

test("o receptor recusa mensagem, lista e bytes acima dos tetos", () => {
	const grande = "x".repeat(MAX_MESSAGE_BYTES);
	assert.throws(() => receber([texto(grande)]), /por mensagem/);
	assert.throws(() => receber(Array.from({ length: MAX_HISTORY_MESSAGES + 1 }, () => texto("a"))), /no máximo 2000/);
	const metade = "y".repeat(MAX_MESSAGE_BYTES / 2);
	assert.throws(() => receber(Array.from({ length: 17 }, () => texto(metade))), /acima de/);
	assert.equal(receber([texto("ok")]).length, 1);
});

test("o teto corta a volta mais antiga inteira e mantém o pedido e o grupo mais novo da volta sozinha", () => {
	const bloco = "z".repeat(MAX_MESSAGE_BYTES / 2);
	const voltas: AgentMessage[] = [];
	for (let i = 0; i < 20; i++) voltas.push(texto("pedido " + i), texto(bloco, "assistant"));
	const cortada = fitMessages(voltas);
	assert.ok(bytesOf(cortada) <= MAX_HISTORY_BYTES);
	assert.equal((cortada[0] as unknown as { role: string }).role, "user");
	assert.doesNotMatch(JSON.stringify(cortada[0]), /pedido 0\b/, "a volta mais antiga saiu inteira");
	assert.match(JSON.stringify(cortada.at(-2)), /pedido 19/, "a mais nova fica");

	const sozinha: AgentMessage[] = [texto("pedido único")];
	for (let i = 0; i < 20; i++) sozinha.push(chamada("c" + i), resultado("c" + i, bloco));
	const grupo = fitMessages(sozinha);
	assert.ok(bytesOf(grupo) <= MAX_HISTORY_BYTES);
	assert.match(JSON.stringify(grupo[0]), /pedido único/);
	assert.match(JSON.stringify(grupo.at(-1)), /"c19"/, "o grupo mais novo fica");
	const chamadas = new Set(grupo.filter((x) => (x as unknown as { role: string }).role === "assistant").map((x) => (x as unknown as { content: { id: string }[] }).content[0]!.id));
	for (const x of grupo.filter((y) => (y as unknown as { role: string }).role === "toolResult")) {
		assert.ok(chamadas.has((x as unknown as { toolCallId: string }).toolCallId), "resultado sem a chamada");
	}

	const argumentosGrandes: AgentMessage[] = [texto("pedido único")];
	for (let i = 0; i < 20; i++) argumentosGrandes.push(chamada("a" + i, { d: bloco }), resultado("a" + i, "ok"));
	const inteiros = fitMessages(argumentosGrandes);
	assert.ok(bytesOf(inteiros) <= MAX_HISTORY_BYTES);
	assert.equal((inteiros[1] as unknown as { role: string }).role, "assistant", "o corte não deixa resultado órfão depois do pedido");
	assert.equal(inteiros.filter((x) => (x as unknown as { role: string }).role === "assistant").length, inteiros.filter((x) => (x as unknown as { role: string }).role === "toolResult").length);
});

test("mensagem acima do teto por mensagem é encolhida e o prompt de sistema não entra na conta", () => {
	const sistema = { role: "system", content: "s".repeat(MAX_HISTORY_BYTES), timestamp: 1 } as unknown as AgentMessage;
	const enorme = { role: "assistant", api: "a", provider: "p", model: "m", stopReason: "stop", timestamp: 1, content: [
		{ type: "thinking", thinking: "t".repeat(MAX_MESSAGE_BYTES) }, { type: "text", text: "w".repeat(MAX_MESSAGE_BYTES * 2) },
	] } as unknown as AgentMessage;
	const cortada = fitMessages([sistema, texto("oi"), enorme]);
	assert.equal(cortada[0], sistema);
	assert.ok(bytesOf(cortada[2]) <= MAX_MESSAGE_BYTES);
	assert.doesNotMatch(JSON.stringify(cortada[2]), /thinking/);
});

test("o processo vivo manda ao provedor só o que cabe nos tetos, em turnos gigantes", async () => {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	const grande = "g".repeat(200_000);
	for (let i = 0; i < 10; i++) await turno(proc, String(i), "faux", grande);
	const resposta = await turno(proc, "x", "faux", "/contexto " + "c".repeat(200_000));
	const [, mensagens, bytes] = /^contexto: (\d+) (\d+) orfaos=0 ferramentas=\d+$/.exec(resposta) ?? [];
	assert.ok(mensagens, resposta);
	assert.ok(Number(bytes) <= MAX_HISTORY_BYTES, "o contexto do provedor passou do teto: " + bytes + " bytes");
	assert.ok(Number(mensagens) <= 21, "e as voltas mais antigas saíram: " + mensagens);
	proc.send({ id: "s", type: "get_state" });
	const estado = await proc.waitFor((v) => v.id === "s" && v.success === true);
	const guardadas = (estado.data as { messageCount: number }).messageCount;
	assert.ok(guardadas <= 14, "o processo vivo guarda só o que cabe nos tetos entre turnos: " + guardadas);
	await encerrar(proc);
});

const papel = (message: unknown): string => (message as { role: string }).role;

/** Uma mensagem de usuário com exatamente `bytes` bytes de JSON. */
function usuarioDe(bytes: number): AgentMessage {
	const vazia = bytesOf(texto(""));
	return texto("u".repeat(bytes - vazia));
}

test("a medida é o array JSON: oito mensagens que somam o teto já não cabem (colchetes e vírgulas)", () => {
	const cheia = usuarioDe(MAX_MESSAGE_BYTES);
	assert.equal(bytesOf(cheia), MAX_MESSAGE_BYTES);
	const oito = Array.from({ length: 8 }, () => usuarioDe(MAX_MESSAGE_BYTES));
	assert.equal(oito.reduce((soma, m) => soma + bytesOf(m), 0), MAX_HISTORY_BYTES, "a soma sozinha é o teto");
	assert.equal(arrayBytes(MAX_HISTORY_BYTES, 8), bytesOf(oito));
	assert.ok(bytesOf(oito) > MAX_HISTORY_BYTES, "o array serializado passa do teto");

	const cortada = fitMessages(oito);
	assert.equal(cortada.length, 7, "sai a volta mais antiga");
	assert.ok(bytesOf(cortada) <= MAX_HISTORY_BYTES);
	assert.throws(() => receber(oito), /acima de/, "o receptor mede igual e recusa");
	assert.equal(receber(oito.slice(1)).length, 7);
});

test("o JSON final de cada mensagem cabe no teto mesmo quando o escape o multiplica", () => {
	// Cada aspas vira duas no JSON: o texto bruto cabe, o JSON não.
	const aspas = texto('"'.repeat(200_000));
	assert.ok(bytesOf(aspas) > MAX_MESSAGE_BYTES);
	const [encolhida] = fitMessages([aspas]);
	assert.ok(bytesOf(encolhida) <= MAX_MESSAGE_BYTES, "mensagem única acima do teto: " + bytesOf(encolhida));
	assert.equal(papel(encolhida), "user");

	const controles = { role: "toolResult", toolCallId: "c", toolName: "t", isError: false, timestamp: 1, content: [{ type: "text", text: "\u0001".repeat(200_000) }] } as unknown as AgentMessage;
	const [resultadoCortado] = fitMessages([chamada("c"), controles]).slice(1);
	assert.ok(bytesOf(resultadoCortado) <= MAX_MESSAGE_BYTES, "controles viram \\u0001: seis bytes cada");

	const lista = Array.from({ length: 12 }, () => texto('"'.repeat(200_000)));
	const final = fitMessages(lista);
	assert.ok(bytesOf(final) <= MAX_HISTORY_BYTES, "o array final cabe: " + bytesOf(final));
	for (const m of final) assert.ok(bytesOf(m) <= MAX_MESSAGE_BYTES);
});

test("o corte de uma volta sozinha não deixa resultado órfão quando os argumentos são grandes e o resultado, pequeno", () => {
	const bloco = "z".repeat(MAX_MESSAGE_BYTES / 2);
	const sozinha: AgentMessage[] = [texto("pedido único")];
	for (let i = 0; i < 20; i++) sozinha.push(chamada("a" + i, { d: bloco }), resultado("a" + i, "ok"));
	const inteiros = fitMessages(sozinha);
	assert.ok(bytesOf(inteiros) <= MAX_HISTORY_BYTES);
	assert.equal(papel(inteiros[1]), "assistant", "depois do pedido vem uma chamada, nunca um resultado solto");
	const chamadas = new Set(inteiros.filter((m) => papel(m) === "assistant").map((m) => (m as unknown as { content: { id: string }[] }).content[0]!.id));
	for (const m of inteiros.filter((x) => papel(x) === "toolResult")) assert.ok(chamadas.has((m as unknown as { toolCallId: string }).toolCallId));
});

test("o receptor devolve só os campos permitidos e recusa resultado sem a chamada dele", () => {
	const suja = {
		role: "assistant", api: "a", provider: "p", model: "m", stopReason: "toolUse", timestamp: 5, responseId: "interno", errorMessage: "diag",
		usage: { input: 3, output: "texto", extra: 9, cost: { total: 0.5, x: 1 } },
		content: [{ type: "text", text: "oi", textSignature: "x" }, { type: "toolCall", id: "c1", name: "eco", arguments: {}, thoughtSignature: "y" }],
	};
	const ok = { role: "toolResult", toolCallId: "c1", toolName: "eco", isError: false, timestamp: 6, content: [{ type: "text", text: "feito" }], details: { interno: true } };
	const [a, r] = receber([texto("pedido"), suja, ok].slice(1)) as unknown as Record<string, unknown>[];
	assert.deepEqual(Object.keys(a!).sort(), ["api", "content", "model", "provider", "role", "stopReason", "timestamp", "usage"]);
	assert.deepEqual(a!.content, [{ type: "text", text: "oi" }, { type: "toolCall", id: "c1", name: "eco", arguments: {} }]);
	assert.deepEqual(a!.usage, { input: 3, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0.5 } });
	assert.deepEqual(Object.keys(r!).sort(), ["content", "isError", "role", "timestamp", "toolCallId", "toolName"]);

	assert.throws(() => receber([texto("pedido"), { ...ok, toolCallId: "outra" }]), /sem a chamada/);
	assert.throws(() => receber([suja, texto("nova volta"), ok]), /sem resultado/, "a chamada de outra volta não vale");
	assert.throws(() => receber([ok]), /sem a chamada/);
});

test("a mensagem final do turno também respeita os tetos, na mesma medida", async () => {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	proc.send({ id: "m", type: "set_model", provider: "faux", modelId: "faux-1" });
	await proc.waitFor((v) => v.command === "set_model" && v.success === true);
	proc.send({ id: "t", type: "set_tools", tools: [{ name: "list_bots", description: "Lista", inputSchema: { type: "object", properties: {} }, origin: { kind: "hub" } }] });
	await proc.waitFor((v) => v.id === "t" && v.success === true);
	proc.send({ id: "p", type: "prompt", message: "/ferramenta list_bots {}" });
	const pedido = await proc.waitFor((v) => v.type === "tool_request");
	proc.send({ type: "tool_result", callId: pedido.callId, ok: true, output: '"'.repeat(150_000) });
	const fim = await proc.waitFor((v) => v.type === "agent_end");
	const mensagens = fim.messages as AgentMessage[];
	assert.ok(mensagens.length >= 3, "pedido, chamada, resultado e resposta");
	assert.ok(bytesOf(mensagens) <= MAX_HISTORY_BYTES);
	for (const m of mensagens) assert.ok(bytesOf(m) <= MAX_MESSAGE_BYTES, papel(m) + " passou do teto por mensagem: " + bytesOf(m));
	await encerrar(proc);
});

const numerica = (preenchimento: number): string =>
	JSON.stringify({ role: "assistant", content: [{ type: "text", text: "n" }], api: "a", provider: "p", model: "m", stopReason: "stop", usage: { input: 0 }, timestamp: 1, x: "z".repeat(preenchimento) }).replace('"input":0', '"input":1e20');

test("o receptor mede os bytes que recebeu, não uma nova serialização (número que cada serializador imprime de um jeito)", () => {
	const numero = numerica(0);
	assert.ok(bytesOf(JSON.parse(numero)) > Buffer.byteLength(numero) + 10, "o Node imprime 1e20 com 21 dígitos; o texto recebido tem 4 bytes");
	const vazia = Buffer.byteLength(JSON.stringify({ role: "user", content: "", timestamp: 1 }));
	const usuario = (bytes: number) => JSON.stringify({ role: "user", content: "e".repeat(bytes - vazia), timestamp: 1 });
	const fixas = [...Array.from({ length: 7 }, () => usuario(MAX_MESSAGE_BYTES)), numero];
	const gasto = fixas.reduce((soma, m) => soma + Buffer.byteLength(m), 0);
	// A última mensagem fecha EXATAMENTE o teto do array recebido: 2 colchetes, a soma e uma vírgula entre cada par.
	const ultima = MAX_HISTORY_BYTES - 2 - fixas.length - gasto;
	assert.ok(ultima > vazia && ultima <= MAX_MESSAGE_BYTES);
	const lista = [...fixas, usuario(ultima)];
	assert.equal(arrayBytes(lista.reduce((soma, m) => soma + Buffer.byteLength(m), 0), lista.length), MAX_HISTORY_BYTES);
	assert.equal(parseHistory(lista).length, 9, "no teto exato dos bytes recebidos cabe");
	assert.throws(() => parseHistory([...fixas, usuario(ultima + 1)]), /acima de/, "um byte a mais não cabe");
});

test("resultado de ferramenta é um para um com a chamada: id consumido e nome igual", () => {
	const ok = resultado("c1", "feito");
	assert.equal(receber([texto("pedido"), chamada("c1"), ok]).length, 3);
	assert.throws(() => receber([texto("pedido"), chamada("c1"), ok, ok]), /sem a chamada/, "o mesmo id não vale duas vezes");
	assert.throws(() => receber([texto("pedido"), chamada("c1"), { ...ok, toolName: "outra" }]), /sem a chamada/, "o nome da ferramenta tem de ser o da chamada");
	const dupla = { role: "assistant", api: "a", provider: "p", model: "m", stopReason: "toolUse", timestamp: 1, content: [{ type: "toolCall", id: "d", name: "eco", arguments: {} }, { type: "toolCall", id: "d", name: "eco", arguments: {} }] };
	assert.throws(() => receber([texto("pedido"), dupla]), /repetida/);
});

test("o acumulador do turno também é limitado enquanto o turno anda, e cada mensagem sai dentro do teto", async () => {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	proc.send({ id: "m", type: "set_model", provider: "faux", modelId: "faux-1" });
	await proc.waitFor((v) => v.command === "set_model" && v.success === true);
	proc.send({ id: "t", type: "set_tools", tools: [{ name: "list_bots", description: "Lista", inputSchema: { type: "object", properties: {} }, origin: { kind: "hub" } }] });
	await proc.waitFor((v) => v.id === "t" && v.success === true);
	proc.send({ id: "p", type: "prompt", message: "/cadeia 14 list_bots {}" });
	const vistos = new Set<unknown>();
	for (let i = 0; i < 14; i++) {
		const pedido = await proc.waitFor((v) => v.type === "tool_request" && !vistos.has(v.callId));
		vistos.add(pedido.callId);
		proc.send({ type: "tool_result", callId: pedido.callId, ok: true, output: "r".repeat(300_000) });
	}
	const fim = await proc.waitFor((v) => v.type === "agent_end");
	const mensagens = fim.messages as AgentMessage[];
	assert.ok(bytesOf(mensagens) <= MAX_HISTORY_BYTES, "o acumulador do turno passou do teto: " + bytesOf(mensagens));
	assert.ok(mensagens.length < 2 * 14 + 2, "as voltas mais antigas do próprio turno saíram: " + mensagens.length);
	for (const m of proc.lines.filter((v) => v.type === "message_end").map((v) => v.message)) {
		assert.ok(bytesOf(m) <= MAX_MESSAGE_BYTES, "mensagem entregue acima do teto por mensagem: " + bytesOf(m));
	}
	await encerrar(proc);
});

async function comFerramenta(): Promise<Proc> {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	proc.send({ id: "m", type: "set_model", provider: "faux", modelId: "faux-1" });
	await proc.waitFor((v) => v.command === "set_model" && v.success === true);
	proc.send({ id: "t", type: "set_tools", tools: [{ name: "list_bots", description: "Lista", inputSchema: { type: "object", properties: {} }, origin: { kind: "hub" } }] });
	await proc.waitFor((v) => v.id === "t" && v.success === true);
	return proc;
}

test("os argumentos de uma chamada chegam inteiros à ferramenta mesmo acima do teto por mensagem", async () => {
	const proc = await comFerramenta();
	proc.send({ id: "p", type: "prompt", message: "/ferramentagrande list_bots 300000" });
	const pedido = await proc.waitFor((v) => v.type === "tool_request");
	assert.equal(((pedido.input as { x: string }).x ?? "").length, 300_000, "a ferramenta recebe o argumento sem corte");
	proc.send({ type: "tool_result", callId: pedido.callId, ok: true, output: "feito" });
	await proc.waitFor((v) => v.type === "agent_end");
	await encerrar(proc);
});

test("o corte do processo vivo espera a volta de ferramentas fechar e nunca deixa resultado sem a chamada", async () => {
	const proc = await comFerramenta();
	proc.send({ id: "p", type: "prompt", message: "/paralelo 10 list_bots {}" });
	const vistos = new Set<unknown>();
	for (let i = 0; i < 10; i++) {
		const pedido = await proc.waitFor((v) => v.type === "tool_request" && !vistos.has(v.callId));
		vistos.add(pedido.callId);
		proc.send({ type: "tool_result", callId: pedido.callId, ok: true, output: i < 9 ? "r".repeat(250_000) : "ok" });
	}
	await proc.waitFor((v) => v.type === "agent_end");
	for (const linha of proc.lines.filter((v) => v.type === "turn_end")) assert.deepEqual(Object.keys(linha), ["type"], "o turn_end vai sem a carga da volta");
	proc.send({ id: "c", type: "prompt", message: "/contexto" });
	const fim = await proc.waitFor((v) => v.type === "agent_end" && proc.lines.filter((x) => x.type === "agent_end").length > 1);
	const resposta = textoFinal(fim);
	assert.match(resposta, /orfaos=0 ferramentas=\d+$/, "o contexto do provedor não tem resultado sem a chamada: " + resposta);
	await encerrar(proc);
});

test("batchOpen vê a volta aberta pela última mensagem do assistente", () => {
	const duas = { role: "assistant", content: [{ type: "toolCall", id: "a", name: "eco", arguments: {} }, { type: "toolCall", id: "b", name: "eco", arguments: {} }], api: "a", provider: "p", model: "m", stopReason: "toolUse", timestamp: 1 } as unknown as AgentMessage;
	assert.equal(batchOpen([texto("pedido"), duas]), true);
	assert.equal(batchOpen([texto("pedido"), duas, resultado("a", "x")]), true);
	assert.equal(batchOpen([texto("pedido"), duas, resultado("a", "x"), resultado("b", "y")]), false);
	assert.equal(batchOpen([texto("pedido"), texto("oi", "assistant")]), false);
	assert.equal(batchOpen([]), false);
});

test("o receptor recusa chamada que ficou sem resultado", () => {
	assert.throws(() => receber([texto("pedido"), chamada("c1"), texto("outra volta")]), /sem resultado/);
	assert.throws(() => receber([texto("pedido"), chamada("c1")]), /sem resultado/);
	assert.equal(receber([texto("pedido"), chamada("c1"), resultado("c1", "ok"), texto("seguinte")]).length, 4);
});

test("o restore_history também respeita os tetos depois de reserializar", async () => {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	proc.send({ id: "r", type: "restore_history", messages: fio([texto("a"), texto("b", "assistant")]) });
	assert.equal((await proc.waitFor((v) => v.id === "r")).success, true);
	await encerrar(proc);
});

test("um catálogo grande de ferramentas chega inteiro ao provedor (a mensagem de sistema não é cortada)", async () => {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	proc.send({ id: "m", type: "set_model", provider: "faux", modelId: "faux-1" });
	await proc.waitFor((v) => v.command === "set_model" && v.success === true);
	const tools = Array.from({ length: 40 }, (_, i) => ({ name: "ferramenta_" + i, description: "d".repeat(8192), inputSchema: { type: "object", properties: {} }, origin: { kind: "hub" } }));
	proc.send({ id: "t", type: "set_tools", tools });
	await proc.waitFor((v) => v.id === "t" && v.success === true);
	proc.send({ id: "c", type: "prompt", message: "/contexto" });
	const fim = await proc.waitFor((v) => v.type === "agent_end");
	assert.match(textoFinal(fim), /ferramentas=40$/, "as 40 declarações, mais de 320 KiB juntas, chegam ao provedor: " + textoFinal(fim));
	await encerrar(proc);
});
