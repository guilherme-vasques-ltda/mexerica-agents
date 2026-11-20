// Histórico da conversa vindo do Hub (restore_history). O processo do motor
// é descartável (cancelar o turno, trocar de provedor ou uma queda o matam, e
// trocar de modelo no app fecha o adaptador); quem guarda a conversa é o Hub,
// e o processo novo a recebe de volta antes do próximo prompt. O Hub manda uma
// cópia enxuta (só texto e chamadas de ferramenta, sem credenciais); aqui cada
// mensagem é conferida por papel e estrutura e PROJETADA nos campos permitidos
// (o que sobra do item de entrada nunca chega ao estado), cada resultado de
// ferramenta precisa ter a chamada dele na mesma volta (um para um: o id é consumido e o
// nome da ferramenta tem de ser o mesmo), e qualquer item inválido
// recusa a lista inteira antes de o estado mudar. O pi-ai normaliza ao trocar de
// provedor (pensamento de outro modelo cai, chamada de ferramenta sem resultado
// ganha um resultado de erro).
import type { AgentMessage } from "@earendil-works/pi-agent-core";
import { arrayBytes, MAX_HISTORY_BYTES, MAX_HISTORY_MESSAGES, MAX_MESSAGE_BYTES } from "./limits.ts";

export { MAX_HISTORY_BYTES, MAX_HISTORY_MESSAGES };

type Json = Record<string, unknown>;
const isObject = (value: unknown): value is Json => typeof value === "object" && value !== null && !Array.isArray(value);
const isText = (value: unknown): value is { type: "text"; text: string } => isObject(value) && value.type === "text" && typeof value.text === "string";
const textBlocks = (value: unknown): value is { type: "text"; text: string }[] => Array.isArray(value) && value.every(isText);
const keep = ({ text }: { text: string }) => ({ type: "text", text });
const timestamp = (message: Json): number => (typeof message.timestamp === "number" && Number.isFinite(message.timestamp) ? message.timestamp : 0);

const USAGE_KEYS = ["input", "output", "cacheRead", "cacheWrite"] as const;
const count = (value: unknown): number => (typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0);
function usageOf(value: unknown): Json {
	const usage = isObject(value) ? value : {};
	const cost = isObject(usage.cost) ? usage.cost : {};
	return {
		...Object.fromEntries(USAGE_KEYS.map((key) => [key, count(usage[key])])),
		totalTokens: count(usage.totalTokens),
		cost: { ...Object.fromEntries(USAGE_KEYS.map((key) => [key, count(cost[key])])), total: count(cost.total) },
	};
}

/** A mensagem só com os campos permitidos, ou erro. `calls`: chamadas abertas na volta, por id (cada resultado consome uma). */
function project(message: unknown, where: string, calls: Map<string, string>): Json {
	if (!isObject(message)) throw new Error(where + ": item inválido");
	switch (message.role) {
		case "user":
			if (calls.size > 0) throw new Error(where + ": chamada sem resultado antes da próxima mensagem");
			calls.clear();
			if (typeof message.content === "string") return { role: "user", content: message.content, timestamp: timestamp(message) };
			if (!textBlocks(message.content)) throw new Error(where + ": conteúdo inválido");
			return { role: "user", content: message.content.map(keep), timestamp: timestamp(message) };
		case "assistant": {
			if (calls.size > 0) throw new Error(where + ": chamada sem resultado antes da próxima mensagem");
			calls.clear();
			const blocks = message.content;
			if (!Array.isArray(blocks)) throw new Error(where + ": conteúdo inválido");
			const content = blocks.map((block) => {
				if (isText(block)) return keep(block);
				if (isObject(block) && block.type === "toolCall" && typeof block.id === "string" && typeof block.name === "string" && isObject(block.arguments)) {
					if (calls.has(block.id)) throw new Error(where + ": chamada repetida na mesma mensagem");
					calls.set(block.id, block.name);
					return { type: "toolCall", id: block.id, name: block.name, arguments: block.arguments };
				}
				throw new Error(where + ": conteúdo inválido");
			});
			for (const key of ["api", "provider", "model", "stopReason"]) {
				if (typeof message[key] !== "string") throw new Error(where + ": campo " + key + " inválido");
			}
			return {
				role: "assistant",
				content,
				api: message.api,
				provider: message.provider,
				model: message.model,
				stopReason: message.stopReason,
				usage: usageOf(message.usage),
				timestamp: timestamp(message),
			};
		}
		case "toolResult":
			if (typeof message.toolCallId !== "string" || typeof message.toolName !== "string" || !textBlocks(message.content)) {
				throw new Error(where + ": resultado de ferramenta inválido");
			}
			if (calls.get(message.toolCallId) !== message.toolName) throw new Error(where + ": resultado sem a chamada dele na mesma volta");
			calls.delete(message.toolCallId);
			return {
				role: "toolResult",
				toolCallId: message.toolCallId,
				toolName: message.toolName,
				content: message.content.map(keep),
				isError: message.isError === true,
				timestamp: timestamp(message),
			};
		default:
			throw new Error(where + ": papel inválido");
	}
}

/**
 * Valida a lista inteira; qualquer item inválido recusa o histórico todo (falha fechada). Cada item chega como o
 * texto JSON que o Hub mandou: o tamanho é o desses bytes, medido antes de qualquer conversão.
 */
export function parseHistory(raw: unknown): AgentMessage[] {
	if (!Array.isArray(raw)) throw new Error("restore_history exige messages como lista");
	if (raw.length > MAX_HISTORY_MESSAGES) throw new Error("restore_history: no máximo " + MAX_HISTORY_MESSAGES + " mensagens");
	let sum = 0;
	for (const [index, item] of raw.entries()) {
		if (typeof item !== "string") throw new Error("restore_history[" + index + "]: cada mensagem chega como texto JSON");
		const size = Buffer.byteLength(item);
		if (size > MAX_MESSAGE_BYTES) throw new Error("restore_history[" + index + "]: acima de " + MAX_MESSAGE_BYTES + " bytes por mensagem");
		sum += size;
	}
	if (arrayBytes(sum, raw.length) > MAX_HISTORY_BYTES) throw new Error("restore_history: acima de " + MAX_HISTORY_BYTES + " bytes");
	const calls = new Map<string, string>();
	const messages = (raw as string[]).map((text, index) => {
		let value: unknown;
		try {
			value = JSON.parse(text);
		} catch {
			throw new Error("restore_history[" + index + "]: não é JSON");
		}
		return project(value, "restore_history[" + index + "]", calls);
	});
	if (calls.size > 0) throw new Error("restore_history: chamada sem resultado no fim da lista");
	return messages as unknown as AgentMessage[];
}
