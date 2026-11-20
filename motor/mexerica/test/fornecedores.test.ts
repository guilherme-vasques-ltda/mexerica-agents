// «Testar acesso» e a conversa de cada fornecedor por chave, contra servidores de mentira (sem rede, sem chave
// real, sem chamada paga). Em todo cenário a chave nunca aparece na saída do motor nem no stderr.
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import assert from "node:assert/strict";
import { encerrar, startEngine, turnoTerminou, type Line } from "./helpers.ts";

const STUB = fileURLToPath(new URL("./stub-fornecedores.mjs", import.meta.url));
const SECRET = "chave-de-mentira-que-nao-pode-vazar-42";

type Chamada = { url: string; method: string; headers: Record<string, string> };

async function comMotor<T>(cenario: string, corpo: (proc: ReturnType<typeof startEngine>, chamadas: () => Chamada[]) => Promise<T>, segredo = SECRET): Promise<T> {
	const dir = mkdtempSync(join(tmpdir(), "motor-fornecedores-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine({ MEXERICA_TEST_CENARIO: cenario, MEXERICA_TESTE_PRAZO_MS: "300" }, { serveKey: segredo, fetchLog: log, fetchStub: STUB });
	const chamadas = () =>
		(existsSync(log) ? readFileSync(log, "utf8") : "")
			.split("\n")
			.filter(Boolean)
			.map((l) => JSON.parse(l) as Chamada);
	try {
		return await corpo(proc, chamadas);
	} finally {
		await encerrar(proc);
		for (const forma of [segredo, encodeURIComponent(segredo)]) {
			assert.ok(!JSON.stringify(proc.lines).includes(forma), "a chave vazou na saída do motor");
			assert.ok(!proc.stderr().includes(forma), "a chave vazou no stderr do motor");
		}
		rmSync(dir, { recursive: true, force: true });
	}
}

async function testar(proc: ReturnType<typeof startEngine>, provider: string): Promise<Record<string, unknown>> {
	proc.send({ id: "t", type: "list_models", provider });
	const resposta = await proc.waitFor((v: Line) => v.id === "t" && v.type === "response");
	assert.equal(resposta.success, true, JSON.stringify(resposta));
	return resposta.data as Record<string, unknown>;
}

const FORNECEDORES = {
	anthropic: { lista: "https://api.anthropic.com/v1/models", conversa: /^https:\/\/api\.anthropic\.com\/v1\/messages/, auth: "x-api-key", modelos: ["claude-teste-grande", "claude-teste-pequeno"] },
	openai: { lista: "https://api.openai.com/v1/models", conversa: /^https:\/\/api\.openai\.com\/v1\/responses/, auth: "authorization", modelos: ["gpt-teste", "gpt-teste-mini"] },
	google: { lista: "https://generativelanguage.googleapis.com/v1beta/models", conversa: /^https:\/\/generativelanguage\.googleapis\.com\/v1beta\/models\/gemini-teste:streamGenerateContent/, auth: "x-goog-api-key", modelos: ["gemini-teste"] },
	xai: { lista: "https://api.x.ai/v1/models", conversa: /^https:\/\/api\.x\.ai\/v1\/chat\/completions/, auth: "authorization", modelos: ["grok-teste"] },
	openrouter: { lista: "https://openrouter.ai/api/v1/models/user", conversa: /^https:\/\/openrouter\.ai\/api\/v1\/chat\/completions/, auth: "authorization", modelos: ["fornecedor/modelo-teste"] },
	meta: { lista: "https://api.meta.ai/v1/models", conversa: /^https:\/\/api\.meta\.ai\/v1\/chat\/completions/, auth: "authorization", modelos: ["muse-teste-1.0"] },
} as const;

for (const [provider, esperado] of Object.entries(FORNECEDORES)) {
	test(`${provider}: chave válida lista só os modelos de conversa que ela enxerga`, () =>
		comMotor("ok", async (proc, chamadas) => {
			const acesso = await testar(proc, provider);
			assert.equal(acesso.ok, true, JSON.stringify(acesso));
			assert.deepEqual((acesso.modelos as { id: string }[]).map((m) => m.id), esperado.modelos);
			const [chamada, ...resto] = chamadas();
			assert.equal(resto.length, 0, "o teste faz um único pedido");
			assert.equal(chamada!.method, "GET", "testar acesso nunca abre conversa");
			assert.ok(chamada!.url.startsWith(esperado.lista), chamada!.url);
			assert.ok(chamada!.headers[esperado.auth]?.includes(SECRET), "a chave do cofre não foi no cabeçalho certo");
		}));

	for (const [cenario, erro] of [["invalida", "chave_invalida"], ["sem_permissao", "sem_permissao"], ["erro", "fornecedor_falhou"], ["lenta", "sem_resposta"], ["quebrada", "resposta_invalida"], ["enorme", "resposta_invalida"]] as const) {
		test(`${provider}: ${cenario} vira «${erro}» sem expor a chave`, () =>
			comMotor(cenario, async (proc) => {
				const acesso = await testar(proc, provider);
				assert.equal(acesso.ok, false);
				assert.equal(acesso.erro, erro, JSON.stringify(acesso));
			}));
	}

	test(`${provider}: modelo sem permissão encerra o turno com erro, no endereço certo`, () =>
		comMotor("ok", async (proc, chamadas) => {
			const modelo = esperado.modelos[0];
			proc.send({ id: "m", type: "set_model", provider, modelId: modelo });
			await proc.waitFor((v) => v.command === "set_model" && v.success === true);
			proc.send({ id: "p", type: "prompt", message: "oi" });
			const fim = await proc.waitFor(turnoTerminou);
			assert.ok(fim, "o turno tinha de terminar com erro");
			const conversa = chamadas().filter((c) => c.method === "POST");
			assert.equal(conversa.length, 1, JSON.stringify(chamadas()));
			assert.match(conversa[0]!.url, esperado.conversa);
			assert.ok(conversa[0]!.headers[esperado.auth]?.includes(SECRET));
		}));
}

test("modelo com id fora do formato é recusado antes de qualquer rede", () =>
	comMotor("ok", async (proc, chamadas) => {
		for (const [provider, id] of [["openai", "../gpt"], ["anthropic", "dono/modelo"], ["google", ""], ["google", "gemini:outro"], ["openrouter", "a//b"]]) {
			proc.send({ id: `m-${provider}`, type: "set_model", provider, modelId: id });
			const resposta = await proc.waitFor((v) => v.id === `m-${provider}`);
			assert.equal(resposta.success, false, `${provider}/${id} devia ser recusado`);
		}
		assert.equal(chamadas().length, 0);
	}));

test("fornecedor desconhecido no «Testar acesso» é recusado", () =>
	comMotor("ok", async (proc, chamadas) => {
		proc.send({ id: "t", type: "list_models", provider: "outro" });
		const resposta = await proc.waitFor((v) => v.id === "t");
		assert.equal(resposta.success, false);
		assert.equal(chamadas().length, 0);
	}));


test("chave repetida pelo fornecedor em forma de URL também é mascarada", () =>
	comMotor(
		"eco_codificado",
		async (proc) => {
			proc.send({ id: "m", type: "set_model", provider: "meta", modelId: "muse-teste-1.0" });
			await proc.waitFor((v) => v.command === "set_model" && v.success === true);
			proc.send({ id: "p", type: "prompt", message: "oi" });
			await proc.waitFor(turnoTerminou);
		},
		"LLM|chave-de-mentira-com-barra-77",
	));
