// Servidores de mentira dos fornecedores, só para testes (--import). Nada sai da máquina. O cenário vem de
// MEXERICA_TEST_CENARIO: ok, invalida, sem_permissao, lenta, erro, quebrada. Toda resposta de erro repete a
// credencial recebida no corpo, como uma API que devolve a chave no erro: a saída do motor não pode contê-la.
// Cada chamada (url, método, cabeçalhos e o corpo JSON da conversa) vai para MEXERICA_MOTOR_FETCH_LOG.
import { appendFileSync } from "node:fs";

const log = process.env.MEXERICA_MOTOR_FETCH_LOG;
const cenario = process.env.MEXERICA_TEST_CENARIO ?? "ok";
const json = (corpo, status = 200) => new Response(JSON.stringify(corpo), { status, headers: { "content-type": "application/json" } });

const LISTAS = {
	"api.anthropic.com": () => ({ data: [
		{ type: "model", id: "claude-teste-grande", display_name: "Claude Teste Grande", max_input_tokens: 200000, max_tokens: 64000 },
		{ type: "model", id: "claude-teste-pequeno", display_name: "Claude Teste Pequeno" },
	], has_more: false }),
	"api.openai.com": () => ({ object: "list", data: [{ id: "gpt-teste" }, { id: "gpt-teste-mini" }, { id: "text-embedding-teste" }, { id: "gpt-teste-tts" }] }),
	"generativelanguage.googleapis.com": () => ({ models: [
		{ name: "models/gemini-teste", displayName: "Gemini Teste", supportedGenerationMethods: ["generateContent", "countTokens"], inputTokenLimit: 1048576, outputTokenLimit: 65536 },
		{ name: "models/embedding-teste", supportedGenerationMethods: ["embedContent"] },
	] }),
	"api.x.ai": () => ({ object: "list", data: [{ id: "grok-teste" }, { id: "grok-teste-image" }] }),
	"api.meta.ai": () => ({ object: "list", data: [{ id: "muse-teste-1.0" }, { id: "muse-teste-image" }, { id: "sam-teste" }] }),
	"openrouter.ai": () => ({ data: [
		{ id: "fornecedor/modelo-teste", name: "Modelo Teste", context_length: 32000, architecture: { output_modalities: ["text"] }, top_provider: { max_completion_tokens: 4096 } },
		{ id: "fornecedor/so-imagem", name: "Só Imagem", architecture: { output_modalities: ["image"] } },
	] }),
};

globalThis.fetch = async (input, init) => {
	const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url);
	const method = (init?.method ?? input?.method ?? "GET").toUpperCase();
	const headers = Object.fromEntries(new Headers(init?.headers ?? input?.headers ?? {}));
	const corpo = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
	if (log) appendFileSync(log, JSON.stringify({ url: url.href, method, headers, corpo }) + "\n");
	const credencial = headers.authorization ?? headers["x-api-key"] ?? headers["x-goog-api-key"] ?? "sem-credencial";
	const eco = (status, motivo) => json({ error: { message: `${motivo}: ${credencial}` } }, status);
	if (cenario === "lenta") {
		return new Promise((_resolve, reject) => init?.signal?.addEventListener("abort", () => reject(init.signal.reason)));
	}
	if (cenario === "invalida") return eco(url.hostname === "generativelanguage.googleapis.com" ? 400 : 401, "chave recusada");
	if (cenario === "sem_permissao") return eco(403, "sem permissão para este recurso");
	if (cenario === "erro") return eco(503, "fornecedor fora do ar");
	if (cenario === "eco_codificado") return json({ error: { message: `chave recusada: ${encodeURIComponent(credencial)}` } }, 400);
	if (method === "GET" && url.pathname.includes("/models")) {
		if (cenario === "quebrada") return new Response("<html>não é json</html>", { status: 200 });
		if (cenario === "enorme") return new Response(`{"data":[${'{"id":"x"},'.repeat(1_000_000)}{"id":"y"}]}`, { status: 200 });
		const lista = LISTAS[url.hostname];
		return lista ? json(lista()) : json({ error: { message: "rota desconhecida" } }, 404);
	}
	// Conversa: o modelo pedido não existe para esta chave.
	return eco(404, "modelo não encontrado");
};

