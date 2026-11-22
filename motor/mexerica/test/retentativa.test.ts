// Nova tentativa do pedido ao fornecedor com rede falsa: só a recusa antes do stream é repetida, no mesmo pedido e na
// mesma conta, com limite de tentativas e de espera, e cancelar durante a espera não gera chamada nova.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { encerrar, startEngine, turnoTerminou, type Proc } from "./helpers.ts";
import { retryProviderRequest } from "../../vendor/pi-mono/packages/ai/src/utils/provider-retry.ts";
import {
	ESPERA_MAXIMA_MS,
	MAX_TENTATIVAS,
	OPCOES_DE_REPETICAO,
	orcamentoDoTurno,
	STATUS_DE_REPETICAO,
	TETO_DE_ESPERA_DO_TURNO_MS,
} from "../src/retentativa.ts";

const STUB = fileURLToPath(new URL("./stub-respostas.mjs", import.meta.url));
const SECRET = "chave-de-teste-da-repeticao";

type Chamada = { url: string; method: string; headers: Record<string, string>; body?: string };
const chamadas = (log: string): Chamada[] =>
	(existsSync(log) ? readFileSync(log, "utf8") : "")
		.split("\n")
		.filter(Boolean)
		.map((linha) => JSON.parse(linha) as Chamada);

interface Cenario {
	proc: Proc;
	log: string;
	fechar: () => Promise<void>;
}

function motor(cenario: string): Cenario {
	const dir = mkdtempSync(join(tmpdir(), "motor-repeticao-"));
	const log = join(dir, "fetch.jsonl");
	const proc = startEngine(
		{ MEXERICA_TEST_CENARIO: cenario },
		{ serveKey: SECRET, fetchLog: log, fetchStub: STUB },
	);
	return {
		proc,
		log,
		fechar: async () => {
			await encerrar(proc);
			rmSync(dir, { recursive: true, force: true });
		},
	};
}

async function modelo(proc: Proc): Promise<void> {
	proc.send({ id: "m", type: "set_model", provider: "openai", modelId: "gpt-teste" });
	await proc.waitFor((v) => v.command === "set_model" && v.success === true);
}

test("duas recusas temporárias com retry-after curto terminam em um único fim de mensagem", async () => {
	const { proc, log, fechar } = motor("429_duas_vezes");
	try {
		await modelo(proc);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const end = await proc.waitFor((v) => v.type === "agent_end", 30_000);
		const texto = JSON.stringify(end);
		assert.match(texto, /resposta simulada/, "o turno tinha de terminar com a resposta do sucesso");
		assert.equal(chamadas(log).length, 3, "esperava 3 chamadas: a recusa duas vezes e o sucesso");
		assert.equal(proc.lines.filter((v) => v.type === "agent_end").length, 1, "um único fim de mensagem");
		assert.ok(!JSON.stringify(proc.lines).includes(SECRET), "a chave vazou na saída RPC");
	} finally {
		await fechar();
	}
});

test("espera pedida acima do teto falha na hora, sem repetir", async () => {
	const { proc, log, fechar } = motor("retry_longo");
	try {
		await modelo(proc);
		const antes = Date.now();
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const fim = await proc.waitFor(turnoTerminou, 30_000);
		assert.match(JSON.stringify(fim), /retry delay \(max: 30s\)/, "a falha tinha de citar o teto da espera");
		assert.ok(Date.now() - antes < 10_000, "não podia esperar a recusa de 120 s");
		assert.equal(chamadas(log).length, 1, "acima do teto não há repetição");
	} finally {
		await fechar();
	}
});

test("recusa de credencial com «x-should-retry: true» não repete", async () => {
	const { proc, log, fechar } = motor("401_de_retry");
	try {
		await modelo(proc);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const fim = await proc.waitFor(turnoTerminou, 30_000);
		assert.match(JSON.stringify(fim), /error/, "o turno tinha de terminar em erro");
		assert.equal(chamadas(log).length, 1, "401 é recusa definitiva: nenhuma repetição");
		assert.ok(!JSON.stringify(proc.lines).includes(SECRET), "a chave vazou na saída RPC");
	} finally {
		await fechar();
	}
});

test("cancelar durante a espera da repetição não gera chamada nova", async () => {
	const { proc, log, fechar } = motor("429_duas_vezes");
	try {
		await modelo(proc);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		// A primeira chamada leva 429 com retry-after de 1 s: o cancelamento chega no meio da espera.
		const espera = Date.now();
		while (chamadas(log).length < 1) {
			assert.ok(Date.now() - espera < 10_000, "a primeira chamada não saiu");
			await new Promise((resolve) => setTimeout(resolve, 20));
		}
		proc.send({ id: "a", type: "abort" });
		await proc.waitFor(turnoTerminou, 30_000);
		// Espera mais do que o retry-after pedido: uma repetição que escapasse do cancelamento apareceria aqui.
		await new Promise((resolve) => setTimeout(resolve, 1_500));
		assert.equal(chamadas(log).length, 1, "cancelar durante a espera não pode gerar chamada nova");
	} finally {
		await fechar();
	}
});

