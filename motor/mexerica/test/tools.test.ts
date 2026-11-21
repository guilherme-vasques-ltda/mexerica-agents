// Prova das ferramentas do Hub no motor (fatia 2): o contrato `set_tools` →
// `tool_request` → `tool_result`, a permissão negada, o cancelamento e a
// regra de segurança de que o motor NUNCA conecta a servidor MCP.
import { createServer } from "node:net";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { encerrar, startEngine, turnoTerminou, type Line, type Proc } from "./helpers.ts";

const FERRAMENTA = {
	name: "list_bots",
	description: "Lista os bots do Space",
	inputSchema: { type: "object", properties: { filtro: { type: "string" } } },
	origin: { kind: "hub" },
};

async function abrir(extra: Parameters<typeof startEngine>[1] = {}): Promise<Proc> {
	const proc = startEngine({ MEXERICA_MOTOR_FAUX: "1" }, extra);
	proc.send({ id: "m", type: "set_model", provider: "faux", modelId: "faux-1" });
	await proc.waitFor((v) => v.command === "set_model" && v.success === true);
	return proc;
}

let sequencia = 0;
async function declarar(proc: Proc, tools: object[]): Promise<Line> {
	const id = `t-${++sequencia}`;
	proc.send({ id, type: "set_tools", tools });
	return proc.waitFor((v) => v.id === id);
}

const textoFinal = (end: Line): string => {
	const messages = end.messages as { role: string; content: { type: string; text?: string }[] }[];
	return messages.at(-1)?.content.map((b) => b.text ?? "").join("") ?? "";
};

test("o modelo chama a ferramenta: o motor só pede e o Hub responde", async () => {
	const proc = await abrir();
	try {
		assert.equal((await declarar(proc, [FERRAMENTA])).success, true);
		proc.send({ id: "p", type: "prompt", message: '/ferramenta list_bots {"filtro":"x"}' });
		const pedido = await proc.waitFor((v) => v.type === "tool_request");
		assert.equal(pedido.name, "list_bots");
		assert.deepEqual(pedido.input, { filtro: "x" });
		assert.equal(typeof pedido.callId, "string");
		proc.send({ id: "r", type: "tool_result", callId: pedido.callId, ok: true, output: "bot-1, bot-2" });
		const end = await proc.waitFor((v) => v.type === "agent_end");
		assert.equal(textoFinal(end), "ferramenta respondeu: bot-1, bot-2");
	} finally {
		await encerrar(proc);
	}
});

test("permissão negada chega ao modelo como erro da ferramenta", async () => {
	const proc = await abrir();
	try {
		await declarar(proc, [FERRAMENTA]);
		proc.send({ id: "p", type: "prompt", message: "/ferramenta list_bots {}" });
		const pedido = await proc.waitFor((v) => v.type === "tool_request");
		proc.send({ type: "tool_result", callId: pedido.callId, ok: false, error: "permissão negada pelo dono" });
		const end = await proc.waitFor((v) => v.type === "agent_end");
		assert.equal(textoFinal(end), "ferramenta falhou: permissão negada pelo dono");
	} finally {
		await encerrar(proc);
	}
});

test("lista inválida é recusada inteira e set_tools substitui a lista toda", async () => {
	const proc = await abrir();
	try {
		for (const ruim of [
			[{ ...FERRAMENTA, name: "com espaço" }],
			[FERRAMENTA, FERRAMENTA],
			[{ ...FERRAMENTA, origin: { kind: "native" } }],
			[{ ...FERRAMENTA, inputSchema: "x" }],
			"nao-e-lista",
		]) {
			const res = await declarar(proc, ruim as object[]);
			assert.equal(res.success, false, JSON.stringify(ruim));
			proc.lines.length = 0;
		}
		await declarar(proc, [FERRAMENTA]);
		// Substitui: a ferramenta anterior some, e a chamada a ela não gera tool_request.
		await declarar(proc, [{ ...FERRAMENTA, name: "outra" }]);
		proc.send({ id: "p", type: "prompt", message: "/ferramenta list_bots {}" });
		const end = await proc.waitFor((v) => v.type === "agent_end");
		assert.ok(!proc.lines.some((v) => v.type === "tool_request"), "ferramenta removida não pode ser pedida");
		assert.match(textoFinal(end), /ferramenta falhou/);
	} finally {
		await encerrar(proc);
	}
});

