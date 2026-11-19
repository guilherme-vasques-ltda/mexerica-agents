// MEX-1307 F0 (docs/plans/instalacao-remota-vpn-vps.md §4.6): gramática estrita de cada mensagem de dados que a
// VPS pode mandar. A VPS não é confiável: tudo aqui é afirmação, nunca prova; a autoridade é refeita no root (F1).
// Módulo puro: sem I/O, sem SSH.

export const CODIGOS_EVENTO = [
  // Fatos da sondagem (a) e (b): `ok` = presente, `pulado` = ausente comprovado, `falha` = não consegui determinar.
  'linger', 'motor', 'hostinst', 'contahub', 'ehhub', 'portaocupada',
  // Requisitos da sondagem (a): `ok` = atende, `falha` = não atende, `pulado` = não consegui determinar (vale como falha).
  'so', 'arquitetura', 'systemd', 'espaco', 'sudo', 'sudotty', 'libcap', 'nosuid', 'hardlinks', 'relogio',
  // `sudo -n` (V6): `ok` = passou sem senha, `pulado` = exige senha, `falha` = indisponível.
  'sudosemsenha',
  // Etapas da instalação e da desinstalação (F1/F3).
  'usuario', 'grupo', 'ajudante', 'unidade', 'sudoers', 'chaveapp', 'iniciar', 'saude', 'remocao', 'transferencia',
] as const
export type CodigoEvento = (typeof CODIGOS_EVENTO)[number]

export const ESTADOS_EVENTO = ['ok', 'pulado', 'falha'] as const
export type EstadoEvento = (typeof ESTADOS_EVENTO)[number]

/** Fatos (presença) e requisitos (atende) têm leitura diferente de `pulado`; ver o cabeçalho de CODIGOS_EVENTO. */
export const CODIGOS_FATO: ReadonlySet<CodigoEvento> = new Set(['linger', 'motor', 'hostinst', 'contahub', 'ehhub', 'portaocupada'])

export const CLASSES_812 = ['limpo-comprovado', 'concluido-integro', 'incompleto', 'ilegivel'] as const
export type Classe812 = (typeof CLASSES_812)[number]

/** Sistemas aceitos pelo plano (Debian 13; Ubuntu 22.04 ou mais novo). `so` = `<id> <VERSION_ID>`. */
export const IDS_SO = ['debian', 'ubuntu'] as const
export type IdSo = (typeof IDS_SO)[number]
const VERSAO_SO = /^[0-9]{1,3}(\.[0-9]{1,3})?$/

export const ARQUITETURAS = ['x86_64', 'aarch64'] as const
export type Arquitetura = (typeof ARQUITETURAS)[number]

/** Teto de bytes por tipo de mensagem de dados (§4.6). */
export const TETO_DADO = {
  tam: 12, hashparcial: 64, uid: 21, arq: 16, so: 24, host: 128, estado812: 24, saude: 4096, total: 90,
} as const
export type TipoDado = keyof typeof TETO_DADO
export const TIPOS_DADO = Object.keys(TETO_DADO) as TipoDado[]

export type Dado =
  | { tipo: 'tam'; bytes: number }
  | { tipo: 'hashparcial'; sha256: string }
  | { tipo: 'uid'; uidAdmin: number; uidHub: number | null }
  | { tipo: 'arq'; arquitetura: Arquitetura }
  | { tipo: 'so'; id: IdSo; versao: string }
  | { tipo: 'host'; computerId: string; versao: string }
  | { tipo: 'estado812'; classe: Classe812 }
  | { tipo: 'saude'; saude: Saude }
  | { tipo: 'total'; bytes: number; sha256: string }

const DECIMAL = /^(0|[1-9][0-9]*)$/
const HEX64 = /^[0-9a-f]{64}$/
const UID_MAX = 4294967294 // 2^32 - 2: o UID 2^32 - 1 é "sem UID" no Linux.

/** Decimal canônico (sem zeros à esquerda, sem sinal) com no máximo `digitos` dígitos. */
function decimal(texto: string, digitos: number, maximo = Number.MAX_SAFE_INTEGER): number | null {
  if (texto.length > digitos || !DECIMAL.test(texto)) return null
  const n = Number(texto)
  return n <= maximo ? n : null
}

