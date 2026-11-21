// Prova do motor (fatia 1): conversa via provedor faux, credencial do fd 3
// chegando ao transporte do provedor real (com fetch stubado, sem rede), falha
// fechada sem credencial, chave fora de argv/saída/stderr e fim do turno quando
// o canal do cofre morre.
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import assert from "node:assert/strict";
import { ENTRY, encerrar, startEngine, turnoTerminou } from "./helpers.ts";

const SECRET = "chave-de-teste-nao-pode-vazar";

const resumo = (valor: string) => `sha256-${createHash("sha256").update(valor).digest("hex").slice(0, 16)}`;
const chamadasFetch = (log: string) =>
	(existsSync(log) ? readFileSync(log, "utf8") : "")
		.split("\n")
		.filter(Boolean)
		.map((l) => JSON.parse(l) as { url: string; headers: Record<string, string>; body?: string });

test("conversa responde pelo provedor faux", async () => {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" });
	proc.send({ id: "m", type: "set_model", provider: "faux", modelId: "faux-1" });
	await proc.waitFor((v) => v.command === "set_model" && v.success === true);
	proc.send({ id: "p", type: "prompt", message: "ola mexerica" });
	const end = await proc.waitFor((v) => v.type === "agent_end");
	const messages = end.messages as { role: string; content: { type: string; text?: string }[] }[];
	const text = messages.at(-1)?.content.map((b) => b.text ?? "").join("") ?? "";
	assert.ok(text.includes("eco: ola mexerica"), `esperava eco, veio: ${text}`);
	await encerrar(proc);
});

test("a credencial do cofre chega ao transporte do provedor certo", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-fetch-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({}, { serveKey: SECRET, fetchLog: log });
	proc.send({ id: "m", type: "set_model", provider: "google", modelId: "gemini-2.5-flash" });
	await proc.waitFor((v) => v.command === "set_model" && v.success === true);
	proc.send({ id: "p", type: "prompt", message: "oi" });
	const done = await proc.waitFor(turnoTerminou);
	assert.ok(done, "o turno tinha de terminar (o stub responde 401)");
	const calls = chamadasFetch(log);
	assert.equal(calls.length, 1, `esperava exatamente 1 chamada de rede, vieram ${calls.length}`);
	const headers = calls[0]!.headers;
	assert.match(calls[0]!.url, /generativelanguage\.googleapis\.com/);
	assert.equal(
		headers["x-goog-api-key"],
		resumo(SECRET),
		"a credencial do fd 3 não chegou ao cabeçalho do provedor",
	);
	// O erro do provedor ecoa a chave no corpo: a saída não pode contê-la.
	assert.ok(!JSON.stringify(proc.lines).includes(SECRET), "a chave vazou na saída RPC");
	await encerrar(proc);
	assert.ok(!proc.stderr().includes(SECRET), "a chave vazou no stderr do motor");
	rmSync(dir, { recursive: true, force: true });
});

test("a credencial do cofre chega ao transporte da Meta Model API", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-fetch-meta-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({}, { serveKey: SECRET, fetchLog: log });
	try {
		proc.send({ id: "m", type: "set_model", provider: "meta", modelId: "muse-spark-1.3" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const done = await proc.waitFor(turnoTerminou);
		assert.ok(done, "o turno tinha de terminar (o stub responde 401)");
		const calls = chamadasFetch(log);
		assert.equal(calls.length, 1, `esperava exatamente 1 chamada de rede, vieram ${calls.length}`);
		assert.equal(calls[0]!.url, "https://api.meta.ai/v1/chat/completions");
		assert.equal(calls[0]!.headers.authorization, resumo(`Bearer ${SECRET}`));
		assert.ok(!JSON.stringify(proc.lines).includes(SECRET), "a chave vazou na saída RPC");
		await encerrar(proc);
		assert.ok(!proc.stderr().includes(SECRET), "a chave vazou no stderr do motor");
	} finally {
		if (!proc.child.killed) await encerrar(proc);
		rmSync(dir, { recursive: true, force: true });
	}
});

test("o pedido à Meta não leva esforço de raciocínio «none»", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-fetch-meta-corpo-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({ MEXERICA_TEST_SUCCESS_COMPLETIONS: "1" }, { serveKey: SECRET, fetchLog: log });
	try {
		proc.send({ id: "m", type: "set_model", provider: "meta", modelId: "muse-spark-1.3" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const end = await proc.waitFor((v) => v.type === "agent_end");
		assert.match(JSON.stringify(end), /resposta simulada/);
		const calls = chamadasFetch(log);
		assert.equal(calls.length, 1, `esperava exatamente 1 chamada de rede, vieram ${calls.length}`);
		assert.equal(calls[0]!.url, "https://api.meta.ai/v1/chat/completions");
		const corpo = JSON.parse(calls[0]!.body ?? "{}") as Record<string, unknown>;
		assert.equal(corpo.model, "muse-spark-1.3");
		// Sem nível de raciocínio pedido, o pedido não pode levar esforço
		// "none": a Meta só aceita "minimal" a "max" e recusa o resto com 400.
		// O modelo montado pelo catálogo não pede raciocínio e a Meta usa o
		// esforço padrão dela.
		assert.equal(corpo.reasoning_effort, undefined, "o pedido à Meta levou reasoning_effort inesperado");
		assert.ok(!JSON.stringify(corpo).includes('"none"'), "o pedido à Meta levou effort «none»");
	} finally {
		if (!proc.child.killed) await encerrar(proc);
		rmSync(dir, { recursive: true, force: true });
	}
});

test("sem credencial o prompt falha fechado e sem rede", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-fetch-"));
	const log = join(dir, "fetch.jsonl");
	// Chave ambiente concorrente presente: o AuthContext vazio a ignora e o
	// cofre responde null — o turno falha sem nenhuma chamada de rede.
	const proc = startEngine({ GEMINI_API_KEY: "ambiente-ignorado", GOOGLE_API_KEY: "ambiente-ignorado" }, { fetchLog: log });
	proc.send({ id: "m", type: "set_model", provider: "google", modelId: "gemini-2.5-flash" });
	await proc.waitFor((v) => v.command === "set_model");
	proc.send({ id: "p", type: "prompt", message: "oi" });
	const done = await proc.waitFor(turnoTerminou);
	assert.ok(done, "o motor não pode completar sem chave");
	assert.equal(chamadasFetch(log).length, 0, "houve chamada de rede sem credencial do cofre");
	await encerrar(proc);
	rmSync(dir, { recursive: true, force: true });
});

test("canal do cofre fechado no meio do pedido termina o turno", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-fetch-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({}, { closeVaultOnRequest: true, fetchLog: log });
	proc.send({ id: "m", type: "set_model", provider: "google", modelId: "gemini-2.5-flash" });
	await proc.waitFor((v) => v.command === "set_model");
	proc.send({ id: "p", type: "prompt", message: "oi" });
	// Sem rejeição das pendências o turno esperaria para sempre.
	const done = await proc.waitFor(turnoTerminou);
	assert.ok(done, "o turno tinha de terminar com o cofre morto");
	assert.equal(chamadasFetch(log).length, 0, "houve chamada de rede sem credencial");
	await encerrar(proc);
	rmSync(dir, { recursive: true, force: true });
});

