import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
// voz-03 (D237): regras puras da tela Voz deste aparelho. Sem I/O, sem React, sem prefs:
// o que decide fonte, gestão da chave no Hub, aviso de custo e o mini que não sobe sozinho.

export type VozProvedor = 'openai' | 'gemini'
/** `device`: Ditado do macOS (ouvir) ou Voz do macOS (ler); senão, um provedor pago. */
export type EscolhaVoz = 'device' | VozProvedor

export interface StatusVoz {
  providers: { provider: VozProvedor; configured: boolean }[]
}

/** De onde viria a chave do provedor, olhando só o Hub do bot da conversa (D229 A; o aparelho chega no voz-04). */
export type FonteChave = 'hub' | 'nenhuma' | 'desconhecida'

export function fonteDaChave(status: StatusVoz | null | undefined, p: VozProvedor): FonteChave {
  if (!status) return 'desconhecida'
  return status.providers.some((x) => x.provider === p && x.configured) ? 'hub' : 'nenhuma'
}

interface HubAlcance {
  readonly isLoopback: boolean
  readonly connection: { kind: string }
}

/**
 * A linha "Chave no Hub <nome>" (setVoiceKey, LOCAL_ONLY no Hub) só existe quando o app alcança o Hub
 * no próprio computador ou pelo túnel SSH. Pela nuvem, nunca: o Hub recusaria e a tela não oferece.
 */
export const podeGerirNoHub = (store: HubAlcance | null | undefined) => !!store && store.isLoopback && store.connection.kind === 'online'

/**
 * Valor inicial de "Voz para texto" a partir das preferências que já existem, sem mudar o que cada
 * pessoa já usava: no modo Fala de um provedor, a transcrição já era desse provedor; no resto, Ditado.
 */
export function ouvirInicial(engine: string, modoDe: (p: VozProvedor) => string): EscolhaVoz {
  if ((engine === 'openai' || engine === 'gemini') && modoDe(engine) === 'speech') return engine
  return 'device'
}

/** Sugestão do SIM das imagens: com a Voz do macOS no Ao vivo, ouvir por um provedor ainda cobra. */
export const avisoLeituraGratis = (aoVivo: EscolhaVoz, ouvir: EscolhaVoz) => aoVivo === 'device' && ouvir !== 'device'
export const TEXTO_LEITURA_GRATIS = 'voz.ajustes.leituraGratis' satisfies ChaveTexto

/**
 * A linha da Voz para texto com a entrada local escolhida. Sem Ditado neste aparelho (Windows, Linux, Mac
 * sem o ajudante), ela aparece indisponível e nunca nomeia o Ditado do macOS; o modo Fala pede uma escolha paga.
 */
export function linhaOuvirLocal(ditado: boolean, indisponivel: ChaveTexto): { ok: boolean; partes: Mensagem[] } {
  if (ditado) return { ok: true, partes: [{ chave: 'voz.ajustes.fonte.ditado' }] }
  return { ok: false, partes: [{ chave: indisponivel }, { chave: 'voz.ajustes.fonte.semDitado' }] }
}

/** Ao vivo no modo Fala sem Voz para texto utilizável. */
export const TEXTO_FALA_SEM_OUVIR = 'voz.ajustes.falaSemOuvir' satisfies ChaveTexto
/** O mesmo aviso na tela de Ajustes, onde Voz para texto é outra aba (ajustes-tela-01). */
export const TEXTO_FALA_SEM_OUVIR_ABAS = 'voz.ajustes.falaSemOuvirAbas' satisfies ChaveTexto

/**
 * Rodapé de "Chaves no computador principal" (sem o nome do computador). Hub no próprio aparelho: o cofre é do Hub, não do app, com a linha
 * de diferença do plano (§2); Hub pelo túnel SSH: as chaves ficam lá, não aqui.
 */
/** Auditoria D249, F10: a frase à vista sob Chaves no Hub; o rodapé inteiro fica em "Saiba mais". */
export function resumoChavesNoHub(): Mensagem {
  return { chave: 'voz.ajustes.hub.resumo' }
}

export function rodapeChavesNoHub(mesmoAparelho: boolean): Mensagem {
  return { chave: mesmoAparelho ? 'voz.ajustes.hub.rodapeAqui' : 'voz.ajustes.hub.rodapeFora' }
}

// MARK: o mini continua mini (S8)

export type TipoModelo = 'realtime' | 'transcribe' | 'speech'
export const TIPOS_MODELO: TipoModelo[] = ['realtime', 'transcribe', 'speech']

/** Classe econômica do modelo, pelo nome publicado pelo provedor (gpt-4o-mini-tts, gpt-realtime-mini, flash-lite…). */
export const isMini = (modelo: string) => /(^|[-_.])(mini|lite|nano)([-_.]|$)/i.test(modelo)

/**
 * Padrões novos do Hub sem promover ninguém: para cada tipo sem escolha da pessoa, se o modelo em uso era
 * mini e o novo padrão não é, devolve o mini para ficar guardado como escolha. Nunca troca sozinho para cima.
 */
export function fixarMinis(
  emUso: Record<TipoModelo, string>,
  novos: Record<TipoModelo, string>,
  escolhido: (t: TipoModelo) => string | null,
): Partial<Record<TipoModelo, string>> {
  const fixar: Partial<Record<TipoModelo, string>> = {}
  for (const t of TIPOS_MODELO) {
    if (escolhido(t) !== null) continue
    if (isMini(emUso[t]) && !isMini(novos[t])) fixar[t] = emUso[t]
  }
  return fixar
}

/** Escolha que guarda: um mini sempre fica gravado, mesmo igual ao padrão, para nunca subir com um padrão novo. */
export const escolhaParaGuardar = (modelo: string, padrao: string): string | null =>
  !modelo ? null : isMini(modelo) ? modelo : modelo === padrao ? null : modelo

/** O modelo escolhido sumiu da lista do provedor: avisar e pedir outra escolha, sem trocar sozinho. */
export const modeloSumiu = (modelo: string, lista: readonly string[]) => lista.length > 0 && !lista.includes(modelo)

// MARK: textos que não podem repetir a chave

/** Formatos de chave da OpenAI e do Gemini, para não exibir uma chave que o provedor devolva no erro. */
const FORMA_DE_CHAVE = /\b(sk-[A-Za-z0-9_-]{8,}|AIza[0-9A-Za-z_-]{10,})/g

/** Tira do texto qualquer trecho da chave digitada (inteira ou um pedaço longo dela) e o que tiver forma de chave. */
export function semChave(texto: string, chave: string): string {
  const k = chave.trim()
  let out = texto.replace(FORMA_DE_CHAVE, '•••')
  if (k.length < 4) return out
  out = out.split(k).join('•••')
  // Pedaços longos (um provedor que ecoa só o começo ou o fim da chave).
  for (const n of [16, 8]) {
    if (k.length <= n) continue
    for (let i = 0; i + n <= k.length; i++) out = out.split(k.slice(i, i + n)).join('•••')
  }
  return out
}
