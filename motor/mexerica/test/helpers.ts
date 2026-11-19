// Ajudantes dos testes do motor: sobe o processo real, serve o cofre falso pelo
// fd 3 e entrega as linhas JSONL que o motor escreve.
import { once } from "node:events";
import { spawn, type ChildProcess } from "node:child_process";
import type { Readable, Writable } from "node:stream";
import { fileURLToPath } from "node:url";
import { attachJsonlLineReader, serializeJsonLine } from "../../vendor/pi-mono/packages/coding-agent/src/modes/rpc/jsonl.ts";

export const ENTRY = fileURLToPath(new URL("../rpc.ts", import.meta.url));
export const FETCH_STUB = fileURLToPath(new URL("./stub-fetch.mjs", import.meta.url));

export interface JsonRpc {
	id: number;
	method?: string;
	params?: { providerId?: string };
}

export type Line = Record<string, unknown> & { type?: string };

export interface EngineOpts {
	/** Credencial servida pelo "cofre" falso; undefined responde null. */
	serveKey?: string;
	/** Fecha o canal do cofre ao receber o primeiro pedido. */
	closeVaultOnRequest?: boolean;
	ignoreVaultRequest?: boolean;
	/** Arquivo JSONL onde o stub de fetch registra as chamadas. */
	fetchLog?: string;
	/** Pasta de trabalho do processo (a prova do MCP inerte a enche de configs). */
	cwd?: string;
}

export interface Proc {
	child: ChildProcess;
	lines: Line[];
	stderr: () => string;
	send: (cmd: object) => void;
	waitFor: (pred: (v: Line) => boolean, timeoutMs?: number) => Promise<Line>;
}

export function startEngine(env: NodeJS.ProcessEnv, opts: EngineOpts = {}): Proc {
	// O fd 3 do stdio do Node é um socket par bidirecional: o canal do cofre.
	const args = opts.fetchLog ? ["--import", FETCH_STUB, ENTRY] : [ENTRY];
	const child = spawn(process.execPath, args, {
		cwd: opts.cwd,
		env: { ...env, ...(opts.fetchLog ? { MEXERICA_MOTOR_FETCH_LOG: opts.fetchLog } : {}) },
		stdio: ["pipe", "pipe", "pipe", "pipe"],
	});
	const vault = child.stdio[3] as unknown as Readable & Writable;

	let erro = "";
	child.stderr?.on("data", (chunk) => (erro += String(chunk)));

	const lines: Line[] = [];
	const waiters: { pred: (v: Line) => boolean; resolve: (v: Line) => void }[] = [];
	attachJsonlLineReader(child.stdout as Readable, (line) => {
		const value = JSON.parse(line) as Line;
		lines.push(value);
		for (let i = waiters.length - 1; i >= 0; i--) {
			if (waiters[i]!.pred(value)) {
				waiters[i]!.resolve(value);
				waiters.splice(i, 1);
			}
		}
	});

	attachJsonlLineReader(vault, (line) => {
		const req = JSON.parse(line) as JsonRpc;
		if (opts.ignoreVaultRequest) return;
		if (opts.closeVaultOnRequest) {
			vault.destroy();
			return;
		}
		const key = opts.serveKey === undefined ? undefined : { type: "api_key", key: opts.serveKey };
		vault.write(serializeJsonLine({ id: req.id, result: key }));
	});

	const waitFor = (pred: (v: Line) => boolean, timeoutMs = 15000) =>
		new Promise<Line>((resolve, reject) => {
			const hit = lines.find(pred);
			if (hit) return resolve(hit);
			waiters.push({ pred, resolve });
			setTimeout(() => reject(new Error(`timeout esperando evento; linhas: ${JSON.stringify(lines)}`)), timeoutMs);
		});

	return {
		child,
		lines,
		stderr: () => erro,
		send: (cmd) => (child.stdin as Writable).write(serializeJsonLine(cmd)),
		waitFor,
	};
}

export async function encerrar(proc: Proc): Promise<void> {
	const closed = once(proc.child, "close");
	proc.child.kill();
	await closed;
}

export const turnoTerminou = (v: Line) =>
	v.type === "agent_end" ||
	v.type === "error" ||
	(v.type === "message_end" && (v.message as { stopReason?: string }).stopReason === "error");

