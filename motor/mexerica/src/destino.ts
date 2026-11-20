// Portão de rede do endereço próprio: um destino escolhido pela pessoa recebe a chave do cofre, então a conexão só sai
// para endereço público e por HTTPS. Rede privada, a própria máquina e endereço de metadados de nuvem são recusados;
// o endereço local (um modelo rodando na máquina ou na rede da pessoa) só vale com a opção «local» do endereço, que
// também aceita HTTP. Metadados de nuvem, link-local e multicast nunca. A conferência vale para cada endereço que o
// nome resolve e de novo para o endereço da conexão aberta, em IPv4 e IPv6. Redirecionamento é erro: chave, cabeçalho e
// histórico não seguem para outro lugar.
import * as dns from "node:dns";
import * as http from "node:http";
import * as https from "node:https";
import * as net from "node:net";
import { Readable } from "node:stream";

export class DestinoRecusado extends Error {
	constructor(motivo: string) {
		super(`destino recusado: ${motivo}`);
		this.name = "DestinoRecusado";
	}
}

const bloco = (faixas: [string, number, "ipv4" | "ipv6"][]) => {
	const lista = new net.BlockList();
	for (const [endereco, prefixo, tipo] of faixas) lista.addSubnet(endereco, prefixo, tipo);
	return lista;
};

/** Nunca, nem com a opção local: «este computador» genérico, link-local (onde moram os metadados de nuvem), multicast,
 * reservados e os endereços de metadados conhecidos que caem em faixas privadas. */
const NUNCA = bloco([
	["0.0.0.0", 8, "ipv4"],
	["169.254.0.0", 16, "ipv4"], // vazamento-ok: faixa da regra de recusa
	["224.0.0.0", 4, "ipv4"],
	["240.0.0.0", 4, "ipv4"],
	["100.100.100.200", 32, "ipv4"], // vazamento-ok: faixa da regra de recusa
	["::", 128, "ipv6"],
	["fe80::", 10, "ipv6"], // vazamento-ok: faixa da regra de recusa
	["ff00::", 8, "ipv6"],
	["fd00:ec2::254", 128, "ipv6"], // vazamento-ok: faixa da regra de recusa
]);

/** Só com a opção local: a própria máquina, redes privadas, CGNAT e afins. */
const LOCAL = bloco([
	["127.0.0.0", 8, "ipv4"],
	["10.0.0.0", 8, "ipv4"], // vazamento-ok: faixa da regra de recusa
	["172.16.0.0", 12, "ipv4"], // vazamento-ok: faixa da regra de recusa
	["192.168.0.0", 16, "ipv4"], // vazamento-ok: faixa da regra de recusa
	["100.64.0.0", 10, "ipv4"], // vazamento-ok: faixa da regra de recusa
	["192.0.0.0", 24, "ipv4"],
	["198.18.0.0", 15, "ipv4"],
	["::1", 128, "ipv6"],
	["fc00::", 7, "ipv6"], // vazamento-ok: faixa da regra de recusa
]);

/** IPv4 escondido num IPv6 (mapeado ou NAT64): a regra vale para o IPv4 de dentro. */
function ipv4Embutido(ip: string): string | undefined {
	const minusculo = ip.toLowerCase();
	for (const prefixo of ["::ffff:", "64:ff9b::"]) {
		if (!minusculo.startsWith(prefixo)) continue;
		const resto = minusculo.slice(prefixo.length);
		if (net.isIPv4(resto)) return resto;
		const partes = resto.split(":");
		if (partes.length === 2 && partes.every((p) => /^[0-9a-f]{1,4}$/.test(p))) {
			const [a, b] = partes.map((p) => Number.parseInt(p, 16)) as [number, number];
			return [a >> 8, a & 255, b >> 8, b & 255].join(".");
		}
	}
	return undefined;
}

export function ipPermitido(ip: string, local: boolean): boolean {
	const v4 = net.isIPv4(ip) ? ip : ipv4Embutido(ip);
	const [endereco, tipo] = v4 ? [v4, "ipv4" as const] : [ip, "ipv6" as const];
	if (!net.isIP(endereco)) return false;
	if (NUNCA.check(endereco, tipo)) return false;
	if (LOCAL.check(endereco, tipo)) return local;
	return true;
}

