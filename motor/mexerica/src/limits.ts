// Tetos da conversa no processo do motor. O Hub guarda uma cópia com
// estes mesmos tetos; aqui eles valem para o processo VIVO: o contexto enviado ao
// provedor em cada chamada, o histórico que o processo mantém (durante o turno e
// entre turnos) e as mensagens do fim do turno.
//
// UMA unidade só em todos os pontos (Hub, receptor e motor): bytes UTF-8 do JSON
// NA FORMA EM QUE ELE CRUZA A FRONTEIRA. O Hub manda cada mensagem como um texto JSON
// e o receptor (history.ts) mede esses mesmos bytes, sem serializar de novo (o
// serializador do Node e o do Rust imprimem alguns números de modos diferentes). Aqui, no
// processo vivo, a medida é o JSON.stringify do próprio Node, a forma em que o motor
// entrega as mensagens. A conversa mede o ARRAY serializado (colchetes e vírgulas
// inclusos, mensagens de sistema de fora) e cada mensagem mede o JSON dela. Estourou: sai a volta mais
// antiga inteira; numa volta sozinha ficam o pedido e os grupos mais novos que
// couberem (a chamada de ferramenta e os resultados dela saem juntos, nunca um
// sem o outro).
import type { AgentMessage } from "@earendil-works/pi-agent-core";

export const MAX_MESSAGE_BYTES = 256 * 1024;
export const MAX_HISTORY_BYTES = 2 * 1024 * 1024;
export const MAX_HISTORY_MESSAGES = 2_000;
const ENVELOPE_BYTES = 8 * 1024;
const TRUNCATED = "\n[truncado pelo Motor]";

type Json = Record<string, unknown>;
const isObject = (value: unknown): value is Json => typeof value === "object" && value !== null && !Array.isArray(value);
const role = (message: AgentMessage): string => String((message as unknown as Json).role);

/** Bytes UTF-8 do JSON de qualquer valor: a unidade de todos os tetos. */
export const bytesOf = (value: unknown): number => Buffer.byteLength(JSON.stringify(value));

/** Bytes do array JSON de `count` mensagens que somam `sum` bytes: colchetes e vírgulas inclusos. */
export const arrayBytes = (sum: number, count: number): number => 2 + sum + Math.max(count - 1, 0);

// Uma mensagem pronta não muda: o tamanho dela é medido uma vez só.
const measured = new WeakMap<object, number>();
function messageBytes(message: AgentMessage): number {
	let size = measured.get(message);
	if (size === undefined) {
		size = bytesOf(message);
		measured.set(message, size);
	}
	return size;
}

function truncate(text: string, budget: number): string {
	if (Buffer.byteLength(text) <= budget) return text;
	return Buffer.from(text).subarray(0, Math.max(budget, 0)).toString("utf8").replace(/\uFFFD$/, "") + TRUNCATED;
}

/** A mensagem com texto e argumentos dentro de `budget` bytes brutos; pensamento, detalhes e imagens saem. */
function project(message: AgentMessage, budget: number): AgentMessage {
	const source = message as unknown as Json;
	const { details: _details, ...rest } = source;
	if (typeof source.content === "string") return { ...rest, content: truncate(source.content, budget) } as unknown as AgentMessage;
	const content: unknown[] = [];
	for (const block of Array.isArray(source.content) ? source.content : []) {
		if (!isObject(block)) continue;
		if (block.type === "text" && typeof block.text === "string") {
			if (budget <= 0) continue;
			const text = truncate(block.text, budget);
			budget -= Buffer.byteLength(text);
			content.push({ type: "text", text });
		} else if (block.type === "toolCall") {
			const size = bytesOf(block.arguments);
			content.push({ type: "toolCall", id: block.id, name: block.name, arguments: size <= budget ? block.arguments : {} });
			budget -= Math.min(size, budget);
		}
	}
	return { ...rest, content } as unknown as AgentMessage;
}

/**
 * Mensagem acima do teto por mensagem: fica só o texto (cortado) e as chamadas. O
 * corte é bruto e o JSON escapa aspas e controles, então o resultado é MEDIDO e o
 * orçamento cai até o JSON final caber no teto.
 */
