import { readSkillSelectionStamp } from '../../../../shared/fork-skill-selection.ts'
import { mesmaOrigem, origemDaFonte, type FonteSkills, type OrigemLeitura } from '../catalogo-skills/consumidores.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { formatarData, formatarNumero } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'

// R3: tamanho do índice de skills que o perfil do bot recebe, medido pelo Hub na mesma montagem
// do bloco (`entrega::medida`). Só leitura; nada aqui corta, limita ou muda a seleção.

/** A medida do Hub: seleção do bot, linhas anunciadas depois da conferência R1 e bytes UTF-8 do bloco. */
export type IndiceSkills = Readonly<{ ligadas: number; noIndice: number; bytes: number }>

/**
 * Uma leitura presa à origem (Hub, link e geração) e à seleção: `pedidaPara` é a versão c1 da seleção
 * que estava na tela no pedido; `versao` é a que o Hub mediu, da mesma linha lida para a medida.
 */
export type LeituraIndice = Readonly<{ origem: OrigemLeitura; pedidaPara: string | null; versao: string | null; indice: IndiceSkills | null; lidoEm: number }>

export type EstadoIndice =
  | { tipo: 'medido'; indice: IndiceSkills; lidoEm: number }
  | { tipo: 'conferindo' }
  | { tipo: 'indisponivel' }


const inteiro = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0

/** `index` do Hub; ausente (Hub antigo), null (registro não conferido) ou incoerente: indisponível, nunca zero. */
export function normalizarIndice(raw: unknown): { indice: IndiceSkills; versao: string } | null {
  if (typeof raw !== 'object' || raw === null) return null
  const { enabled, listed, bytes, selectionVersion } = raw as Record<string, unknown>
  if (!inteiro(enabled) || !inteiro(listed) || !inteiro(bytes) || listed > enabled || (listed === 0) !== (bytes === 0)) return null
  const versao = readSkillSelectionStamp({ protocol: 1, version: selectionVersion })?.version
  return versao ? { indice: { ligadas: enabled, noIndice: listed, bytes }, versao } : null
}

/** A versão c1 da seleção que o bot mostra agora; sem ela, nada a comparar. */
export const versaoDaSelecao = (bot: { skillSelection?: unknown }) => readSkillSelectionStamp(bot.skillSelection)?.version ?? null

/** Abaixo de 1.000 bytes, em bytes; daí em diante, KB de 1.000 bytes com uma casa (números no idioma). */
export function formatarTamanho(idioma: AppLanguage, bytes: number): string {
  if (bytes === 0) return `${formatarNumero(idioma, 0)} KB`
  if (bytes < 1000) return `${formatarNumero(idioma, bytes)} bytes`
  return `${formatarNumero(idioma, Math.round(bytes / 100) / 10, { minimumFractionDigits: 1, maximumFractionDigits: 1 })} KB`
}

/** A linha e o detalhe da medida, no idioma (bots.ajustes.indice.*). Zero tem chave própria: no CLDR do pt, 0 cai em `one`. */
export function textoDoIndice(t: Tradutor, idioma: AppLanguage, i: IndiceSkills, lidoEm: number): { linha: string; detalhe: string } {
  const linha = t('bots.ajustes.indice.linha', { ligadas: i.ligadas === 0 ? t('bots.ajustes.indice.nenhumaLigada', { n: formatarNumero(idioma, 0) }) : t.plural('bots.ajustes.indice.ligadas', i.ligadas), noIndice: formatarNumero(idioma, i.noIndice), tamanho: formatarTamanho(idioma, i.bytes) })
  const detalhe = t('bots.ajustes.indice.detalhe', { bytes: formatarNumero(idioma, i.bytes), hora: formatarData(idioma, lidoEm, { hour: '2-digit', minute: '2-digit', hour12: false }) })
  return { linha, detalhe }
}

/**
 * O que a tela mostra agora. Só vale como medida a leitura da origem viva cuja versão medida pelo Hub
 * é a da seleção na tela; a mesma contagem com outra versão não serve.
 */
export function estadoDoIndice(fonte: FonteSkills, leitura: LeituraIndice | null, versaoAtual: string | null): EstadoIndice {
  if (!leitura || versaoAtual === null || !mesmaOrigem(fonte, leitura.origem)) return { tipo: 'indisponivel' }
  if (leitura.pedidaPara !== versaoAtual) return { tipo: 'conferindo' }
  const i = leitura.indice
  if (!i) return { tipo: 'indisponivel' }
  if (leitura.versao !== versaoAtual) return { tipo: 'conferindo' }
  return { tipo: 'medido', indice: i, lidoEm: leitura.lidoEm }
}

type Cliente = { call<T>(method: string, body?: unknown): Promise<T> }

/** A recusa de formato de um Hub sem o `botId` (anterior ao R3); qualquer outra falha não é "Hub antigo". */
export const FORMATO_ANTIGO = 'Intenção de consulta de habilidades inválida' // textos-ok: mensagem de recusa do Hub (protocolo), só comparada

/**
 * A lista de instaladas e, na mesma leitura do Hub, a medida do índice deste bot (`skills {botId}`).
 * Só a recusa de formato, com a origem do pedido ainda viva, relê a lista sem `botId` (Hub antigo:
 * medida indisponível). Autoridade, Space, rede, registro ou origem trocada: o erro segue, sem segunda leitura.
 * O link usado precisa ser o vivo da origem capturada; senão a medida já nasce indisponível.
 */
export async function lerSkillsDoBot<T>(fonte: FonteSkills, client: Cliente, botId: string, pedidaPara: string | null): Promise<{ items: T[]; leitura: LeituraIndice }> {
  const origem = origemDaFonte(fonte)
  const doLink = origem !== null && origem.client === client
  const base = { origem, pedidaPara, lidoEm: Date.now() }
  let resposta: { items: T[]; index?: unknown }
  try {
    resposta = await client.call<{ items: T[]; index?: unknown }>('skills', { botId })
  } catch (e) {
    if (!doLink || !mesmaOrigem(fonte, origem) || !(e instanceof Error) || e.message.trim() !== FORMATO_ANTIGO) throw e
    const lista = await client.call<{ items: T[] }>('skills')
    return { items: lista.items, leitura: { ...base, versao: null, indice: null } }
  }
  const medida = doLink ? normalizarIndice(resposta.index) : null
  return { items: resposta.items, leitura: { ...base, versao: medida?.versao ?? null, indice: medida?.indice ?? null } }
}
