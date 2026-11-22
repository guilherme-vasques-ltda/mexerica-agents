// Fornecedores por chave de API do Motor Mexerica: endereço, formato da conversa e como listar os modelos que a
// chave enxerga. A lista não é escrita aqui: vem do próprio fornecedor («Testar acesso»), e o modelo escolhido é
// montado a partir do fornecedor. Listar modelos não gera cobrança; nenhuma conversa é aberta para testar.
import type { Api, Model } from "@earendil-works/pi-ai";
import { conferirUrl, DestinoRecusado } from "./destino.ts";

export type FornecedorId = "anthropic" | "openai" | "google" | "xai" | "openrouter" | "meta";

export interface ModeloListado {
	id: string;
	name: string;
	contextWindow?: number;
	maxTokens?: number;
}

export interface Fornecedor {
	nome: string;
	/** O Google responde 400 para chave que não existe. */
	chaveInvalidaEm400?: boolean;
	/** Endereço próprio: lista de modelos ausente (404/405) não é erro de chave; a pessoa digita o modelo. */
	listaOpcional?: boolean;
	api: Api;
	baseUrl: string;
	contextWindow: number;
	maxTokens: number;
	/** Primeira página da lista de modelos e o cabeçalho de autenticação. */
	lista: (chave: string, pagina?: string) => { url: string; headers: Record<string, string> };
	/** Modelos de conversa da página e o marcador da próxima, se houver. */
	ler: (corpo: unknown) => { modelos: ModeloListado[]; proxima?: string };
}

const bearer = (chave: string) => ({ authorization: `Bearer ${chave}` });
type Objeto = Record<string, unknown>;
const lista = (corpo: unknown, campo: string): Objeto[] => {
	const valor = (corpo as Objeto | null)?.[campo];
	if (!Array.isArray(valor)) throw new RespostaInvalida();
	return valor.filter((item): item is Objeto => typeof item === "object" && item !== null);
};
const texto = (valor: unknown): string | undefined => (typeof valor === "string" && valor ? valor : undefined);
const numero = (valor: unknown): number | undefined => (typeof valor === "number" && valor > 0 ? valor : undefined);
const comId = (item: Objeto, extra: Partial<ModeloListado> = {}): ModeloListado[] => {
	const id = texto(item["id"]);
	if (!id) return [];
	const nome = texto(item["display_name"]) ?? texto(item["name"]) ?? id;
	return [{ id, name: nome, ...extra }];
};

/** O motor guarda a conversa: nenhum modelo usa a conversa no servidor do fornecedor (threads). Sem isso, a
 * identidade de cache enviada como identidade de sessão ligaria o beta de threads da Anthropic, que guardaria estado
 * do lado de lá e reenviaria o pedido por conta própria. */
const SEM_THREADS = { supportsMessageThreads: false } as const;

/** Modelos que não conversam (voz, imagem, vetores, moderação): ficam fora da escolha do bot. */
const NAO_CONVERSA = /(embed|tts|whisper|transcribe|dall-e|image|imagen|imagine|veo|video|audio|realtime|moderation|aqa|sora|babbage|davinci|computer-use|segment-anything|^sam-)/i;