function shrink(message: AgentMessage): AgentMessage {
	if (messageBytes(message) <= MAX_MESSAGE_BYTES) return message;
	let budget = MAX_MESSAGE_BYTES - ENVELOPE_BYTES;
	for (;;) {
		const candidate = project(message, budget);
		const size = bytesOf(candidate);
		if (size <= MAX_MESSAGE_BYTES) {
			measured.set(candidate, size);
			return candidate;
		}
		if (budget === 0) break;
		budget = Math.max(budget - (size - MAX_MESSAGE_BYTES) - 1, 0);
	}
	const source = message as unknown as Json;
	const bare = { role: source.role, content: typeof source.content === "string" ? "" : [], timestamp: source.timestamp } as unknown as AgentMessage;
	measured.set(bare, bytesOf(bare));
	return bare;
}

/**
 * Limita UMA mensagem no próprio objeto, no instante em que ela termina (`message_end`). O laço do
 * agente guarda esse mesmo objeto na lista do turno e na do contexto, então os dois acumuladores já
 * recebem a mensagem dentro do teto e nada cresce até um corte posterior.
 */
export function boundInPlace(message: AgentMessage): void {
	// Uma mensagem com chamadas de ferramenta é a que o laço vai EXECUTAR agora: os argumentos dela não mudam antes disso
	// (o corte dela vem em seguida, no fim da volta, por `fitMessages`).
	if (role(message) === "assistant" && callIds(message).length > 0) return;
	// A mensagem de sistema leva as declarações das ferramentas (toolsAdded) que o provedor usa: nunca é cortada aqui.
	if (role(message) === "system") return;
	measured.delete(message);
	const bounded = shrink(message);
	if (bounded === message) return;
	const target = message as unknown as Json;
	for (const key of Object.keys(target)) delete target[key];
	Object.assign(target, bounded);
	measured.delete(message);
}

const callIds = (message: AgentMessage): string[] => {
	const content = (message as unknown as Json).content;
	return Array.isArray(content) ? (content as Json[]).filter((block) => block.type === "toolCall").map((block) => String(block.id)) : [];
};

/** Há uma volta de ferramentas em andamento: a última mensagem do assistente tem chamada sem o resultado ainda. */
export function batchOpen(messages: AgentMessage[]): boolean {
	let answered = 0;
	for (let index = messages.length - 1; index >= 0; index--) {
		const current = messages[index]!;
		if (role(current) === "toolResult") answered++;
		else if (role(current) === "assistant") return callIds(current).length > answered;
		else if (role(current) !== "system") return false;
	}
	return false;
}

/** Onde acaba o grupo que começa em `start`: a mensagem do assistente e os resultados das chamadas dela. */
function groupEnd(messages: AgentMessage[], start: number): number {
	const first = messages[start] as unknown as Json;
	const ids = new Set<string>(
		role(messages[start]!) === "assistant" && Array.isArray(first.content)
			? (first.content as Json[]).filter((block) => block.type === "toolCall").map((block) => String(block.id))
			: [],
	);
	let end = start + 1;
	while (end < messages.length && ids.size > 0 && role(messages[end]!) === "toolResult" && ids.has(String((messages[end] as unknown as Json).toolCallId))) end++;
	return end;
}

/** A conversa dentro dos tetos. As mensagens de sistema ficam de fora da conta e do corte. */
export function fitMessages(messages: AgentMessage[]): AgentMessage[] {
	const system = messages.filter((message) => role(message) === "system");
	const rest = messages.filter((message) => role(message) !== "system").map(shrink);
	let sum = rest.reduce((total, message) => total + messageBytes(message), 0);
	while (arrayBytes(sum, rest.length) > MAX_HISTORY_BYTES || rest.length > MAX_HISTORY_MESSAGES) {
		const nextTurn = rest.findIndex((message, index) => index > 0 && role(message) === "user");
		let range: [number, number];
		if (nextTurn > 0) range = [0, nextTurn];
		else {
			const start = rest.length > 0 && role(rest[0]!) === "user" ? 1 : 0;
			if (start >= rest.length || rest.length <= 1) break;
			range = [start, groupEnd(rest, start)];
		}
		for (const removed of rest.splice(range[0], range[1] - range[0])) sum -= messageBytes(removed);
	}
	return [...system, ...rest];
}