test("a chave não aparece em argv, na saída nem no stderr", async () => {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" }, { serveKey: SECRET });
	assert.deepEqual(proc.child.spawnargs.slice(1), [ENTRY]);
	assert.ok(!proc.child.spawnargs.join(" ").includes(SECRET));
	proc.send({ id: "m", type: "set_model", provider: "faux", modelId: "faux-1" });
	proc.send({ id: "p", type: "prompt", message: "oi" });
	await proc.waitFor((v) => v.type === "agent_end");
	assert.ok(!JSON.stringify(proc.lines).includes(SECRET), "a chave vazou na saída RPC");
	await encerrar(proc);
	assert.ok(!proc.stderr().includes(SECRET), "a chave vazou no stderr do motor");
});

test("OpenAI recebe a chave do cofre no transporte simulado", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-openai-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({}, { serveKey: SECRET, fetchLog: log });
	try {
		proc.send({ id: "m", type: "set_model", provider: "openai", modelId: "gpt-5-mini" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		await proc.waitFor(turnoTerminou);
		const calls = chamadasFetch(log);
		assert.equal(calls.length, 1);
		assert.match(calls[0]!.url, /^https:\/\/api\.openai\.com\//);
		assert.equal(calls[0]!.headers["authorization"], resumo(`Bearer ${SECRET}`));
		assert.ok(!JSON.stringify(proc.lines).includes(SECRET));
	} finally {
		await encerrar(proc);
		rmSync(dir, { recursive: true, force: true });
	}
	assert.ok(!proc.stderr().includes(SECRET));
});

test("stderr fragmentado não expõe a chave após o processo encerrar", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-stderr-"));
	const proc = startEngine({ MEXERICA_TEST_STDERR_SPLIT: "1" }, { serveKey: SECRET, fetchLog: join(dir, "fetch.jsonl") });
	try {
		proc.send({ id: "m", type: "set_model", provider: "google", modelId: "gemini-2.5-flash" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		await proc.waitFor(turnoTerminou);
	} finally {
		await encerrar(proc);
		rmSync(dir, { recursive: true, force: true });
	}
	assert.match(proc.stderr(), /diagnóstico omitido/);
	assert.ok(!proc.stderr().includes(SECRET));
});

test("cofre silencioso falha após o prazo sem chamada de rede", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-timeout-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({}, { ignoreVaultRequest: true, fetchLog: log });
	try {
		proc.send({ id: "m", type: "set_model", provider: "google", modelId: "gemini-2.5-flash" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		await proc.waitFor(turnoTerminou, 40_000);
		assert.equal(chamadasFetch(log).length, 0);
	} finally {
		await encerrar(proc);
		rmSync(dir, { recursive: true, force: true });
	}
});

test("Google conclui resposta no transporte simulado", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-success-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({ MEXERICA_TEST_SUCCESS: "1" }, { serveKey: SECRET, fetchLog: log });
	try {
		proc.send({ id: "m", type: "set_model", provider: "google", modelId: "gemini-2.5-flash" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const end = await proc.waitFor((v) => v.type === "agent_end");
		assert.match(JSON.stringify(end), /resposta simulada/);
		assert.equal(chamadasFetch(log).length, 1);
	} finally {
		await encerrar(proc);
		rmSync(dir, { recursive: true, force: true });
	}
});
