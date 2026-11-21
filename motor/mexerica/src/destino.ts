// Portão de rede do endereço próprio: um destino escolhido pela pessoa recebe a chave do cofre, então a conexão só sai
// para endereço público e por HTTPS. Rede privada, a própria máquina e endereço de metadados de nuvem são recusados;
// o endereço local (um modelo rodando na máquina ou na rede da pessoa) só vale com a opção «local» do endereço, que
// também aceita HTTP — mas só para rede local: IP público nunca em HTTP, nem com a opção. Metadados de nuvem, link-local e multicast nunca. A conferência vale para cada endereço que o
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

/** Endereços IPv6 de tradução e escopo especial que nunca valem como destino: 6to4 e site-local obsoleto. */
const NUNCA_V6_EXTRA = bloco([
	["2002::", 16, "ipv6"], // vazamento-ok: faixa da regra de recusa
	["2001::", 32, "ipv6"], // vazamento-ok: faixa da regra de recusa
	["fec0::", 10, "ipv6"], // vazamento-ok: faixa da regra de recusa
	["100::", 64, "ipv6"], // vazamento-ok: faixa da regra de recusa
	["64:ff9b:1::", 48, "ipv6"], // vazamento-ok: faixa da regra de recusa
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

/** Os 16 bytes de um endereço IPv6; `undefined` se não for IPv6. O IPv4 escrito no fim (`::ffff:1.2.3.4`)
 * ocupa os dois últimos grupos, na forma canônica: daqui sai a regra certa para as formas comprimidas. */
function ipv6Bytes(ip: string): Uint8Array | undefined {
	if (!net.isIPv6(ip)) return undefined;
	const lados = ip.toLowerCase().split("::");
	if (lados.length > 2) return undefined;
	const grupos: number[][] = [[], []];
	for (let i = 0; i < lados.length; i++) {
		if (lados[i] === "") continue;
		const partes = lados[i]!.split(":");
		const ultima = partes.at(-1)!;
		const octetos = net.isIPv4(ultima) ? ultima.split(".").map(Number) : undefined;
		if (octetos) {
			// O IPv4 escrito no fim vale no último lado, com ou sem "::" no meio do endereço.
			if (i !== lados.length - 1) return undefined;
			if (octetos.length !== 4 || octetos.some((o) => !Number.isInteger(o) || o < 0 || o > 255)) return undefined;
			partes.pop();
		}
		for (const parte of partes) {
			if (!/^[0-9a-f]{1,4}$/.test(parte)) return undefined;
			grupos[i]!.push(Number.parseInt(parte, 16));
		}
		if (octetos) grupos[i]!.push((octetos[0]! << 8) | octetos[1]!, (octetos[2]! << 8) | octetos[3]!);
	}
	const total = grupos[0]!.length + grupos[1]!.length;
	if (total > 8 || (lados.length === 1 && total !== 8)) return undefined;
	const faltam = 8 - total;
	const bytes = new Uint8Array(16);
	[...grupos[0]!, ...new Array<number>(faltam).fill(0), ...grupos[1]!].forEach((grupo, i) => {
		bytes[i * 2] = grupo >> 8;
		bytes[i * 2 + 1] = grupo & 255;
	});
	return bytes;
}

/** IPv4 escondido num IPv6, lido dos 16 bytes canônicos (mapeado, traduzido, compatível ou NAT64, nas formas
 * comprimidas ou não): a regra vale para o IPv4 de dentro. `::1` é loopback de verdade, não IPv4-compatível. */
function ipv4DeBytes(bytes: Uint8Array): string | undefined {
	const zero = (de: number, ate: number) => bytes.subarray(de, ate).every((b) => b === 0);
	const embutido = () => [...bytes.subarray(12)].join(".");
	// NAT64: 64:ff9b::/96 e o de uso local 64:ff9b:1::/48.
	if (bytes[0] === 0 && bytes[1] === 0x64 && bytes[2] === 0xff && bytes[3] === 0x9b) {
		if (zero(4, 12) || (bytes[4] === 0 && bytes[5] === 1 && zero(6, 12))) return embutido();
	}
	// Mapeado e traduzido (::ffff:0:0/96 nas duas grafias): ffff num par do meio, zeros no resto.
	if (zero(0, 8) && ((bytes[8] === 0xff && bytes[9] === 0xff && zero(10, 12)) || (bytes[10] === 0xff && bytes[11] === 0xff && zero(8, 10))))
		return embutido();
	// IPv4-compatível obsoleto (::/96), fora do ::1 de loopback.
	if (zero(0, 12) && !(bytes[12] === 0 && bytes[13] === 0 && bytes[14] === 0 && bytes[15] === 1)) return embutido();
	return undefined;
}

/** O endereço pode receber a chave? `local` libera rede privada; `seguro` (HTTPS) é o único jeito de falar com
 * endereço público — a opção local não abre exceção para HTTP na Internet. */
export function ipPermitido(ip: string, local: boolean, seguro: boolean): boolean {
	if (!net.isIP(ip)) return false;
	const bytes = ipv6Bytes(ip);
	// IPv6 que não se deixa ler em bytes canônicos não passa: o portão falha fechado.
	if (net.isIPv6(ip) && !bytes) return false;
	const embutido = bytes && ipv4DeBytes(bytes);
	const [endereco, tipo] = embutido
		? [embutido, "ipv4" as const]
		: net.isIPv4(ip) ? [ip, "ipv4" as const] : [ip, "ipv6" as const];
	if (NUNCA.check(endereco, tipo) || NUNCA_V6_EXTRA.check(endereco, tipo)) return false;
	if (LOCAL.check(endereco, tipo)) return local;
	return seguro;
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
	if (net.isIP(host) && !ipPermitido(host, local, url.protocol === "https:")) throw new DestinoRecusado("endereço de rede não permitido");
	return url;
}

type Lookup = NonNullable<http.RequestOptions["lookup"]>;

/** Resolve o nome e recusa se QUALQUER endereço devolvido não for permitido (um nome que mistura público e privado
 * não escolhe o público por sorte). */
function lookupGuardado(local: boolean, seguro: boolean): Lookup {
	return ((hostname: string, opcoes: dns.LookupOptions, callback: (...args: unknown[]) => void) => {
		dns.lookup(hostname, { ...opcoes, all: true }, (erro, enderecos) => {
			if (erro) return callback(erro);
			const lista = enderecos as dns.LookupAddress[];
			if (lista.length === 0 || lista.some((e) => !ipPermitido(e.address, local, seguro))) {
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
		const seguro = modulo === https;
		return new Promise<Response>((resolve, reject) => {
			const req = modulo.request(
				completa,
				{ method: pedido.method, headers: cabecalhos, lookup: lookupGuardado(local, seguro), signal: pedido.signal, agent: false },
				(res) => {
					try {
						const status = res.statusCode ?? 0;
						if (status >= 300 && status < 400) {
							res.destroy();
							return reject(new TypeError("redirecionamento recusado"));
						}
						// Fora de 200..599 o Response nem existe: recusar limpo, sem derrubar o processo.
						if (status < 200 || status > 599) {
							res.destroy();
							return reject(new TypeError(`status de resposta fora do esperado: ${status}`));
						}
						const headers = new Headers();
						for (const [nome, valor] of Object.entries(res.headers)) {
							if (valor === undefined) continue;
							for (const v of Array.isArray(valor) ? valor : [valor]) headers.append(nome, v);
						}
						const body = SEM_CORPO.has(status) ? null : (Readable.toWeb(res) as ReadableStream<Uint8Array>);
						resolve(new Response(body, { status, statusText: res.statusMessage ?? "", headers }));
					} catch (erro) {
						res.destroy();
						reject(erro instanceof Error ? erro : new TypeError("resposta fora do formato esperado"));
					}
				},
			);
			// A conexão aberta é conferida de novo: o endereço de fato, não só o que o nome prometeu.
			req.on("socket", (socket) => {
				const conferir = () => {
					const remoto = socket.remoteAddress;
					if (!remoto || !ipPermitido(remoto, local, seguro)) req.destroy(new DestinoRecusado("conexão com endereço de rede não permitido"));
				};
				socket.once(url.protocol === "https:" ? "secureConnect" : "connect", conferir);
			});
			req.on("error", reject);
			req.end(corpo);
		});
	};
}
