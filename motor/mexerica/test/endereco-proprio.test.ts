// Endereço próprio: o portão de rede (destino.ts) e o motor falando com um serviço local de mentira nos três formatos.
// Nenhuma rede externa: os servidores sobem em 127.0.0.1, que só vale com a opção «local» do endereço.
import assert from "node:assert/strict";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { test } from "node:test";
import { conferirUrl, DestinoRecusado, fetchGuardado, ipPermitido } from "../src/destino.ts";
import { encerrar, startEngine, turnoTerminou, type Line } from "./helpers.ts";

const SECRET = "chave-de-mentira-do-endereco-proprio-9";
const PROVEDOR = "proprio-abcdef12";

test("endereços de rede: público passa; privado só com a opção local; metadados e link-local nunca", () => {
	const casos: [string, boolean, boolean][] = [
		["8.8.8.8", true, true],
		["2606:4700:4700::1111", true, true],
		["127.0.0.1", false, true],
		["10.1.2.3", false, true], // vazamento-ok: endereço reservado em teste de recusa
		["172.20.0.1", false, true], // vazamento-ok: endereço reservado em teste de recusa
		["192.168.1.10", false, true], // vazamento-ok: endereço reservado em teste de recusa
		["100.64.0.1", false, true], // vazamento-ok: endereço reservado em teste de recusa
		["::1", false, true],
		["fd12::1", false, true], // vazamento-ok: endereço reservado em teste de recusa
		["::ffff:127.0.0.1", false, true],
		["::ffff:7f00:1", false, true],
		["169.254.169.254", false, false], // vazamento-ok: endereço reservado em teste de recusa
		["64:ff9b::a9fe:a9fe", false, false],
		["::ffff:169.254.169.254", false, false], // vazamento-ok: endereço reservado em teste de recusa
		["fd00:ec2::254", false, false], // vazamento-ok: endereço reservado em teste de recusa
		["100.100.100.200", false, false], // vazamento-ok: endereço reservado em teste de recusa
		["fe80::1", false, false], // vazamento-ok: endereço reservado em teste de recusa
		["0.0.0.0", false, false],
		["224.0.0.1", false, false],
		["não-é-ip", false, false],
	];
	for (const [ip, publico, local] of casos) {
		assert.equal(ipPermitido(ip, false), publico, `${ip} sem a opção local`);
		assert.equal(ipPermitido(ip, true), local, `${ip} com a opção local`);
	}
});

test("endereço base: só HTTPS, sem credencial, consulta ou fragmento; HTTP só com a opção local", () => {
	assert.equal(conferirUrl("https://api.example.invalid/v1", false).hostname, "api.example.invalid");
	assert.equal(conferirUrl("http://127.0.0.1:11434/v1", true).port, "11434");
	for (const [url, local] of [
		["http://api.example.invalid/v1", false],
		["https://usuario:senha@api.example.invalid/v1", false],
		["https://api.example.invalid/v1?key=abc", false],
		["https://api.example.invalid/v1#x", false],
		["https://127.0.0.1/v1", false],
		["https://[::1]/v1", false],
		["http://169.254.169.254/latest", true], // vazamento-ok: endereço reservado em teste de recusa
		["ftp://api.example.invalid", true],
		["não é endereço", true],
	] as const) {
		assert.throws(() => conferirUrl(url, local), DestinoRecusado, url);
	}
});

const ouvir = (servidor: Server) =>
	new Promise<number>((resolve) => servidor.listen(0, "127.0.0.1", () => resolve((servidor.address() as AddressInfo).port)));

interface Pedido {
	method: string;
	path: string;
	headers: IncomingMessage["headers"];
	body: string;
}

/** Um serviço de mentira que anota cada pedido e responde pela rota. */
async function servico(rotas: (p: Pedido, res: ServerResponse) => void) {
	const pedidos: Pedido[] = [];
	const servidor = createServer((req, res) => {
		let body = "";
		req.on("data", (c) => (body += c));
		req.on("end", () => {
			const p = { method: req.method ?? "", path: req.url ?? "", headers: req.headers, body };
			pedidos.push(p);
			rotas(p, res);
		});
	});
	const porta = await ouvir(servidor);
	return { pedidos, porta, base: `http://127.0.0.1:${porta}`, fechar: () => servidor.close() };
}

