// Stub de rede só para testes (--import): registra cada chamada fetch em
// MEXERICA_MOTOR_FETCH_LOG (JSONL com url e headers) e responde 401 ecoando a
// credencial recebida — como uma API que devolve a chave no corpo do erro.
// Assim as provas conferem destino, credencial e contagem de chamadas sem rede.
import { appendFileSync } from "node:fs";

const log = process.env.MEXERICA_MOTOR_FETCH_LOG;
const calls = [];
globalThis.fetch = async (input, init) => {
	const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
	const headers = Object.fromEntries(new Headers(init?.headers ?? input?.headers ?? {}));
	calls.push({ url, headers });
	if (log) appendFileSync(log, JSON.stringify({ url, headers }) + "\n");
	const auth = headers.authorization ?? headers["x-goog-api-key"] ?? "sem-credencial";
	if (process.env.MEXERICA_TEST_STDERR_SPLIT === "1") {
		process.stderr.write(auth.slice(0, Math.floor(auth.length / 2)));
		process.stderr.write(auth.slice(Math.floor(auth.length / 2)));
	}
	if (process.env.MEXERICA_TEST_SUCCESS === "1") {
		const event = { candidates: [{ content: { role: "model", parts: [{ text: "resposta simulada" }] }, finishReason: "STOP", index: 0 }], usageMetadata: { promptTokenCount: 1, candidatesTokenCount: 1, totalTokenCount: 2 } };
		return new Response(`data: ${JSON.stringify(event)}\n\n`, { status: 200, headers: { "content-type": "text/event-stream" } });
	}
	return new Response(JSON.stringify({ error: { message: `chave recusada: ${auth}` } }), { status: 401 });
};
