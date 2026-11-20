#!/usr/bin/env node
// Motor Mexerica — uma conversa por processo. Skills, memória e prompt vêm do
// harness do Hub (`set_system_prompt`), nunca de pastas nativas. As ferramentas
// também: o Hub declara a lista (`set_tools`), o motor só pede a chamada
// (`tool_request`) e espera o `tool_result`; ele NÃO executa nada e NÃO fala
// com servidor MCP (nenhum spawn, socket ou URL de MCP, nenhuma config nativa
// do Pi é lida). Protocolo: subconjunto do modo RPC do Pi (JSONL em
// stdin/stdout: comandos, respostas `{type:"response"}` e eventos do agente).
// A credencial chega SOMENTE pelo canal do cofre (fd 3, JSON-RPC
// `credential.read`/`credential.modify`); o AuthContext é vazio e o ambiente
// chega higienizado pelo Hub.
import { Agent, type AgentEvent } from "@earendil-works/pi-agent-core";
import { createModels, type AssistantMessageEvent, type Model } from "@earendil-works/pi-ai";
import { attachJsonlLineReader, serializeJsonLine } from "../vendor/pi-mono/packages/coding-agent/src/modes/rpc/jsonl.ts";
import { emptyAuthContext } from "./src/auth-context.ts";
import { HubCredentialStore } from "./src/credentials.ts";
import { parseHistory } from "./src/history.ts";
import { boundInPlace, fitMessages } from "./src/limits.ts";
import { HubTools, parseToolSpecs } from "./src/hub-tools.ts";
import { fauxAcesso, registerProviders } from "./src/providers.ts";
import { comPrazoDeSilencio } from "./src/silencio.ts";
import { ehFornecedor, modeloDoFornecedor, testarAcesso } from "./src/fornecedores.ts";
import { VaultChannel } from "./src/vault-channel.ts";

type Command = { id?: string; type: string } & Record<string, unknown>;

// Eventos JSON completos têm chaves conhecidas mascaradas. Stderr pode
// fragmentar uma chave entre escritas: seu conteúdo é omitido integralmente.
const segredos = new Set<string>();
const saneado = (texto: string): string => {
	for (const segredo of segredos) texto = texto.split(segredo).join("***");
	return texto;
};
const write = (obj: unknown) => {
	process.stdout.write(saneado(serializeJsonLine(obj)));
};
const stderrWrite = process.stderr.write.bind(process.stderr) as (...args: unknown[]) => boolean;
process.stderr.write = ((_chunk: unknown, ...rest: unknown[]) =>
	stderrWrite("[diagnóstico omitido]\n", ...rest)) as typeof process.stderr.write;
const respond = (id: string | undefined, command: string, data?: object) =>
	write({ id, type: "response", command, success: true, ...(data === undefined ? {} : { data }) });
const fail = (id: string | undefined, command: string, error: string) =>
	write({ id, type: "response", command, success: false, error });

const channel = VaultChannel.open(); // falha fechado: sem fd 3 não há cofre
const credentials = new HubCredentialStore(channel, (key) => segredos.add(key));
const models = createModels({ credentials, authContext: emptyAuthContext() });
const fauxHandle = registerProviders(models);

const hubTools = new HubTools(write);
// Um único fetch para todos os pedidos (o cliente da Anthropic é reaproveitado enquanto o fetch é o mesmo).
const fetchDoFornecedor = comPrazoDeSilencio((input, init) => globalThis.fetch(input, init));
/** «Testar acesso» é curto: 15 s sem resposta já é «sem resposta», e o teste inteiro não passa de 30 s. Os testes do
 * motor encurtam o prazo por MEXERICA_TESTE_PRAZO_MS; o Hub abre o motor com o ambiente limpo, sem ela. */
const prazoDeTeste = Number(process.env["MEXERICA_TESTE_PRAZO_MS"]) || 15_000;
const fetchDoTeste = comPrazoDeSilencio((input, init) => globalThis.fetch(input, init), prazoDeTeste);
const PRAZO_DO_TESTE_MS = prazoDeTeste * 2;
const agent = new Agent({
	streamFn: (model, context, options) => models.streamSimple(model, context, { ...options, fetch: fetchDoFornecedor }),
	toolExecution: "parallel",
	// Os tetos da conversa valem em cada chamada ao provedor, também no meio de um turno gigante. O corte vale
	// também para a lista do próprio laço (ela é a mesma que chega aqui), que então não cresce sem limite.
	transformContext: async (messages) => {
		const fitted = fitMessages(messages);
		messages.splice(0, messages.length, ...fitted);
		return fitted;
	},
	// A lista do turno (`newMessages`, a que o `agent_end` leva) é limitada a cada volta do laço, junto com o contexto:
	// cada mensagem já entra dentro do teto (`boundInPlace` no `message_end`) e aqui sai a volta mais antiga que passar.
	finishTurn: ({ newMessages, context }) => {
		newMessages.splice(0, newMessages.length, ...fitMessages(newMessages));
		context.messages.splice(0, context.messages.length, ...fitMessages(context.messages));
	},
});

const stripPartial = (event: AssistantMessageEvent) => {
	if (!("partial" in event)) return event;
	const { partial: _partial, ...rest } = event;
	return rest;
};