/** A URL base de um endereço próprio: HTTPS (HTTP só com a opção local), sem usuário ou senha, sem consulta nem
 * fragmento, com host. */
export function conferirUrl(texto: string, local: boolean): URL {
	let url: URL;
	try {
		url = new URL(texto);
	} catch {
		throw new DestinoRecusado("endereço inválido");
	}
	if (url.protocol !== "https:" && !(local && url.protocol === "http:")) throw new DestinoRecusado("só HTTPS");
	if (url.username || url.password) throw new DestinoRecusado("credencial dentro do endereço");
	if (url.search || url.hash) throw new DestinoRecusado("endereço com consulta ou fragmento");
	if (!url.hostname) throw new DestinoRecusado("endereço sem host");
	const host = url.hostname.replace(/^\[|\]$/g, "");
	if (net.isIP(host) && !ipPermitido(host, local)) throw new DestinoRecusado("endereço de rede não permitido");
	return url;
}

type Lookup = NonNullable<http.RequestOptions["lookup"]>;

/** Resolve o nome e recusa se QUALQUER endereço devolvido não for permitido (um nome que mistura público e privado
 * não escolhe o público por sorte). */
function lookupGuardado(local: boolean): Lookup {
	return ((hostname: string, opcoes: dns.LookupOptions, callback: (...args: unknown[]) => void) => {
		dns.lookup(hostname, { ...opcoes, all: true }, (erro, enderecos) => {
			if (erro) return callback(erro);
			const lista = enderecos as dns.LookupAddress[];
			if (lista.length === 0 || lista.some((e) => !ipPermitido(e.address, local))) {
				return callback(new DestinoRecusado("o nome resolve para um endereço de rede não permitido"));
			}
			if (opcoes.all) return callback(null, lista);
			callback(null, lista[0]!.address, lista[0]!.family);
		});
	}) as Lookup;
}

const SEM_CORPO = new Set([101, 204, 205, 304]);

/** `fetch` para um endereço próprio, por `node:http(s)` com o portão acima. Pede resposta sem compressão. */
export function fetchGuardado(local: boolean): typeof globalThis.fetch {
	return async (input, init) => {
		const pedido = new Request(input, init);
		const url = conferirUrl(pedido.url.replace(/[?#].*$/, ""), local);
		const completa = new URL(pedido.url);
		if (completa.username || completa.password) throw new DestinoRecusado("credencial dentro do endereço");
		const corpo = pedido.body ? Buffer.from(await pedido.arrayBuffer()) : undefined;
		const cabecalhos: Record<string, string> = {};
		pedido.headers.forEach((valor, nome) => (cabecalhos[nome] = valor));
		cabecalhos["accept-encoding"] = "identity";
		const modulo = url.protocol === "https:" ? https : http;
		return new Promise<Response>((resolve, reject) => {
			const req = modulo.request(
				completa,
				{ method: pedido.method, headers: cabecalhos, lookup: lookupGuardado(local), signal: pedido.signal, agent: false },
				(res) => {
					const status = res.statusCode ?? 0;
					if (status >= 300 && status < 400) {
						res.destroy();
						return reject(new TypeError("redirecionamento recusado"));
					}
					const headers = new Headers();
					for (const [nome, valor] of Object.entries(res.headers)) {
						if (valor === undefined) continue;
						for (const v of Array.isArray(valor) ? valor : [valor]) headers.append(nome, v);
					}
					const body = SEM_CORPO.has(status) ? null : (Readable.toWeb(res) as ReadableStream<Uint8Array>);
					resolve(new Response(body, { status, statusText: res.statusMessage ?? "", headers }));
				},
			);
			// A conexão aberta é conferida de novo: o endereço de fato, não só o que o nome prometeu.
			req.on("socket", (socket) => {
				const conferir = () => {
					const remoto = socket.remoteAddress;
					if (!remoto || !ipPermitido(remoto, local)) req.destroy(new DestinoRecusado("conexão com endereço de rede não permitido"));
				};
				socket.once(url.protocol === "https:" ? "secureConnect" : "connect", conferir);
			});
			req.on("error", reject);
			req.end(corpo);
		});
	};
}
