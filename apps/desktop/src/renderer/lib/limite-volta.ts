// MX-245: a hora em que o limite do plano volta, em texto no idioma do app. Só usa o que o provedor informou: um
// instante (epoch ms), um texto dele, ou nada (então diz que não informa; nunca chuta). Módulo puro: o relógio e o fuso
// entram por parâmetro, para os testes fixarem "agora" e a virada de dia. As telas só chamam `t(...)` com o resultado.
import type { AppLanguage } from '../../shared/fork-idioma.ts'
import { direcaoDe } from '../../shared/fork-idioma.ts'
import { ltr } from '../../shared/bidi.ts'
import { formatarData, localeDe, type Tradutor } from './fork-i18n/index.ts'

const DIA = 86_400_000

/**
 * Marcas bidi que o próprio `Intl.DateTimeFormat` já injeta em locales RTL (LRM/RLM e isoladores): isoladas de novo
 * por cima, embaralham a ordem dos números (MX-245, ex.: a data vira "1811/"). Remover antes de isolar com `ltr()`,
 * que já cuida da direção da pessoa toda.
 */
const MARCAS_BIDI = /[\u200e\u200f\u2066-\u2069]/g

/** O dia civil de `instante` no fuso (o do computador, quando não vem), como número de dias desde 1970. */
function numeroDoDia(instante: number, fuso?: string): number {
  const p = Object.fromEntries(
    new Intl.DateTimeFormat('en-US', { timeZone: fuso, year: 'numeric', month: 'numeric', day: 'numeric' })
      .formatToParts(instante).map((x) => [x.type, x.value]),
  )
  return Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day)) / DIA
}

/**
 * Quando o limite volta, no fuso de quem vê: hoje → "volta às 14:30"; amanhã → "volta amanhã às 09:00"; até 6 dias
 * → "volta seg 09:00"; depois → "volta 14/11 às 09:00". Instante já passado: "o prazo informado já passou (…)",
 * porque o provedor pode ainda não ter atualizado o dado. Sem instante válido: `null`.
 */
export function voltaEm(t: Tradutor, idioma: AppLanguage, instante: number | null | undefined, agora: number = Date.now(), fuso?: string): string | null {
  if (instante == null || !Number.isFinite(instante) || Number.isNaN(new Date(instante).getTime())) return null
  const rtl = direcaoDe(idioma) === 'rtl'
  // Em árabe, hora e data entram isoladas (sem as marcas bidi do próprio Intl, que reordenam por cima), e a hora
  // sai em h23: sem ela, o marcador ص/م (letra forte à direita dos dígitos) inverte a leitura da hora na frase.
  const nasce = (s: string) => (rtl ? ltr(s.replace(MARCAS_BIDI, '')) : s)
  const fmt = (o: Intl.DateTimeFormatOptions) => nasce(formatarData(idioma, instante, { ...(rtl ? { hourCycle: 'h23' } : {}), ...o, timeZone: fuso }))
  const hora = fmt({ hour: '2-digit', minute: '2-digit' })
  const dias = numeroDoDia(instante, fuso) - numeroDoDia(agora, fuso)
  if (instante < agora) {
    const quando = dias === 0 ? hora : fmt({ day: 'numeric', month: 'numeric', hour: '2-digit', minute: '2-digit' })
    return t('sistema.barra.uso.volta.passou', { quando })
  }
  if (dias === 0) return t('sistema.barra.uso.volta.hoje', { hora })
  if (dias === 1) return t('sistema.barra.uso.volta.amanha', { hora })
  // O dia abreviado sai como o CLDR do idioma formata (com o ponto onde ele pede, como "qui." e "jeu."): tirar o
  // ponto sempre trocava "jeu." (abreviação de quinta) por "jeu" (a palavra "jogo" em francês) — MX-245.
  if (dias < 7) return t('sistema.barra.uso.volta.semana', { dia: fmt({ weekday: 'short' }), hora })
  return t('sistema.barra.uso.volta.data', { data: fmt({ day: 'numeric', month: 'numeric' }), hora })
}