export const FORNECEDORES: Record<FornecedorId, Fornecedor> = {
	anthropic: {
		nome: "Anthropic",
		api: "anthropic-messages",
		baseUrl: "https://api.anthropic.com",
		contextWindow: 200000,
		maxTokens: 8192,
		lista: (chave, pagina) => ({
			url: `https://api.anthropic.com/v1/models?limit=1000${pagina ? `&after_id=${encodeURIComponent(pagina)}` : ""}`,
			headers: { "x-api-key": chave, "anthropic-version": "2023-06-01" },
		}),
		ler: (corpo) => {
			const dados = lista(corpo, "data");
			const temMais = (corpo as Objeto)["has_more"] === true;
			const proxima = temMais ? texto((corpo as Objeto)["last_id"]) : undefined;
			if (temMais && !proxima) throw new RespostaInvalida();
			return {
				modelos: dados.flatMap((item) => comId(item, { contextWindow: numero(item["max_input_tokens"]), maxTokens: numero(item["max_tokens"]) })),
				proxima,
			};
		},
	},
	openai: {
		nome: "OpenAI",
		api: "openai-responses",
		baseUrl: "https://api.openai.com/v1",
		contextWindow: 128000,
		maxTokens: 16384,
		lista: (chave) => ({ url: "https://api.openai.com/v1/models", headers: bearer(chave) }),
		ler: (corpo) => ({ modelos: lista(corpo, "data").flatMap((item) => comId(item)).filter((m) => !NAO_CONVERSA.test(m.id)) }),
	},
	google: {
		nome: "Google",
		chaveInvalidaEm400: true,
		api: "google-generative-ai",
		baseUrl: "https://generativelanguage.googleapis.com/v1beta",
		contextWindow: 1048576,
		maxTokens: 8192,
		lista: (chave, pagina) => ({
			url: `https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000${pagina ? `&pageToken=${encodeURIComponent(pagina)}` : ""}`,
			headers: { "x-goog-api-key": chave },
		}),
		ler: (corpo) => ({
			modelos: lista(corpo, "models").flatMap((item) => {
				const metodos = item["supportedGenerationMethods"];
				const id = texto(item["name"])?.replace(/^models\//, "");
				if (!id || !Array.isArray(metodos) || !metodos.includes("generateContent") || NAO_CONVERSA.test(id)) return [];
				return [{ id, name: texto(item["displayName"]) ?? id, contextWindow: numero(item["inputTokenLimit"]), maxTokens: numero(item["outputTokenLimit"]) }];
			}),
			proxima: texto((corpo as Objeto)["nextPageToken"]),
		}),
	},
	xai: {
		nome: "xAI",
		api: "openai-completions",
		baseUrl: "https://api.x.ai/v1",
		contextWindow: 131072,
		maxTokens: 16384,
		lista: (chave) => ({ url: "https://api.x.ai/v1/models", headers: bearer(chave) }),
		ler: (corpo) => ({
			modelos: lista(corpo, "data")
				.flatMap((item) => comId(item, { contextWindow: numero(item["context_length"]) }))
				.filter((m) => !NAO_CONVERSA.test(m.id)),
		}),
	},
	openrouter: {
		nome: "OpenRouter",
		api: "openai-completions",
		baseUrl: "https://openrouter.ai/api/v1",
		contextWindow: 128000,
		maxTokens: 16384,
		// Só os modelos que esta chave pode usar (a lista geral do OpenRouter é pública e não prova a chave).
		lista: (chave) => ({ url: "https://openrouter.ai/api/v1/models/user", headers: bearer(chave) }),
		ler: (corpo) => ({
			modelos: lista(corpo, "data").flatMap((item) => {
				const saidas = (item["architecture"] as Objeto | undefined)?.["output_modalities"];
				if (Array.isArray(saidas) && !saidas.includes("text")) return [];
				const topo = item["top_provider"] as Objeto | undefined;
				return comId(item, { contextWindow: numero(item["context_length"]), maxTokens: numero(topo?.["max_completion_tokens"]) });
			}),
		}),
	},
	meta: {
		nome: "Meta",
		// Meta Model API: compatível com Chat Completions, Responses e Anthropic Messages; aqui, Chat Completions.
		api: "openai-completions",
		baseUrl: "https://api.meta.ai/v1",
		contextWindow: 1048576,
		maxTokens: 16384,
		lista: (chave) => ({ url: "https://api.meta.ai/v1/models", headers: bearer(chave) }),
		ler: (corpo) => ({ modelos: lista(corpo, "data").flatMap((item) => comId(item)).filter((m) => !NAO_CONVERSA.test(m.id)) }),
	},
};

export const ehFornecedor = (id: string): id is FornecedorId => Object.hasOwn(FORNECEDORES, id);

/** Id de modelo aceitável: só o OpenRouter usa barra (`dono/modelo`). */
export function modeloValido(fornecedor: FornecedorId, id: string): boolean {
	// O Google põe o id no caminho (`models/<id>:streamGenerateContent`): ali, nem `:`.
	const permitido =
		fornecedor === "openrouter"
			? /^[A-Za-z0-9][A-Za-z0-9._:~\/-]{0,199}$/
			: fornecedor === "google"
				? /^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$/
				: /^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$/;
	return permitido.test(id) && !id.includes("..") && !id.includes("//");
}

/** O modelo escolhido, montado a partir do fornecedor; sem controle de raciocínio (cada modelo usa o seu padrão). */
export function modeloDoFornecedor(fornecedor: FornecedorId, id: string, listado?: ModeloListado): Model<Api> | undefined {
	if (!modeloValido(fornecedor, id)) return undefined;
	const f = FORNECEDORES[fornecedor];
	return {
		id,
		name: listado?.name ?? id,
		api: f.api,
		provider: fornecedor,
		baseUrl: f.baseUrl,
		compat: SEM_THREADS,
		input: ["text"],
		reasoning: false,
		cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
		contextWindow: listado?.contextWindow ?? f.contextWindow,
		maxTokens: listado?.maxTokens ?? f.maxTokens,
	};
}

export type ErroDeAcesso =
	| "chave_invalida"
	| "sem_permissao"
	| "limite"
	| "fornecedor_falhou"
	| "sem_resposta"
	| "resposta_invalida"
	| "sem_lista"
	| "destino_recusado";

export class RespostaInvalida extends Error {
	constructor() {
		super("resposta do fornecedor fora do formato esperado");
	}
}

/** Classifica a resposta HTTP sem nunca olhar o corpo (ele pode repetir a chave). */
function erroDoStatus(f: Fornecedor, status: number): ErroDeAcesso {
	if (status === 401) return "chave_invalida";
	if (status === 400 && f.chaveInvalidaEm400) return "chave_invalida";
	if ((status === 404 || status === 405) && f.listaOpcional) return "sem_lista";
	if (status === 403) return "sem_permissao";
	if (status === 429) return "limite";
	return "fornecedor_falhou";
}

export type Acesso = { ok: true; modelos: ModeloListado[] } | { ok: false; erro: ErroDeAcesso; status?: number };

const MAX_PAGINAS = 10;
const MAX_MODELOS = 2000;
/** Teto de uma página da lista de modelos; acima disso a resposta é recusada sem ser lida inteira. */
const MAX_BYTES_PAGINA = 8 * 1024 * 1024;
const MAX_MARCADOR = 1024;

/** Lê o corpo como JSON sem passar de `limite` bytes. */
async function jsonLimitado(resposta: Response, limite: number): Promise<unknown> {
	const leitor = resposta.body?.getReader();
	if (!leitor) throw new RespostaInvalida();
	const partes: Uint8Array[] = [];
	let total = 0;
	for (;;) {
		const { done, value } = await leitor.read();
		if (done) break;
		total += value.byteLength;
		if (total > limite) {
			await leitor.cancel().catch(() => {});
			throw new RespostaInvalida();
		}
		partes.push(value);
	}
	try {
		return JSON.parse(Buffer.concat(partes).toString("utf8"));
	} catch {
		throw new RespostaInvalida();
	}
}

/** «Testar acesso»: lista os modelos que a chave enxerga. Só GET de listagem; nenhuma conversa, nenhuma cobrança. */
export async function testarAcesso(
	f: Fornecedor,
	chave: string,
	fetch: typeof globalThis.fetch,
	signal: AbortSignal,
	idValido: (id: string) => boolean,
): Promise<Acesso> {
	const modelos: ModeloListado[] = [];
	let pagina: string | undefined;
	try {
		for (let vez = 0; vez < MAX_PAGINAS; vez++) {
			const pedido = f.lista(chave, pagina);
			const resposta = await fetch(pedido.url, { headers: { accept: "application/json", ...pedido.headers }, signal, redirect: "error" });
			if (!resposta.ok) {
				await resposta.body?.cancel().catch(() => {});
				return { ok: false, erro: erroDoStatus(f, resposta.status), status: resposta.status };
			}
			const lido = f.ler(await jsonLimitado(resposta, MAX_BYTES_PAGINA));
			for (const modelo of lido.modelos) {
				if (modelos.length >= MAX_MODELOS) break;
				// Modelo cujo id ecoa a chave é excluído em vez de virar um id falso com «[removido]».
				if ((!chave || !modelo.id.includes(chave)) && idValido(modelo.id) && !modelos.some((m) => m.id === modelo.id)) {
					modelos.push(modelo);
				}
			}
			if (lido.proxima && lido.proxima.length > MAX_MARCADOR) throw new RespostaInvalida();
			pagina = lido.proxima;
			if (!pagina) break;
		}
	} catch (erro) {
		const destino = erro instanceof DestinoRecusado || (erro instanceof Error && erro.cause instanceof DestinoRecusado);
		return { ok: false, erro: erro instanceof RespostaInvalida ? "resposta_invalida" : destino ? "destino_recusado" : "sem_resposta" };
	}
	// A chave sai do nome ANTES do corte em 120 caracteres: se ela cruzasse o limite, o prefixo não sobrevive.
	const semChave = (valor: string) => (chave && valor.includes(chave) ? valor.split(chave).join("[removido]") : valor);
	const nomeLimpo = (valor: string) => [...semChave(valor)].slice(0, 120).join("");
	return {
		ok: true,
		modelos: modelos
			.filter((m) => (!chave || !m.id.includes(chave)) && idValido(m.id))
			.map((m) => ({ ...m, name: nomeLimpo(m.name) })),
	};
}

// Endereço próprio: um serviço qualquer num dos três formatos, com endereço e chave da pessoa. O endereço passa pelo
// portão de `destino.ts` na hora de salvar (no Hub) e em cada conexão (aqui).

export type Formato = "chat-completions" | "responses" | "anthropic-messages";

export interface Endereco {
	baseUrl: string;
	formato: Formato;
	local: boolean;
}

const API_DO_FORMATO: Record<Formato, Api> = {
	"chat-completions": "openai-completions",
	responses: "openai-responses",
	"anthropic-messages": "anthropic-messages",
};

export const ehProprio = (id: string) => /^proprio-[a-z0-9]{8,32}$/.test(id);

/** O endereço que veio do cofre, conferido de novo aqui (defesa em profundidade). */
export function enderecoDoCofre(valor: unknown): Endereco {
	const e = valor as Partial<Endereco> | null;
	if (!e || typeof e.baseUrl !== "string" || typeof e.local !== "boolean" || !e.formato || !Object.hasOwn(API_DO_FORMATO, e.formato)) {
		throw new DestinoRecusado("configuração do endereço próprio inválida");
	}
	conferirUrl(e.baseUrl, e.local);
	return { baseUrl: e.baseUrl.replace(/\/+$/, ""), formato: e.formato, local: e.local };
}

/** O mesmo formato de id do OpenRouter: serviços próprios costumam usar `dono/modelo` e `:`. */
export const modeloProprioValido = (id: string) => modeloValido("openrouter", id);

export function fornecedorProprio(e: Endereco): Fornecedor {
	const anthropic = e.formato === "anthropic-messages";
	return {
		nome: "Endereço próprio",
		api: API_DO_FORMATO[e.formato],
		baseUrl: e.baseUrl,
		contextWindow: 128000,
		maxTokens: 8192,
		listaOpcional: true,
		lista: (chave, pagina) =>
			anthropic
				? {
						url: `${e.baseUrl}/v1/models?limit=1000${pagina ? `&after_id=${encodeURIComponent(pagina)}` : ""}`,
						headers: { "x-api-key": chave, "anthropic-version": "2023-06-01" },
					}
				: { url: `${e.baseUrl}/models`, headers: bearer(chave) },
		ler: (corpo) => {
			const temMais = anthropic && (corpo as Objeto)["has_more"] === true;
			const proxima = temMais ? texto((corpo as Objeto)["last_id"]) : undefined;
			if (temMais && !proxima) throw new RespostaInvalida();
			return {
				modelos: lista(corpo, "data").flatMap((item) =>
					comId(item, anthropic ? { contextWindow: numero(item["max_input_tokens"]), maxTokens: numero(item["max_tokens"]) } : {}),
				),
				proxima,
			};
		},
	};
}

export function modeloProprio(provider: string, e: Endereco, id: string, listado?: ModeloListado): Model<Api> | undefined {
	if (!modeloProprioValido(id)) return undefined;
	const f = fornecedorProprio(e);
	return {
		id,
		name: id,
		api: f.api,
		provider,
		baseUrl: e.baseUrl,
		compat: SEM_THREADS,
		input: ["text"],
		reasoning: false,
		cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
		contextWindow: listado?.contextWindow ?? f.contextWindow,
		maxTokens: listado?.maxTokens ?? f.maxTokens,
	};
}