const json = (res: ServerResponse, corpo: unknown, status = 200) => {
	res.writeHead(status, { "content-type": "application/json" });
	res.end(JSON.stringify(corpo));
};

test("sem a opção local, o nome que resolve para a própria máquina é recusado antes de conectar", async () => {
	const s = await servico((_p, res) => json(res, {}));
	try {
		await assert.rejects(fetchGuardado(false)(`https://localhost:${s.porta}/v1/models`), DestinoRecusado);
		await assert.rejects(fetchGuardado(false)(`${s.base}/v1/models`), DestinoRecusado);
		assert.equal(s.pedidos.length, 0, "nada pode chegar ao serviço local");
	} finally {
		s.fechar();
	}
});

test("com a opção local, o pedido chega; redirecionamento é recusado e não leva a chave", async () => {
	const outro = await servico((_p, res) => json(res, {}));
	const s = await servico((p, res) => {
		if (p.path === "/fuga") {
			res.writeHead(307, { location: `${outro.base}/roubar` });
			return res.end();
		}
		json(res, { eco: p.body });
	});
	try {
		const fetch = fetchGuardado(true);
		const resposta = await fetch(`${s.base}/v1/chat`, { method: "POST", headers: { authorization: `Bearer ${SECRET}` }, body: "oi" });
		assert.deepEqual(await resposta.json(), { eco: "oi" });
		assert.equal(s.pedidos[0]!.headers.authorization, `Bearer ${SECRET}`);
		await assert.rejects(fetch(`${s.base}/fuga`, { headers: { "x-api-key": SECRET } }));
		assert.equal(outro.pedidos.length, 0, "o redirecionamento não pode ser seguido");
		await assert.rejects(fetchGuardado(true)("http://169.254.169.254/latest/meta-data"), DestinoRecusado); // vazamento-ok: endereço reservado em teste de recusa
	} finally {
		s.fechar();
		outro.fechar();
	}
});

function motorCom(endpoint: object) {
	return startEngine({ MEXERICA_TESTE_PRAZO_MS: "1000" }, { serveCredential: { type: "api_key", key: SECRET, endpoint } });
}

async function testar(proc: ReturnType<typeof startEngine>) {
	proc.send({ id: "t", type: "list_models", provider: PROVEDOR });
	const resposta = await proc.waitFor((v: Line) => v.id === "t" && v.type === "response");
	assert.equal(resposta.success, true, JSON.stringify(resposta));
	return resposta.data as Record<string, unknown>;
}

async function semVazar(proc: ReturnType<typeof startEngine>) {
	await encerrar(proc);
	assert.ok(!JSON.stringify(proc.lines).includes(SECRET), "a chave vazou na saída do motor");
	assert.ok(!proc.stderr().includes(SECRET), "a chave vazou no stderr do motor");
}

test("«Testar acesso» nos formatos OpenAI lista {base}/models com a chave", async () => {
	const s = await servico((p, res) => (p.path === "/v1/models" ? json(res, { data: [{ id: "modelo-local" }, { id: "dono/outro:tag" }] }) : json(res, {}, 404)));
	const proc = motorCom({ baseUrl: `${s.base}/v1`, formato: "chat-completions", local: true });
	try {
		const acesso = await testar(proc);
		assert.equal(acesso.ok, true, JSON.stringify(acesso));
		assert.deepEqual((acesso.modelos as { id: string }[]).map((m) => m.id), ["modelo-local", "dono/outro:tag"]);
		assert.equal(s.pedidos[0]!.method, "GET");
		assert.equal(s.pedidos[0]!.headers.authorization, `Bearer ${SECRET}`);
	} finally {
		await semVazar(proc);
		s.fechar();
	}
});

test("«Testar acesso» no formato Anthropic Messages usa /v1/models com x-api-key", async () => {
	const s = await servico((p, res) => (p.path.startsWith("/v1/models") ? json(res, { data: [{ id: "claude-local" }], has_more: false }) : json(res, {}, 404)));
	const proc = motorCom({ baseUrl: s.base, formato: "anthropic-messages", local: true });
	try {
		const acesso = await testar(proc);
		assert.equal(acesso.ok, true, JSON.stringify(acesso));
		assert.deepEqual((acesso.modelos as { id: string }[]).map((m) => m.id), ["claude-local"]);
		assert.equal(s.pedidos[0]!.headers["x-api-key"], SECRET);
		assert.ok(s.pedidos[0]!.headers["anthropic-version"]);
	} finally {
		await semVazar(proc);
		s.fechar();
	}
});