test("tool_result com callId desconhecido ou repetido é recusado", async () => {
	const proc = await abrir();
	try {
		await declarar(proc, [FERRAMENTA]);
		proc.send({ id: "x", type: "tool_result", callId: "inventado", ok: true, output: "x" });
		assert.equal((await proc.waitFor((v) => v.id === "x")).success, false);
		proc.send({ id: "p", type: "prompt", message: "/ferramenta list_bots {}" });
		const pedido = await proc.waitFor((v) => v.type === "tool_request");
		proc.send({ type: "tool_result", callId: pedido.callId, ok: true, output: "ok" });
		await proc.waitFor((v) => v.type === "agent_end");
		proc.send({ id: "y", type: "tool_result", callId: pedido.callId, ok: true, output: "de novo" });
		assert.equal((await proc.waitFor((v) => v.id === "y")).success, false);
	} finally {
		await encerrar(proc);
	}
});

test("abort com ferramenta pendente encerra o turno sem esperar o Hub", async () => {
	const proc = await abrir();
	try {
		await declarar(proc, [FERRAMENTA]);
		proc.send({ id: "p", type: "prompt", message: "/ferramenta list_bots {}" });
		await proc.waitFor((v) => v.type === "tool_request");
		proc.send({ id: "a", type: "abort" });
		await proc.waitFor((v) => v.id === "a" && v.success === true);
		await proc.waitFor((v) => v.type === "agent_end");
	} finally {
		await encerrar(proc);
	}
});

test("set_tools durante o turno é recusado", async () => {
	const proc = await abrir();
	try {
		await declarar(proc, [FERRAMENTA]);
		proc.send({ id: "p", type: "prompt", message: "/ferramenta list_bots {}" });
		await proc.waitFor((v) => v.type === "tool_request");
		assert.equal((await declarar(proc, [])).success, false);
		proc.send({ id: "a", type: "abort" });
		await proc.waitFor((v) => v.type === "agent_end");
	} finally {
		await encerrar(proc);
	}
});

// ---- MCP inerte -----------------------------------------------------------

const MCP_CONFIGS: [string, string][] = [
	// Formatos de config MCP que o Pi e outros clientes leem de HOME e do cwd.
	[".pi/agent/mcp.json", "mcpServers"],
	[".pi/mcp.json", "mcpServers"],
	[".pi/settings.json", "mcpServers"],
	[".mcp.json", "mcpServers"],
	["config/pi/mcp.json", "mcpServers"],
	[".config/pi/mcp.json", "mcpServers"],
];

