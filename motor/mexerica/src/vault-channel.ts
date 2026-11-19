// Canal do cofre: JSON-RPC sobre o descritor 3 (socket par criado pelo Hub).
// A chave só existe em memória neste processo — nunca em argv, ambiente,
// arquivo, log ou URL. Sem fd 3 (ou sem resposta do Hub) o motor falha fechado:
// pendências são rejeitadas no fechamento/erro e cada pedido tem prazo.
import * as net from "node:net";
import { attachJsonlLineReader, serializeJsonLine } from "../../vendor/pi-mono/packages/coding-agent/src/modes/rpc/jsonl.ts";

/** Prazo de cada pedido ao cofre; sem resposta, o turno termina com erro. */
export const VAULT_TIMEOUT_MS = 30_000;

interface VaultRequest {
	id: number;
	method: string;
	params?: unknown;
}

interface VaultResponse {
	id: number;
	result?: unknown;
	error?: { message?: string };
}

interface Pending {
	resolve: (v: unknown) => void;
	reject: (e: Error) => void;
	timer: NodeJS.Timeout;
}

export class VaultChannel {
	private socket: net.Socket;
	private nextId = 1;
	private pending = new Map<number, Pending>();
	private closed = false;

	private constructor(socket: net.Socket) {
		this.socket = socket;
		attachJsonlLineReader(socket, (line) => {
			let msg: VaultResponse;
			try {
				msg = JSON.parse(line);
			} catch {
				return; // lixo no canal não derruba o motor
			}
			const waiter = this.pending.get(msg.id);
			if (!waiter) return;
			this.pending.delete(msg.id);
			clearTimeout(waiter.timer);
			if (msg.error) waiter.reject(new Error(msg.error.message ?? "credential channel error"));
			else waiter.resolve(msg.result);
		});
		const fechar = (cause?: Error) =>
			this.failAll(cause ?? new Error("canal do cofre encerrado pelo Hub"));
		socket.on("error", fechar);
		socket.on("close", () => fechar());
		socket.on("end", () => fechar());
	}

	/** Rejeita todas as pendências; pedidos novos falham na hora. */
	private failAll(cause: Error): void {
		if (this.closed) return;
		this.closed = true;
		for (const waiter of this.pending.values()) {
			clearTimeout(waiter.timer);
			waiter.reject(cause);
		}
		this.pending.clear();
	}

	/** Abre o canal no fd 3 ou falha fechado (o Hub sempre o cria). */
	static open(fd = 3): VaultChannel {
		try {
			return new VaultChannel(new net.Socket({ fd, readable: true, writable: true }));
		} catch (cause) {
			throw new Error(`canal do cofre indisponível no fd ${fd}: ${cause instanceof Error ? cause.message : cause}`);
		}
	}

	request<T>(method: string, params?: unknown, timeoutMs = VAULT_TIMEOUT_MS): Promise<T> {
		if (this.closed) return Promise.reject(new Error("canal do cofre encerrado pelo Hub"));
		const id = this.nextId++;
		return new Promise<T>((resolve, reject) => {
			const timer = setTimeout(() => {
				this.pending.delete(id);
				reject(new Error(`canal do cofre sem resposta em ${timeoutMs}ms (${method})`));
			}, timeoutMs);
			this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, timer });
			const out: VaultRequest = { id, method, params };
			this.socket.write(serializeJsonLine(out), (error) => {
				if (error) {
					this.pending.delete(id);
					clearTimeout(timer);
					reject(error);
				}
			});
		});
	}
}
