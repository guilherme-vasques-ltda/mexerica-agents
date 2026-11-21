// Provedores por chave de API (Anthropic, OpenAI, Google, xAI e OpenRouter), com a chave vinda só do cofre do Hub.
// Nenhum modelo é fixo aqui: a lista vem do fornecedor e o modelo escolhido é montado em `fornecedores.ts`.
// O provedor `faux` existe apenas sob MEXERICA_MOTOR_FAUX=1 (testes locais do Hub), sem credencial e sem rede.
import {
	createProvider,
	envApiKeyAuth,
	fauxAssistantMessage,
	fauxProvider,
	fauxToolCall,
	type FauxProviderHandle,
	type Api,
	type FauxResponseFactory,
	type Model,
	type MutableModels,
	type Provider,
} from "@earendil-works/pi-ai";
import {
	stream as anthropicStream,
	streamSimple as anthropicStreamSimple,
} from "@earendil-works/pi-ai/api/anthropic-messages.ts";
import {
	stream as googleStream,
	streamSimple as googleStreamSimple,
} from "@earendil-works/pi-ai/api/google-generative-ai.ts";
import {
	stream as completionsStream,
	streamSimple as completionsStreamSimple,
} from "@earendil-works/pi-ai/api/openai-completions.ts";
import {
	stream as responsesStream,
	streamSimple as responsesStreamSimple,
} from "@earendil-works/pi-ai/api/openai-responses.ts";
import { FORNECEDORES, type FornecedorId } from "./fornecedores.ts";

const STREAMS = {
	"anthropic-messages": { stream: anthropicStream, streamSimple: anthropicStreamSimple },
	"google-generative-ai": { stream: googleStream, streamSimple: googleStreamSimple },
	"openai-completions": { stream: completionsStream, streamSimple: completionsStreamSimple },
	"openai-responses": { stream: responsesStream, streamSimple: responsesStreamSimple },
} as const;