/**
 * Forma curta para onde a frase não cabe (a ilha): "09:00", "amanhã 09:00", "seg 09:00", "18/11, 09:00". "Amanhã" vem do
 * `Intl.RelativeTimeFormat` do idioma, sem texto novo. Sem instante válido ou já passado: `null` (a tela deixa a frase
 * completa no `title`).
 */
export function voltaCurta(idioma: AppLanguage, instante: number | null | undefined, agora: number = Date.now(), fuso?: string): string | null {
  if (instante == null || !Number.isFinite(instante) || Number.isNaN(new Date(instante).getTime()) || instante < agora) return null
  const rtl = direcaoDe(idioma) === 'rtl'
  const nasce = (s: string) => (rtl ? ltr(s.replace(MARCAS_BIDI, '')) : s)
  const fmt = (o: Intl.DateTimeFormatOptions) => formatarData(idioma, instante, { ...(rtl ? { hourCycle: 'h23' } : {}), ...o, timeZone: fuso })
  const hora = nasce(fmt({ hour: '2-digit', minute: '2-digit' }))
  const dias = numeroDoDia(instante, fuso) - numeroDoDia(agora, fuso)
  if (dias === 0) return hora
  if (dias === 1) return `${new Intl.RelativeTimeFormat(localeDe(idioma), { numeric: 'auto' }).format(1, 'day')} ${hora}`
  if (dias < 7) return `${fmt({ weekday: 'short' })} ${hora}`
  return nasce(fmt({ day: 'numeric', month: 'numeric', hour: '2-digit', minute: '2-digit' }))
}

/** O que o provedor informou de uma janela: instante (`resetsAt`, ou o que se lê do texto dele) e/ou o texto. */
export interface QuandoInformado { instante: number | null; texto: string | null }

/**
 * A frase da janela de limite no painel: instante → `voltaEm`; só texto do provedor sem instante legível → o texto como
 * veio (sempre da esquerda para a direita, é do provedor); nada → "o provedor não informa quando volta".
 */
export function voltaDaJanela(t: Tradutor, idioma: AppLanguage, q: QuandoInformado, agora: number = Date.now(), fuso?: string): string {
  const quando = voltaEm(t, idioma, q.instante, agora, fuso)
  if (quando) return quando
  const texto = q.texto?.trim()
  if (texto) return t('sistema.barra.uso.volta.texto', { texto: direcaoDe(idioma) === 'rtl' ? ltr(texto) : texto })
  return t('sistema.barra.uso.volta.desconhecido')
}

/** Os campos da espera do Hub (`Entry.data.subscriptionWait`) que decidem a frase da conversa. */
export interface EsperaDoLimite { resumeAt?: number | null; cancelled?: boolean; ready?: boolean; autoResume?: boolean; snoozed?: boolean }

/**
 * A frase "Parou pelo limite do plano; volta às HH:MM" para a espera ativa e automática. Devolve `null` quando o texto
 * do Hub diz outra coisa que importa mais (cancelada, retomada automática desligada, prazo que chegou ou já vencido pelo
 * relógio, pausada por cinco retomadas): nesses casos a linha continua a do Hub.
 */
export function paradaPeloLimite(t: Tradutor, idioma: AppLanguage, espera: EsperaDoLimite, pausada: boolean, agora: number = Date.now(), fuso?: string): string | null {
  if (pausada || espera.cancelled || espera.ready || espera.autoResume === false) return null
  // Prazo vencido (Hub antigo sem `ready`, ou antes de o Hub atualizar o aviso): o texto do Hub fica, sem "já passou".
  if (typeof espera.resumeAt === 'number' && espera.resumeAt <= agora) return null
  const quando = voltaEm(t, idioma, espera.resumeAt, agora, fuso)
  return quando ? t('sistema.conversa.espera.parou', { volta: quando }) : t('sistema.conversa.espera.parouSemHora')
}