test("serviço sem lista de modelos devolve «sem_lista», não chave inválida", async () => {
	const s = await servico((_p, res) => json(res, { error: { message: `não existe: ${SECRET}` } }, 404));
	const proc = motorCom({ baseUrl: `${s.base}/v1`, formato: "responses", local: true });
	try {
		assert.deepEqual(await testar(proc), { ok: false, erro: "sem_lista", status: 404 });
	} finally {
		await semVazar(proc);
		s.fechar();
	}
});

test("endereço local sem a opção local é recusado sem nenhum pedido", async () => {
	const s = await servico((_p, res) => json(res, { data: [] }));
	const proc = motorCom({ baseUrl: `${s.base}/v1`, formato: "chat-completions", local: false });
	try {
		assert.deepEqual(await testar(proc), { ok: false, erro: "destino_recusado" });
		assert.equal(s.pedidos.length, 0);
	} finally {
		await semVazar(proc);
		s.fechar();
	}
});

test("conversa pelo formato Chat Completions chega ao endereço próprio com a chave e volta com a resposta", async () => {
	const s = await servico((p, res) => {
		if (p.path !== "/v1/chat/completions") return json(res, {}, 404);
		res.writeHead(200, { "content-type": "text/event-stream" });
		const pedaco = (delta: object, fim: string | null) =>
			`data: ${JSON.stringify({ id: "c1", object: "chat.completion.chunk", created: 1, model: "modelo-local", choices: [{ index: 0, delta, finish_reason: fim }] })}\n\n`;
		res.end(pedaco({ role: "assistant", content: "olá do endereço próprio" }, null) + pedaco({}, "stop") + "data: [DONE]\n\n");
	});
	const proc = motorCom({ baseUrl: `${s.base}/v1`, formato: "chat-completions", local: true });
	try {
		proc.send({ id: "m", type: "set_model", provider: PROVEDOR, modelId: "modelo-local" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		const fim = await proc.waitFor(turnoTerminou);
		const mensagens = (fim.messages ?? []) as { content?: { type: string; text?: string }[] }[];
		const texto = mensagens.at(-1)?.content?.map((b) => b.text ?? "").join("") ?? "";
		assert.equal(texto, "olá do endereço próprio", JSON.stringify(fim));
		const conversa = s.pedidos.find((p) => p.method === "POST")!;
		assert.equal(conversa.headers.authorization, `Bearer ${SECRET}`);
		assert.equal(JSON.parse(conversa.body).model, "modelo-local");
	} finally {
		await semVazar(proc);
		s.fechar();
	}
});

test("conversa redirecionada pelo endereço próprio termina com erro e não chega ao outro endereço", async () => {
	const outro = await servico((_p, res) => json(res, {}));
	const s = await servico((_p, res) => {
		res.writeHead(307, { location: `${outro.base}/v1/messages` });
		res.end();
	});
	const proc = motorCom({ baseUrl: s.base, formato: "anthropic-messages", local: true });
	try {
		proc.send({ id: "m", type: "set_model", provider: PROVEDOR, modelId: "claude-local" });
		await proc.waitFor((v) => v.command === "set_model" && v.success === true);
		proc.send({ id: "p", type: "prompt", message: "oi" });
		await proc.waitFor(turnoTerminou);
		assert.ok(s.pedidos[0]!.path.startsWith("/v1/messages"), s.pedidos[0]!.path);
		assert.equal(outro.pedidos.length, 0, "a conversa seguiu o redirecionamento");
	} finally {
		await semVazar(proc);
		s.fechar();
		outro.fechar();
	}
});

test("configuração do endereço inválida no cofre recusa o modelo", async () => {
	const proc = motorCom({ baseUrl: "https://usuario:senha@api.example.invalid/v1", formato: "responses", local: false });
	try {
		proc.send({ id: "m", type: "set_model", provider: PROVEDOR, modelId: "x" });
		const resposta = await proc.waitFor((v) => v.id === "m");
		assert.equal(resposta.success, false);
	} finally {
		await semVazar(proc);
	}
});
