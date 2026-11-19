// MEX-1307 F0 (plano §3, "Compromisso de consentimento"): `H = SHA-256(...)` sobre os campos que o dono aprova. A
// aprovação vale só para esse `H` e **nunca** é reaproveitada numa retomada; `H` só explica o que mudou.
import { createHash } from 'node:crypto'
import type { Arquitetura, Classe812, IdSo } from './esquemas.ts'

/** Impressão digital no formato do `ssh-keygen -l`: `SHA256:` + 43 caracteres base64 sem padding. */
export const FINGERPRINT = /^SHA256:[A-Za-z0-9+/]{43}$/
export const ehFingerprint = (v: unknown): v is string => typeof v === 'string' && FINGERPRINT.test(v)

export interface EstadoSondado {
  instalado: boolean
  usuarioMotor: boolean
  /** `null` = não consegui ler. */
  linger: boolean | null
  hostOutroComputador: boolean
  /** Resultado da sondagem privilegiada; entra em `H` (§4.3.1b). */
  classe812: Classe812
}

export interface EntradaConsentimento {
  /** Impressões digitais aceitas pelo dono. */
  chaves: readonly string[]
  usuario: string
  uid: number
  /** Sistema operacional sondado (`ID` e `VERSION_ID` do os-release): Debian e Ubuntu não se confundem no `H`. */
  so: { id: IdSo; versao: string }
  arquitetura: Arquitetura
  /** SHA-256 do `SHA256SUMS` do pacote (hex minúsculo) e a versão. */
  pacoteDigest: string
  versao: string
  estado: EstadoSondado
  /** Lista de mudanças, como códigos do catálogo (`usuario`, `grupo`, `chaveapp`…), na ordem em que acontecem. */
  mudancas: readonly string[]
}

export type CampoConsentimento = 'chaves' | 'usuario' | 'uid' | 'so' | 'arquitetura' | 'pacote' | 'estado' | 'mudancas'

/** Campo `nome=<comprimento>:<valor>\n`: o comprimento evita que `a`+`bc` e `ab`+`c` gerem o mesmo texto. */
const campo = (nome: string, valor: string) => `${nome}=${Buffer.byteLength(valor)}:${valor}\n`
const lista = (itens: readonly string[]) => itens.map((i) => `${Buffer.byteLength(i)}:${i}`).join(',')

function campos(e: EntradaConsentimento): Record<CampoConsentimento, string> {
  return {
    chaves: lista([...e.chaves].sort()),
    usuario: e.usuario,
    uid: String(e.uid),
    so: `${e.so.id} ${e.so.versao}`,
    arquitetura: e.arquitetura,
    pacote: `${e.pacoteDigest}@${e.versao}`,
    estado: [e.estado.instalado, e.estado.usuarioMotor, e.estado.linger ?? 'desconhecido', e.estado.hostOutroComputador, e.estado.classe812].join(','),
    mudancas: lista(e.mudancas),
  }
}

const ORDEM: readonly CampoConsentimento[] = ['chaves', 'usuario', 'uid', 'so', 'arquitetura', 'pacote', 'estado', 'mudancas']

export function calcularH(e: EntradaConsentimento): string {
  const c = campos(e)
  const texto = 'mexerica-consentimento-v1\n' + ORDEM.map((k) => campo(k, c[k])).join('')
  return createHash('sha256').update(texto, 'utf8').digest('hex')
}

/** O que mudou entre a tentativa anterior e esta, em ordem fixa; vazio = "nada mudou desde a tentativa anterior". */
export function diferencas(antes: EntradaConsentimento, agora: EntradaConsentimento): CampoConsentimento[] {
  const a = campos(antes)
  const b = campos(agora)
  return ORDEM.filter((k) => a[k] !== b[k])
}