/** Registra os fornecedores por chave (sem modelos fixos) e, sob a flag de teste, o faux. */
export function registerProviders(models: MutableModels): FauxProviderHandle | undefined {
	for (const [id, f] of Object.entries(FORNECEDORES) as [FornecedorId, (typeof FORNECEDORES)[FornecedorId]][]) {
		const streams = STREAMS[f.api as keyof typeof STREAMS];
		// A variável nomeada nunca é lida: o AuthContext do motor é vazio e a chave vem só do cofre.
		const provider: Provider = createProvider({
			id,
			name: f.nome,
			baseUrl: f.baseUrl,
			auth: { apiKey: envApiKeyAuth(`${f.nome} API key`, []) },
			models: [],
			api: streams,
		} as Parameters<typeof createProvider>[0]);
		models.setProvider(provider);
	}

	if (process.env["MEXERICA_MOTOR_FAUX"] !== "1") return undefined;
	// Dois provedores falsos (`faux` e `faux2`) para provar a troca de provedor.
	const userText = (m: { content?: unknown }): string => {
		const blocks = typeof m.content === "string" ? [{ type: "text", text: m.content }] : ((m.content ?? []) as { type: string; text?: string }[]);
		return blocks
			.filter((b) => b.type === "text")
			.map((b) => b.text ?? "")
			.join("\n");
	};
	const faux = fauxProvider();
	const faux2 = fauxProvider({ provider: "faux2" });
	// Eco do último texto do usuário, reenfileirado a cada turno. Extensões
	// de teste: `/ferramenta <nome> <json>` faz o "modelo" chamar essa ferramenta
	// e, de volta com o resultado, ele o repete (`ferramenta respondeu: ...`); `/cadeia <N> <nome> <json>`
	// chama a ferramenta N vezes em sequência (um turno longo) antes de responder;
	// `/paralelo <N> <nome> <json>` chama a ferramenta N vezes na MESMA mensagem; `/ferramentagrande <nome> <N>` chama a
	// ferramenta com um argumento `x` de N caracteres;
	// `/historico` responde com todos os textos de usuário que o modelo recebeu;
	// `/contexto` responde `contexto: <mensagens> <bytes>` do que o provedor recebeu.
	let chained = 0;
	let issued: number | undefined;
	const echo: FauxResponseFactory = async (context) => {
		faux.appendResponses([echo]);
		faux2.appendResponses([echo]);
		const tail = context.messages.at(-1);
		const lastUser = context.messages.filter((m) => m.role === "user").at(-1);
		const parallel = /^\/paralelo (\d+) (\S+) ([\s\S]*)$/.exec(lastUser ? userText(lastUser) : "");
		if (tail?.role === "user" && parallel && issued !== tail.timestamp) {
			issued = tail.timestamp;
			const calls = Array.from({ length: Number(parallel[1]) }, () => fauxToolCall(parallel[2]!, JSON.parse(parallel[3]!)));
			return fauxAssistantMessage(calls, { stopReason: "toolUse" });
		}
		const big = /^\/ferramentagrande (\S+) (\d+)$/.exec(lastUser ? userText(lastUser) : "");
		if (tail?.role === "user" && big) {
			return fauxAssistantMessage(fauxToolCall(big[1]!, { x: "a".repeat(Number(big[2])) }), { stopReason: "toolUse" });
		}
		const chain = /^\/cadeia (\d+) (\S+) ([\s\S]*)$/.exec(lastUser ? userText(lastUser) : "");
		if (tail?.role === "user") chained = 0;
		if (tail?.role === "toolResult" && chain && ++chained < Number(chain[1])) {
			return fauxAssistantMessage(fauxToolCall(chain[2]!, JSON.parse(chain[3]!)), { stopReason: "toolUse" });
		}
		if (tail?.role === "toolResult") {
			const out = tail.content.map((b) => (b.type === "text" ? b.text : "")).join("");
			return fauxAssistantMessage(`${tail.isError ? "ferramenta falhou" : "ferramenta respondeu"}: ${out}`);
		}
		const users = context.messages.filter((m) => m.role === "user");
		const text = users.length === 0 ? "" : userText(users[users.length - 1]!);
		if (text.includes("/contexto")) {
			const kept = context.messages.filter((m) => m.role !== "system");
			const asked = new Set(kept.flatMap((m) => (m.role === "assistant" ? m.content.filter((b) => b.type === "toolCall").map((b) => b.id) : [])));
			const declared = context.messages.flatMap((m) => (m.role === "system" ? ((m as { toolsAdded?: unknown[] }).toolsAdded ?? []) : [])).length;
			const orphans = kept.filter((m) => m.role === "toolResult" && !asked.has(m.toolCallId)).length;
			return fauxAssistantMessage(`contexto: ${kept.length} ${Buffer.byteLength(JSON.stringify(kept))} orfaos=${orphans} ferramentas=${declared}`);
		}
		if (text.includes("/historico")) return fauxAssistantMessage("historico: " + users.map(userText).join(" | "));
		const call = /^\/ferramenta (\S+) ([\s\S]*)$/.exec(text) ?? /^\/cadeia \d+ (\S+) ([\s\S]*)$/.exec(text);
		if (call) return fauxAssistantMessage(fauxToolCall(call[1]!, JSON.parse(call[2]!)), { stopReason: "toolUse" });
		return fauxAssistantMessage(`eco: ${text}`);
	};
	faux.setResponses([echo]);
	faux2.setResponses([echo]);
	models.setProvider(faux.provider);
	models.setProvider(faux2.provider);
	return faux;
}

/** «Testar acesso» do provedor falso (só testes do Hub): uma chave que começa com `recusada` é inválida. */
export function fauxAcesso(provider: string, key: string, modelos: readonly Model<Api>[]) {
	if (!key) return { ok: false, erro: "sem_chave" };
	if (key.startsWith("recusada")) return { ok: false, erro: "chave_invalida", status: 401 };
	const proprios = modelos.filter((m) => m.provider === provider);
	return { ok: true, modelos: proprios.map((m) => ({ id: m.id, name: m.name, contextWindow: m.contextWindow, maxTokens: m.maxTokens })) };
}