test("ferramenta já executada não é repetida: a repetição é o pedido seguinte, igual byte a byte", async () => {
	const { proc, log, fechar } = motor("ferramenta_429");
	try {
		await modelo(proc);
		proc.send({
			id: "t",
			type: "set_tools",
			tools: [
				{
					name: "efeito",
					description: "aplica um efeito uma vez",
					inputSchema: { type: "object", properties: { vezes: { type: "number" } } },
					origin: { kind: "hub" },
				},
			],
		});
		await proc.waitFor((v) => v.command === "set_tools" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "aplique" });
		const pedido = await proc.waitFor((v) => v.type === "tool_request", 30_000);
		proc.send({ id: "r", type: "tool_result", callId: pedido["callId"], ok: true, output: "efeito aplicado" });
		await proc.waitFor((v) => v.type === "agent_end", 30_000);
		assert.equal(proc.lines.filter((v) => v.type === "tool_request").length, 1, "a ferramenta tinha de rodar uma vez");
		const todas = chamadas(log);
		assert.equal(todas.length, 3, "esperava o pedido da ferramenta, a recusa e o pedido repetido");
		assert.ok(todas[1]!.body?.includes("efeito aplicado"), "o pedido repetido leva o resultado da ferramenta");
		assert.equal(todas[2]!.body, todas[1]!.body, "a repetição é o mesmo pedido depois do resultado da ferramenta");
	} finally {
		await fechar();
	}
});

test("queda de conexão sem resposta não repete: o pedido pode ter sido processado", async () => {
	const { proc, log, fechar } = motor("queda_de_conexao");
	try {
		await modelo(proc);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const fim = await proc.waitFor(turnoTerminou, 30_000);
		assert.match(JSON.stringify(fim), /error/, "o turno tinha de terminar em erro");
		assert.equal(chamadas(log).length, 1, "erro sem status HTTP não prova recusa: nenhuma repetição");
	} finally {
		await fechar();
	}
});

test("a repetição é finita: três chamadas e o turno falha", async () => {
	const { proc, log, fechar } = motor("429_sempre");
	try {
		await modelo(proc);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const fim = await proc.waitFor(turnoTerminou, 40_000);
		assert.match(JSON.stringify(fim), /error/, "o turno tinha de terminar em erro");
		assert.equal(chamadas(log).length, MAX_TENTATIVAS, `esperava ${MAX_TENTATIVAS} chamadas e nenhuma quarta`);
	} finally {
		await fechar();
	}
});

test("5xx não repete: o pedido pode ter sido processado", async () => {
	const { proc, log, fechar } = motor("503_sempre");
	try {
		await modelo(proc);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const fim = await proc.waitFor(turnoTerminou, 30_000);
		assert.match(JSON.stringify(fim), /error/, "o turno tinha de terminar em erro");
		assert.equal(chamadas(log).length, 1, "5xx não prova recusa antes do processamento: nenhuma repetição");
	} finally {
		await fechar();
	}
});

test("resposta que já começou a chegar não é repetida", async () => {
	const { proc, log, fechar } = motor("resposta_parcial");
	try {
		await modelo(proc);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const fim = await proc.waitFor(turnoTerminou, 30_000);
		const parciais = proc.lines.filter(
			(v) =>
				v.type === "message_update" &&
				(v.assistantMessageEvent as { type?: string } | undefined)?.type === "text_delta",
		);
		assert.ok(parciais.length > 0, "o texto parcial tinha de chegar antes da falha");
		assert.match(JSON.stringify(fim), /error/, "o turno tinha de terminar em erro");
		assert.equal(chamadas(log).length, 1, "resposta parcial não se repete");
	} finally {
		await fechar();
	}
});

test("o orçamento de espera é do turno, não de cada chamada", async () => {
	assert.equal(orcamentoDoTurno().remainingMs, TETO_DE_ESPERA_DO_TURNO_MS, "o turno começa com o teto inteiro");
	const recusa = () => Object.assign(new Error("limite de uso"), { status: 429, headers: new Headers({ "retry-after": "0.03" }) });
	// Um orçamento pequeno, compartilhado como no turno: 40 ms, e cada espera pede 30 ms.
	const orcamento = { remainingMs: 40 };
	let tentativas = 0;
	const pedido = async () => {
		tentativas += 1;
		if (tentativas === 1) throw recusa();
		return "ok";
	};
	assert.equal(await retryProviderRequest(pedido, { maxRetries: 2, retryBudget: orcamento }), "ok");
	assert.equal(tentativas, 2, "a primeira chamada repetiu uma vez");
	assert.ok(Math.abs(orcamento.remainingMs - 10) < 1, `a espera saiu do orçamento: sobrou ${orcamento.remainingMs}`);
	// A chamada seguinte do mesmo turno (depois de uma ferramenta, por exemplo) usa o MESMO orçamento, já gasto.
	let tentativasNaSegunda = 0;
	const pedidoQueNaoCabe = async () => {
		tentativasNaSegunda += 1;
		throw recusa();
	};
	await assert.rejects(
		retryProviderRequest(pedidoQueNaoCabe, { maxRetries: 2, retryBudget: orcamento }),
		/limite de uso/,
	);
	assert.equal(tentativasNaSegunda, 1, "orçamento do turno gasto: a repetição não começa");
});

test("a política de repetição é limitada e só repete a recusa de pedido não aceito", () => {
	assert.ok(MAX_TENTATIVAS >= 2 && MAX_TENTATIVAS <= 3, "poucas tentativas: 1 pedido e no máximo 2 repetições");
	assert.ok(ESPERA_MAXIMA_MS <= 30_000, "a espera de uma repetição não passa de 30 s");
	assert.ok(TETO_DE_ESPERA_DO_TURNO_MS <= 60_000, "o turno não espera repetições por mais de 60 s");
	assert.deepEqual([...STATUS_DE_REPETICAO], [408, 409, 429], "a lista é exatamente a recusa de pedido não aceito");
	for (const status of [400, 401, 403, 404, 413, 422, 500, 502, 503, 504]) {
		assert.ok(!STATUS_DE_REPETICAO.includes(status), `${status} pode vir depois de o fornecedor processar`);
	}
	assert.equal(OPCOES_DE_REPETICAO.retryWithoutStatus, false, "erro sem status não prova recusa");
	assert.equal(OPCOES_DE_REPETICAO.maxRetryDelayMs, ESPERA_MAXIMA_MS);
});
