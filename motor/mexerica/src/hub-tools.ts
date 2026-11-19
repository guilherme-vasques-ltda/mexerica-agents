// Ferramentas do Hub no motor (fatia 2). O motor NÃO executa nada: cada
// ferramenta é só uma declaração que o Hub enviou por `set_tools`. Quando o
// modelo a chama, o motor emite `tool_request` e espera o `tool_result` do
// Hub — que executa pelo caminho de ferramentas/MCP do Hub, com as permissões
// e aprovações dele. Este módulo não abre processo, socket nem URL.
import type { AgentTool } from "@earendil-works/pi-agent-core";

export interface HubToolOrigin {
	kind: "hub" | "connector";
	accountLabel?: string;
}

export interface HubToolSpec {
	name: string;
	description: string;
	inputSchema: object;
	origin: HubToolOrigin;
}

/** Limites defensivos: a lista vem do Hub, mas o modelo vê tudo o que ela diz. */
export const MAX_TOOLS = 256;
export const MAX_DESCRIPTION = 8_192;
export const MAX_OUTPUT = 256 * 1024;
/** Prazo de uma ferramenta; a aprovação humana do Hub conta dentro dele. */
export const TOOL_TIMEOUT_MS = 10 * 60_000;

const NAME = /^[A-Za-z0-9_-]{1,64}$/;

/** Valida a lista inteira; qualquer item inválido recusa a lista toda (falha fechada). */
export function parseToolSpecs(raw: unknown): HubToolSpec[] {
	if (!Array.isArray(raw)) throw new Error("set_tools exige `tools` como lista");
	if (raw.length > MAX_TOOLS) throw new Error(`set_tools: no máximo ${MAX_TOOLS} ferramentas`);
	const seen = new Set<string>();
	return raw.map((item, index) => {
		const tool = item as Partial<HubToolSpec> | null;
		if (typeof tool !== "object" || tool === null) throw new Error(`set_tools[${index}]: item inválido`);
		const { name, description, inputSchema, origin } = tool;
		if (typeof name !== "string" || !NAME.test(name)) throw new Error(`set_tools[${index}]: nome inválido`);
		if (seen.has(name)) throw new Error(`set_tools: nome repetido: ${name}`);
		seen.add(name);
		if (typeof description !== "string" || description.length > MAX_DESCRIPTION) {
			throw new Error(`set_tools[${name}]: descrição inválida`);
		}
		if (typeof inputSchema !== "object" || inputSchema === null || Array.isArray(inputSchema)) {
			throw new Error(`set_tools[${name}]: inputSchema deve ser um objeto JSON Schema`);
		}
		if (origin?.kind !== "hub" && origin?.kind !== "connector") {
			throw new Error(`set_tools[${name}]: origem inválida`);
		}
		const accountLabel = typeof origin.accountLabel === "string" ? origin.accountLabel : undefined;
		return {
			name,
			description,
			inputSchema,
			origin: { kind: origin.kind, ...(accountLabel === undefined ? {} : { accountLabel }) },
		};
	});
}

export interface ToolResult {
	callId: string;
	ok: boolean;
	output?: unknown;
	error?: unknown;
}

interface Pending {
	resolve: (value: ToolResult) => void;
	reject: (reason: Error) => void;
}

const asText = (value: unknown): string => {
	const text = typeof value === "string" ? value : value === undefined ? "" : JSON.stringify(value);
	return text.length > MAX_OUTPUT ? `${text.slice(0, MAX_OUTPUT)}\n[saída truncada pelo motor]` : text;
};

/** Ponte entre as chamadas do modelo e o Hub: `emit` escreve o evento no stdout. */
export class HubTools {
	private readonly emit: (event: object) => void;
	private readonly pending = new Map<string, Pending>();
	private counter = 0;

	constructor(emit: (event: object) => void) {
		this.emit = emit;
	}

	/** Declara cada ferramenta ao modelo; `execute` só pede ao Hub e espera. */
	toAgentTools(specs: HubToolSpec[]): AgentTool<any>[] {
		return specs.map((spec) => ({
			name: spec.name,
			label: spec.name,
			description:
				spec.origin.accountLabel === undefined
					? spec.description
					: `${spec.description}\n(conta: ${spec.origin.accountLabel})`,
			parameters: spec.inputSchema,
			executionMode: "sequential",
			execute: async (_toolCallId, params, signal) => {
				const result = await this.request(spec.name, params, signal);
				if (!result.ok) throw new Error(asText(result.error ?? "o Hub recusou a ferramenta"));
				return { content: [{ type: "text", text: asText(result.output) }], details: undefined };
			},
		}));
	}

	private request(name: string, input: unknown, signal?: AbortSignal): Promise<ToolResult> {
		const callId = `call-${++this.counter}-${crypto.randomUUID()}`;
		return new Promise<ToolResult>((resolve, reject) => {
			const finish = (action: () => void) => {
				clearTimeout(timer);
				signal?.removeEventListener("abort", onAbort);
				this.pending.delete(callId);
				action();
			};
			const onAbort = () => finish(() => reject(new Error("turno cancelado")));
			const timer = setTimeout(
				() => finish(() => reject(new Error("o Hub não respondeu à ferramenta no prazo"))),
				TOOL_TIMEOUT_MS,
			);
			signal?.addEventListener("abort", onAbort, { once: true });
			this.pending.set(callId, {
				resolve: (value) => finish(() => resolve(value)),
				reject: (reason) => finish(() => reject(reason)),
			});
			if (signal?.aborted) return onAbort();
			this.emit({ type: "tool_request", callId, name, input });
		});
	}

	/** `tool_result` do Hub; callId desconhecido (ou repetido) é recusado. */
	resolve(result: ToolResult): void {
		const waiter = typeof result.callId === "string" ? this.pending.get(result.callId) : undefined;
		if (!waiter) throw new Error("tool_result sem pedido correspondente");
		waiter.resolve(result);
	}

	/** Fim do turno ou do processo: ninguém fica esperando o Hub. */
	failAll(reason: string): void {
		for (const waiter of [...this.pending.values()]) waiter.reject(new Error(reason));
	}
}