test("com config MCP do Pi na pasta do motor, nenhum servidor abre e a chamada só sai pelo Hub", async () => {
	const pasta = mkdtempSync(join(tmpdir(), "motor-mcp-"));
	const sentinela = join(pasta, "mcp-subiu");
	// Um "servidor MCP" por URL: qualquer conexão do motor a ele é registrada.
	const conexoes: string[] = [];
	const escuta = createServer((socket) => {
		conexoes.push("url");
		socket.destroy();
	});
	await new Promise<void>((ok) => escuta.listen(0, "127.0.0.1", ok));
	const porta = (escuta.address() as { port: number }).port;
	const servidores = {
		local: { command: process.execPath, args: ["-e", `require('fs').writeFileSync(${JSON.stringify(sentinela)}, 'x')`] },
		remoto: { url: `http://127.0.0.1:${porta}/mcp` },
	};
	for (const [arquivo, chave] of MCP_CONFIGS) {
		const alvo = join(pasta, arquivo);
		mkdirSync(join(alvo, ".."), { recursive: true });
		writeFileSync(alvo, JSON.stringify({ [chave]: servidores }));
	}
	const proc = startEngine(
		{ MEXERICA_MOTOR_FAUX: "1", HOME: pasta, XDG_CONFIG_HOME: join(pasta, "config"), PATH: process.env["PATH"] ?? "" },
		{ cwd: pasta },
	);
	try {
		proc.send({ id: "m", type: "set_model", provider: "faux", modelId: "faux-1" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		await declarar(proc, [FERRAMENTA]);
		proc.send({ id: "p", type: "prompt", message: "/ferramenta list_bots {}" });
		// A chamada só existe como pedido ao Hub...
		const pedido = await proc.waitFor((v) => v.type === "tool_request");
		assert.equal(pedido.name, "list_bots");
		// ...e o processo não tem filho (nenhum servidor MCP subiu) nem socket para o servidor.
		const pid = proc.child.pid!;
		const filhos = readdirSync(`/proc/${pid}/task`).flatMap((t) =>
			readFileSync(`/proc/${pid}/task/${t}/children`, "utf8").split(" ").filter(Boolean),
		);
		assert.deepEqual(filhos, [], "o motor abriu processo filho");
		proc.send({ type: "tool_result", callId: pedido.callId, ok: true, output: "via hub" });
		const end = await proc.waitFor((v) => v.type === "agent_end");
		assert.equal(textoFinal(end), "ferramenta respondeu: via hub");
		assert.equal(existsSync(sentinela), false, "o servidor MCP local foi executado");
		assert.deepEqual(conexoes, [], "o motor conectou na URL do servidor MCP");
		assert.ok(!proc.stderr().includes("mcp"), "o motor mencionou MCP no stderr");
	} finally {
		await encerrar(proc);
		escuta.close();
		rmSync(pasta, { recursive: true, force: true });
	}
});

test("o código do motor não tem caminho para abrir processo, MCP, rede ou ler config do Pi", () => {
	const raiz = fileURLToPath(new URL("..", import.meta.url));
	const fontes = ["rpc.ts", ...readdirSync(join(raiz, "src")).map((f) => `src/${f}`)];
	// Só o cofre (fd 3) usa `node:net`; ninguém abre processo, HTTP, DNS nem cliente MCP.
	const proibido = /child_process|node:(http|https|http2|tls|dgram|dns|cluster|worker_threads)\b|@modelcontextprotocol|\bmcp[-_]client|coding-agent\/src\/(core|config)|settings-manager|(?<![.\w])(spawn|exec|execFile|execSync)\(/i;
	// Exceção única e nomeada: `src/destino.ts` é o transporte do endereço próprio (pedido ao fornecedor por HTTP, com
	// o portão de rede: DNS e endereço da conexão conferidos). Processo, MCP e config do Pi continuam proibidos nele.
	const transporte = /child_process|node:(http2|dgram|cluster|worker_threads)\b|@modelcontextprotocol|\bmcp[-_]client|coding-agent\/src\/(core|config)|settings-manager|(?<![.\w])(spawn|exec|execFile|execSync)\(/i;
	for (const fonte of fontes) {
		const texto = readFileSync(join(raiz, fonte), "utf8");
		const regra = fonte === "src/destino.ts" ? transporte : proibido;
		assert.ok(!regra.test(texto), `${fonte} usa recurso proibido (${regra.exec(texto)?.[0]})`);
		if (fonte !== "src/vault-channel.ts" && fonte !== "src/destino.ts") assert.ok(!/node:net\b/.test(texto), `${fonte} importa node:net`);
	}
	// O transporte com rede própria só é usado pelo endereço próprio.
	const usos = fontes.filter((f) => f !== "src/destino.ts" && /fetchGuardado/.test(readFileSync(join(raiz, f), "utf8")));
	assert.deepEqual(usos, ["rpc.ts"]);
	assert.ok(statSync(join(raiz, "src/hub-tools.ts")).isFile());
});

test("a cobertura do turno termina mesmo se o Hub nunca responde ao pedido (prazo do abort)", async () => {
	const proc = await abrir();
	try {
		await declarar(proc, [FERRAMENTA]);
		proc.send({ id: "p", type: "prompt", message: "/ferramenta list_bots {}" });
		await proc.waitFor((v) => v.type === "tool_request");
		proc.send({ id: "n", type: "new_session" });
		await proc.waitFor((v) => v.id === "n" && v.success === true);
		await proc.waitFor(turnoTerminou);
	} finally {
		await encerrar(proc);
	}
});
