// Provedores da fatia 1: chave de API direta (Google ou OpenAI), vindas só do
// cofre do Hub. O provedor `faux` existe apenas sob MEXERICA_MOTOR_FAUX=1
// (testes locais do Hub), sem credencial e sem rede.
import {
	createProvider,
	envApiKeyAuth,
	fauxAssistantMessage,
	fauxProvider,
	fauxToolCall,
	type FauxProviderHandle,
	type FauxResponseFactory,
	type Model,
	type MutableModels,
	type Provider,
} from "@earendil-works/pi-ai";
import {
	stream as googleStream,
	streamSimple as googleStreamSimple,
} from "@earendil-works/pi-ai/api/google-generative-ai.ts";
import {
	stream as openaiStream,
	streamSimple as openaiStreamSimple,
} from "@earendil-works/pi-ai/api/openai-responses.ts";

const NO_COST = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

const GOOGLE_MODELS: Model<"google-generative-ai">[] = [
	{
		id: "gemini-2.5-flash",
		name: "Gemini 2.5 Flash",
		api: "google-generative-ai",
		provider: "google",
		baseUrl: "https://generativelanguage.googleapis.com/v1beta",
		input: ["text"],
		reasoning: true,
		cost: NO_COST,
		contextWindow: 1048576,
		maxTokens: 65536,
	},
	{
		id: "gemini-2.5-pro",
		name: "Gemini 2.5 Pro",
		api: "google-generative-ai",
		provider: "google",
		baseUrl: "https://generativelanguage.googleapis.com/v1beta",
		input: ["text"],
		reasoning: true,
		cost: NO_COST,
		contextWindow: 1048576,
		maxTokens: 65536,
	},
];

const OPENAI_MODELS: Model<"openai-responses">[] = [
	{
		id: "gpt-5",
		name: "GPT-5",
		api: "openai-responses",
		provider: "openai",
		baseUrl: "https://api.openai.com/v1",
		input: ["text"],
		reasoning: true,
		cost: NO_COST,
		contextWindow: 400000,
		maxTokens: 128000,
	},
	{
		id: "gpt-5-mini",
		name: "GPT-5 Mini",
		api: "openai-responses",
		provider: "openai",
		baseUrl: "https://api.openai.com/v1",
		input: ["text"],
		reasoning: true,
		cost: NO_COST,
		contextWindow: 400000,
		maxTokens: 128000,
	},
];

/** Registra google + openai (chave no cofre) e, sob a flag de teste, o faux. */
export function registerProviders(models: MutableModels): FauxProviderHandle | undefined {
	const providers: Provider[] = [
		createProvider({
			id: "google",
			name: "Google",
			baseUrl: "https://generativelanguage.googleapis.com/v1beta",
			auth: { apiKey: envApiKeyAuth("Gemini API key", ["GEMINI_API_KEY"]) },
			models: GOOGLE_MODELS,
			api: { stream: googleStream, streamSimple: googleStreamSimple },
		}),
		createProvider({
			id: "openai",
			name: "OpenAI",
			baseUrl: "https://api.openai.com/v1",
			auth: { apiKey: envApiKeyAuth("OpenAI API key", ["OPENAI_API_KEY"]) },
			models: OPENAI_MODELS,
			api: { stream: openaiStream, streamSimple: openaiStreamSimple },
		}),
	];
	for (const provider of providers) models.setProvider(provider);

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
	// `/contexto` responde `contexto: <mensagens> <bytes>` do que o provedor recebeu;
	// `/pedido` responde `pedido: <json>` com os textos de sistema e de usuário que o provedor recebeu.
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
			const orphans = kept.filter((m) => m.role === "toolResult" && !asked.has(m.toolCallId)).length;
			return fauxAssistantMessage(`contexto: ${kept.length} ${Buffer.byteLength(JSON.stringify(kept))} orfaos=${orphans}`);
		}
		if (text.includes("/pedido")) {
			const sistema = context.messages.filter((m) => m.role === "system").map(userText);
			return fauxAssistantMessage(`pedido: ${JSON.stringify({ sistema, usuario: users.map(userText) })}`);
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