/**
 * `computerId` do Hub (host/src/remote/crypto.rs): SHA-256(hostSignPub)[0..16] em base64url sem padding: 22 caracteres
 * `[A-Za-z0-9_-]`; 16 bytes deixam 4 bits úteis no último caractere, que por isso só pode ser A, Q, g ou w.
 */
export const COMPUTER_ID = /^[A-Za-z0-9_-]{21}[AQgw]$/
export const ehComputerId = (v: unknown): v is string => typeof v === 'string' && COMPUTER_ID.test(v)

const VERSAO = /^(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})$/
export const ehVersao = (v: unknown): v is string => typeof v === 'string' && VERSAO.test(v)

/** Identificador de tentativa: UUID gerado pelo app, validado como hexadecimal antes de entrar em qualquer comando. */
export const ID_TENTATIVA = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
export const ehIdTentativa = (v: unknown): v is string => typeof v === 'string' && ID_TENTATIVA.test(v)

// MARK: JSON estrito para `saude`

/** `donnabot-host info --json` (host/src/main.rs `info`): esquema fechado. O token é lido só para validar e nunca sai daqui. */
export interface Saude {
  name: string
  computerId: string
  signKey: string
  boxKey: string | null
  version: string
  port: number
  running: boolean
}
const CHAVES_SAUDE = ['name', 'computerId', 'signKey', 'boxKey', 'version', 'port', 'token', 'running'] as const
const PROFUNDIDADE_MAX = 4

class JsonInvalido extends Error {}

/** JSON estrito: recusa chave repetida, profundidade > 4, números fora do inteiro seguro e texto depois do valor. */
export function jsonEstrito(texto: string): unknown {
  let i = 0
  const falhar = (): never => { throw new JsonInvalido() }
  const espaco = () => { while (texto[i] === ' ' || texto[i] === '\n' || texto[i] === '\t' || texto[i] === '\r') i++ }
  const cadeia = (): string => {
    if (texto[i] !== '"') falhar()
    const inicio = i++
    while (i < texto.length && texto[i] !== '"') {
      if (texto.charCodeAt(i) < 0x20) falhar()
      if (texto[i] === '\\') i++
      i++
    }
    if (texto[i] !== '"') falhar()
    i++
    return JSON.parse(texto.slice(inicio, i)) as string
  }
  const valor = (profundidade: number): unknown => {
    espaco()
    const c = texto[i]
    if (c === '{' || c === '[') {
      if (profundidade >= PROFUNDIDADE_MAX) falhar()
      i++
      espaco()
      if (c === '[') {
        const lista: unknown[] = []
        if (texto[i] === ']') { i++; return lista }
        for (;;) {
          lista.push(valor(profundidade + 1)); espaco()
          if (texto[i] === ',') { i++; continue }
          if (texto[i] === ']') { i++; return lista }
          return falhar()
        }
      }
      const objeto = Object.create(null) as Record<string, unknown>
      if (texto[i] === '}') { i++; return objeto }
      for (;;) {
        espaco()
        const chave = cadeia()
        if (chave in objeto) falhar()
        espaco()
        if (texto[i++] !== ':') falhar()
        objeto[chave] = valor(profundidade + 1)
        espaco()
        if (texto[i] === ',') { i++; continue }
        if (texto[i] === '}') { i++; return objeto }
        return falhar()
      }
    }
    if (c === '"') return cadeia()
    for (const [palavra, v] of [['true', true], ['false', false], ['null', null]] as const) {
      if (texto.startsWith(palavra, i)) { i += palavra.length; return v }
    }
    const numero = /^-?(0|[1-9][0-9]*)/.exec(texto.slice(i))
    if (!numero) return falhar()
    i += numero[0].length
    const n = Number(numero[0])
    return Number.isSafeInteger(n) ? n : falhar()
  }
  const resultado = valor(0)
  espaco()
  if (i !== texto.length) falhar()
  return resultado
}

const SEGREDO_PUBLICO = /^[A-Za-z0-9_-]{1,128}$/

