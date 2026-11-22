// Identidade de cache da conversa e prompt de sistema no pedido ao fornecedor, com rede falsa: o valor opaco vindo do
// Hub vai no corpo como identidade de sessão, sem ele o campo não existe, valor fora do formato é recusado, e um prompt
// de sistema novo substitui o anterior em vez de somar.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { encerrar, startEngine, type Proc } from "./helpers.ts";

const STUB = fileURLToPath(new URL("./stub-respostas.mjs", import.meta.url));
const SECRET = "chave-de-teste-do-cache";
const IDENTIDADE = "9f2c4a1d7b3e5068ac1d2b3c4d5e6f70";

interface Cenario {
	proc: Proc;
	log: string;
	fechar: () => Promise<void>;
}

function motor(): Cenario {
	const dir = mkdtempSync(join(tmpdir(), "motor-cache-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({}, { serveKey: SECRET, fetchLog: log, fetchStub: STUB });
	return {
		proc,
		log,
		fechar: async () => {
			await encerrar(proc);
			rmSync(dir, { recursive: true, force: true });
		},
	};
}

const corpos = (log: string): Record<string, unknown>[] =>
	(existsSync(log) ? readFileSync(log, "utf8") : "")
		.split("\n")
		.filter(Boolean)
		.map((linha) => JSON.parse(JSON.parse(linha).body ?? "{}") as Record<string, unknown>);

/** Espera a n-ésima ocorrência de um evento (o `waitFor` dos ajudantes devolve a primeira que já chegou). */
async function esperar(proc: Proc, pred: (v: Record<string, unknown>) => boolean, n = 1): Promise<void> {
	const limite = Date.now() + 30_000;
	while (proc.lines.filter(pred).length < n) {
		assert.ok(Date.now() < limite, `timeout esperando o evento ${n}; linhas: ${JSON.stringify(proc.lines)}`);
		await new Promise((resolve) => setTimeout(resolve, 20));
	}
}

test("o pedido leva a identidade de cache igual ao valor opaco, e o começo do pedido se repete", async () => {
	const { proc, log, fechar } = motor();
	try {
		proc.send({ id: "m", type: "set_model", provider: "openai", modelId: "gpt-teste", cacheKey: IDENTIDADE });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p1", type: "prompt", message: "primeira" });
		await esperar(proc, (v) => v.type === "agent_end", 1);
		proc.send({ id: "p2", type: "prompt", message: "segunda" });
		await esperar(proc, (v) => v.type === "agent_end", 2);
		const [primeiro, segundo] = corpos(log);
		assert.equal(primeiro?.["prompt_cache_key"], IDENTIDADE, "o corpo do primeiro turno não levou a identidade");
		assert.equal(segundo?.["prompt_cache_key"], IDENTIDADE, "o corpo do segundo turno não levou a identidade");
		const inicio = (corpo: Record<string, unknown>) => JSON.stringify((corpo["input"] as unknown[] | undefined)?.slice(0, 1));
		assert.equal(inicio(primeiro!), inicio(segundo!), "o começo do pedido mudou entre turnos seguintes");
		assert.ok(!JSON.stringify(proc.lines).includes(SECRET), "a chave vazou na saída RPC");
	} finally {
		await fechar();
	}
});

test("sem a identidade o campo de cache não existe no pedido", async () => {
	const { proc, log, fechar } = motor();
	try {
		proc.send({ id: "m", type: "set_model", provider: "openai", modelId: "gpt-teste" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		await esperar(proc, (v) => v.type === "agent_end");
		const [corpo] = corpos(log);
		assert.ok(corpo, "o turno tinha de chegar ao fornecedor");
		assert.equal(corpo["prompt_cache_key"], undefined, "sem identidade o campo não pode existir");
	} finally {
		await fechar();
	}
});

test("identidade de cache fora do formato é recusada sem ir para o pedido", async () => {
	const { proc, log, fechar } = motor();
	try {
		proc.send({ id: "m", type: "set_model", provider: "openai", modelId: "gpt-teste", cacheKey: "curto demais!" });
		const recusa = await proc.waitFor((v) => v.command === "set_model");
		assert.equal(recusa["success"], false, "identidade fora do formato tinha de recusar o modelo");
		assert.match(String(recusa["error"]), /cacheKey/);
		assert.deepEqual(corpos(log), [], "nada podia sair para o fornecedor");
	} finally {
		await fechar();
	}
});

test("a Anthropic não recebe thread no servidor, mesmo com identidade de cache", async () => {
	const dir = mkdtempSync(join(tmpdir(), "motor-anthropic-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine(
		{ MEXERICA_TEST_CENARIO: "anthropic" },
		{ serveKey: SECRET, fetchLog: log, fetchStub: STUB },
	);
	try {
		proc.send({ id: "m", type: "set_model", provider: "anthropic", modelId: "claude-teste", cacheKey: IDENTIDADE });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		await esperar(proc, (v) => v.type === "agent_end");
		const linha = JSON.parse(readFileSync(log, "utf8").split("\n").filter(Boolean)[0]!) as {
			headers: Record<string, string>;
			body: string;
		};
		const corpo = JSON.parse(linha.body) as Record<string, unknown>;
		assert.equal(corpo["thread"], undefined, "o motor guarda a conversa: nada de thread no fornecedor");
		assert.equal(
			Object.keys(linha.headers)
				.filter((nome) => nome.toLowerCase() === "anthropic-beta")
				.map((nome) => linha.headers[nome] ?? "")
				.join(" ")
				.includes("message-threads"),
			false,
			"o beta de threads não pode ser pedido",
		);
	} finally {
		await encerrar(proc);
		rmSync(dir, { recursive: true, force: true });
	}
});

test("prompt de sistema novo substitui o anterior em vez de somar, e as ferramentas continuam", async () => {
	const { proc, log, fechar } = motor();
	try {
		proc.send({ id: "m", type: "set_model", provider: "openai", modelId: "gpt-teste" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({
			id: "t",
			type: "set_tools",
			tools: [
				{
					name: "efeito",
					description: "aplica um efeito uma vez",
					inputSchema: { type: "object", properties: {} },
					origin: { kind: "hub" },
				},
			],
		});
		await proc.waitFor((v) => v.command === "set_tools" && v.success === true);
		proc.send({ id: "s1", type: "set_system_prompt", prompt: "PRIMEIRO-MANUAL" });
		await proc.waitFor((v) => v.command === "set_system_prompt" && v.success === true);
		proc.send({ id: "s2", type: "set_system_prompt", prompt: "SEGUNDO-MANUAL" });
		await esperar(proc, (v) => v.command === "set_system_prompt", 2);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		await esperar(proc, (v) => v.type === "agent_end");
		const [corpo] = corpos(log);
		const texto = JSON.stringify(corpo);
		assert.equal(texto.split("PRIMEIRO-MANUAL").length - 1, 0, "o prompt anterior continuou no pedido");
		assert.equal(texto.split("SEGUNDO-MANUAL").length - 1, 1, "o prompt novo tinha de ir exatamente uma vez");
		const ferramentas = (corpo?.["tools"] ?? []) as { name?: string }[];
		assert.deepEqual(
			ferramentas.map((f) => f.name),
			["efeito"],
			"as ferramentas declaradas continuam valendo depois do prompt novo",
		);
	} finally {
		await fechar();
	}
});
