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
import { HubTools, parseToolSpecs } from "./src/hub-tools.ts";
import { registerProviders } from "./src/providers.ts";
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
const models = createModels({
	credentials: new HubCredentialStore(channel, (key) => segredos.add(key)),
	authContext: emptyAuthContext(),
});
registerProviders(models);

const hubTools = new HubTools(write);
const agent = new Agent({
	streamFn: (model, context, options) => models.streamSimple(model, context, options),
	toolExecution: "parallel",
});

const stripPartial = (event: AssistantMessageEvent) => {
	if (!("partial" in event)) return event;
	const { partial: _partial, ...rest } = event;
	return rest;
};

agent.subscribe((event: AgentEvent) => {
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
		}
	},
	set_model(command) {
		const found = models.getModel(String(command.provider), String(command.modelId));
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