function validarSaude(texto: string): Saude | null {
  let v: unknown
  try { v = jsonEstrito(texto) } catch { return null }
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null
  const o = v as Record<string, unknown>
  const chaves = Object.keys(o)
  if (chaves.some((k) => !(CHAVES_SAUDE as readonly string[]).includes(k))) return null
  const { name, computerId, signKey, boxKey, version, port, token, running } = o
  if (typeof name !== 'string' || name.length < 1 || name.length > 128 || /[\u0000-\u001f\u007f]/.test(name)) return null
  if (!ehComputerId(computerId) || !ehVersao(version) || typeof running !== 'boolean') return null
  if (typeof signKey !== 'string' || !SEGREDO_PUBLICO.test(signKey)) return null
  if (boxKey !== undefined && boxKey !== null && (typeof boxKey !== 'string' || !SEGREDO_PUBLICO.test(boxKey))) return null
  if (typeof token !== 'string' || token.length < 1 || token.length > 256 || /[\s\u0000-\u001f\u007f]/.test(token)) return null
  if (typeof port !== 'number' || !Number.isInteger(port) || port < 1 || port > 65535) return null
  return { name, computerId, signKey, boxKey: typeof boxKey === 'string' ? boxKey : null, version, port, running }
}

// MARK: validação por tipo

/** O conteúdo (já com o tamanho conferido) de uma mensagem de dados, ou null se fugir da gramática. */
export function validarDado(tipo: TipoDado, conteudo: string): Dado | null {
  switch (tipo) {
    case 'tam': {
      const bytes = decimal(conteudo, TETO_DADO.tam)
      return bytes === null ? null : { tipo, bytes }
    }
    case 'hashparcial':
      return HEX64.test(conteudo) ? { tipo, sha256: conteudo } : null
    case 'uid': {
      const [a, h, resto] = conteudo.split(' ')
      if (a === undefined || h === undefined || resto !== undefined) return null
      const uidAdmin = decimal(a, 10, UID_MAX)
      const uidHub = h === '-' ? null : decimal(h, 10, UID_MAX)
      return uidAdmin === null || (h !== '-' && uidHub === null) ? null : { tipo, uidAdmin, uidHub }
    }
    case 'arq':
      return (ARQUITETURAS as readonly string[]).includes(conteudo) ? { tipo, arquitetura: conteudo as Arquitetura } : null
    case 'so': {
      const [id, versao, resto] = conteudo.split(' ')
      return resto === undefined && versao !== undefined && (IDS_SO as readonly string[]).includes(id ?? '') && VERSAO_SO.test(versao) ? { tipo, id: id as IdSo, versao } : null
    }
    case 'host': {
      const [id, versao, resto] = conteudo.split(' ')
      return resto === undefined && ehComputerId(id) && ehVersao(versao) ? { tipo, computerId: id, versao } : null
    }
    case 'estado812':
      return (CLASSES_812 as readonly string[]).includes(conteudo) ? { tipo, classe: conteudo as Classe812 } : null
    case 'saude': {
      const saude = validarSaude(conteudo)
      return saude ? { tipo, saude } : null
    }
    case 'total': {
      const [n, hash, resto] = conteudo.split(' ')
      const bytes = n === undefined ? null : decimal(n, 12)
      return resto === undefined && bytes !== null && hash !== undefined && HEX64.test(hash) ? { tipo, bytes, sha256: hash } : null
    }
  }
}

// MARK: registro local da tentativa (§3)

/**
 * `estado.json` (espelho remoto) e o registro do Mac têm este formato. Nunca leva senha. O espelho remoto é dado não
 * confiável: serve só para retomar transferência.
 */
export interface RegistroTentativa {
  id: string
  estado: string
  lingerJaAtivo: boolean | null
  hash: string | null
}

export function validarRegistroTentativa(v: unknown, estadosValidos: readonly string[]): RegistroTentativa | null {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null
  const o = v as Record<string, unknown>
  if (Object.keys(o).some((k) => !['id', 'estado', 'lingerJaAtivo', 'hash'].includes(k))) return null
  if (!ehIdTentativa(o.id) || typeof o.estado !== 'string' || !estadosValidos.includes(o.estado)) return null
  if (o.lingerJaAtivo !== null && typeof o.lingerJaAtivo !== 'boolean') return null
  if (o.hash !== null && !(typeof o.hash === 'string' && HEX64.test(o.hash))) return null
  return { id: o.id, estado: o.estado, lingerJaAtivo: o.lingerJaAtivo, hash: o.hash }
}