agent.subscribe((event: AgentEvent) => {
	// A mensagem sai e fica no acumulador do laço já dentro do teto por mensagem (mesmo objeto).
	if (event.type === "message_start" || event.type === "message_end") boundInPlace(event.message);
	if (event.type === "message_update") {
		// Só o delta: `message` leva a mensagem inteira a cada evento e a
		// saída cresceria ao quadrado do tamanho da resposta.
		write({
			type: "message_update",
			assistantMessageEvent: stripPartial(event.assistantMessageEvent),
		});
		return;
	}
	write(event);
	// O estado do processo também não passa dos tetos durante o turno, não só entre turnos.
	if (event.type === "message_end") agent.state.messages = fitMessages(agent.state.messages);
});

let streaming = false;
let model: Model<string> | undefined;

const handlers: Record<string, (command: Command) => Promise<void> | void> = {
	async prompt(command) {
		if (!model) throw new Error("nenhum modelo escolhido (set_model antes do prompt)");
		respond(command.id, "prompt", { disposition: "started" });
		streaming = true;
		try {
			await agent.prompt(String(command.message ?? ""));
		} catch (error) {
			write({ type: "error", error: error instanceof Error ? error.message : String(error) });
		} finally {
			streaming = false;
			// O processo vivo também respeita os tetos entre turnos.
			agent.state.messages = fitMessages(agent.state.messages);
		}
	},
	set_model(command) {
		const provider = String(command.provider);
		const modelId = String(command.modelId);
		const found = models.getModel(provider, modelId) ?? (ehFornecedor(provider) ? modeloDoFornecedor(provider, modelId) : undefined);
		if (!found) throw new Error(`modelo desconhecido: ${command.provider}/${command.modelId}`);
		model = found;
		agent.state.model = found;
		respond(command.id, "set_model", { model: found });
	},
	get_state(command) {
		respond(command.id, "get_state", {
			model,
			isStreaming: streaming || agent.state.isStreaming,
			messageCount: agent.state.messages.length,
		});
	},
	new_session(command) {
		agent.abort();
		hubTools.failAll("sessão reiniciada");
		agent.state.messages = [];
		respond(command.id, "new_session", { cancelled: true });
	},
	// «Testar acesso»: só lista os modelos que a chave do cofre enxerga. A resposta nunca leva corpo do fornecedor.
	async list_models(command) {
		const provider = String(command.provider);
		const faux = fauxHandle !== undefined && (provider === "faux" || provider === "faux2");
		if (!ehFornecedor(provider) && !faux) throw new Error(`fornecedor desconhecido: ${provider}`);
		const credential = await credentials.read(provider);
		const key = (credential as { key?: unknown } | undefined)?.key;
		if (typeof key !== "string" || !key) return respond(command.id, "list_models", { ok: false, erro: "sem_chave" });
		if (!ehFornecedor(provider)) return respond(command.id, "list_models", fauxAcesso(provider, key, models.getModels(provider)));
		respond(command.id, "list_models", await testarAcesso(provider, key, fetchDoTeste, AbortSignal.timeout(PRAZO_DO_TESTE_MS)));
	},
	get_available_models(command) {
		respond(command.id, "get_available_models", { models: models.getModels() });
	},
	set_system_prompt(command) {
		if (streaming || agent.state.isStreaming) throw new Error("set_system_prompt só vale fora de um turno");
		if (typeof command.prompt !== "string") throw new Error("set_system_prompt exige `prompt` texto");
		// O prompt vive na transcrição: uma mensagem de sistema nova o substitui.
		agent.state.messages = [...agent.state.messages, { role: "system", content: command.prompt, timestamp: Date.now() }];
		respond(command.id, "set_system_prompt");
	},
	// Substitui a lista inteira; o Hub a manda antes do prompt, nunca durante o turno.
	set_tools(command) {
		if (streaming || agent.state.isStreaming) throw new Error("set_tools só vale fora de um turno");
		const specs = parseToolSpecs(command.tools);
		agent.state.tools = hubTools.toAgentTools(specs);
		respond(command.id, "set_tools", { count: specs.length });
	},
	// O Hub guarda a conversa; o processo novo (depois de cancelar, trocar de
	// provedor ou cair) a recebe aqui, depois do prompt de sistema e antes do prompt.
	restore_history(command) {
		if (streaming || agent.state.isStreaming) throw new Error("restore_history só vale fora de um turno");
		const history = parseHistory(command.messages);
		agent.state.messages = [...agent.state.messages.filter((message) => message.role === "system"), ...history];
		respond(command.id, "restore_history", { count: history.length });
	},
	tool_result(command) {
		const { callId, ok, output, error } = command as unknown as Record<string, unknown>;
		if (typeof callId !== "string" || typeof ok !== "boolean") throw new Error("tool_result inválido");
		hubTools.resolve({ callId, ok, output, error });
		respond(command.id, "tool_result");
	},
	abort(command) {
		agent.abort();
		hubTools.failAll("turno cancelado");
		respond(command.id, "abort");
	},
};

attachJsonlLineReader(process.stdin, (line) => {
	let command: Command;
	try {
		command = JSON.parse(line);
	} catch {
		return write({ type: "response", command: "parse", success: false, error: "linha não é JSON" });
	}
	const handle = handlers[command.type];
	if (!handle) return fail(command.id, command.type, `comando não suportado nesta fatia: ${command.type}`);
	Promise.resolve()
		.then(() => handle(command))
		.catch((error) => fail(command.id, command.type, error instanceof Error ? error.message : String(error)));
});

process.stdin.on("end", () => process.exit(0));
