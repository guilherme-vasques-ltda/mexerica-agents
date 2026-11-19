import type { SelecaoXai } from '../../../../shared/fork-voz-xai.ts'
import type { VoiceCallEngine } from '../../../../shared/analytics.ts'
import type { ChaveAparelho } from '../../../../shared/fork-voz.ts'
import { chaveDaquiDecide, type ParteLocal } from './chave-aparelho.ts'
import type { EscolhaVoz, VozProvedor } from './voz-model.ts'

// voz-03: qual motor leva a chamada, a partir das duas escolhas deste aparelho (Voz para texto e Ao vivo).
// Nunca escolhe um provedor pago que a pessoa não escolheu: sem voz utilizável, não há motor.

export type PlanoMotor =
  | { tipo: 'tempo-real'; provider: 'xai'; fonte: 'aparelho'; selecao: SelecaoXai }
  /** voz-04: `aparelho`, credencial curta feita pelo main com a chave deste aparelho; `hub`, pela reserva do Hub do bot. */
  | { tipo: 'tempo-real'; provider: VozProvedor; fonte: 'aparelho' | 'hub' }
  /** Modo Fala: transcreve por `ouvir` e lê por `ler`, os dois pelo Hub. */
  | { tipo: 'fala'; ouvir: VozProvedor; ler: VozProvedor }
  /** Modo Fala ouvindo pelo Ditado do macOS e lendo pelo provedor. `transicao`: o Ditado entrou no lugar da chave deste Mac. */
  | { tipo: 'fala-ditado'; ler: VozProvedor; transicao?: 'ouvir' }
  /** Ditado e Voz do macOS: tudo no Mac, grátis. `transicao`: que parte entrou no lugar da chave deste Mac. */
  | { tipo: 'local'; transicao?: ParteLocal; chavePendente?: true }
  /** Voz do macOS lendo, com a Voz para texto de um provedor ouvindo (cobra a transcrição). */
  | { tipo: 'local-ouvindo'; ouvir: VozProvedor; transicao?: 'ler' }
  | { tipo: 'sem-voz'; chavePendente?: true }
  /** Modo Fala sem Voz para texto utilizável: sem Ditado aqui, falta escolher a OpenAI ou o Gemini. */
  | { tipo: 'sem-ouvir' }
  /** voz-04 até o 05b: modo Fala com chave deste aparelho e sem Ditado (Windows/Linux). Avisa e não liga. */
  | { tipo: 'so-tempo-real'; provedores: VozProvedor[] }

export interface Escolhas {
  /** null: Voz do macOS. */
  aoVivo: VozProvedor | null
  fala: boolean
  ouvir: EscolhaVoz
  /** O Ditado do macOS existe neste aparelho (helper de fala). */
  ditado: boolean
  /** voz-04: as chaves deste aparelho (sem a chave), por provedor. Ausente: nenhuma. */
  aparelho?: Partial<Record<VozProvedor, ChaveAparelho>> | null
}

/**
 * O plano da chamada. Ordem das fontes no Tempo real: chave deste aparelho (mesmo travada ou recusada: aí
 * o main recusa e a chamada cai para o local, nunca para o Hub), depois a reserva do Hub. Até o voz-05b,
 * a chave deste aparelho não vale no modo Fala e impede a reserva **em cada parte**: no Mac, a parte
 * dela vira Ditado (ouvir) ou Voz do macOS (ler); sem Ditado, a chamada não liga.
 */
export function planoDoMotor(escolhas: Escolhas): PlanoMotor {
  // Sem confirmação do disco, nenhuma das partes pode iniciar uma fonte paga por tabela.
  const partes = [escolhas.aoVivo, ...(escolhas.fala || !escolhas.aoVivo ? [escolhas.ouvir] : [])]
  if (partes.some(p => p && p !== 'device' && chaveDaquiDecide(escolhas.aparelho?.[p]) &&
      ['incerta', 'limpeza-pendente'].includes(escolhas.aparelho![p]!.situacao))) {
    return escolhas.ditado ? { tipo: 'local', chavePendente: true } : { tipo: 'sem-voz', chavePendente: true }
  }
  const base = planoSemChaveDaqui(escolhas)
  const decide = (p: VozProvedor) => chaveDaquiDecide(escolhas.aparelho?.[p])
  switch (base.tipo) {
    case 'tempo-real':
      return { ...base, fonte: decide(base.provider) ? 'aparelho' : 'hub' }
    case 'fala': {
      const ouvir = decide(base.ouvir)
      const ler = decide(base.ler)
      if (!ouvir && !ler) return base
      if (!escolhas.ditado) return { tipo: 'so-tempo-real', provedores: [...new Set([...(ouvir ? [base.ouvir] : []), ...(ler ? [base.ler] : [])])] }
      if (ouvir && ler) return { tipo: 'local', transicao: 'ambos' }
      return ouvir ? { tipo: 'fala-ditado', ler: base.ler, transicao: 'ouvir' } : { tipo: 'local-ouvindo', ouvir: base.ouvir, transicao: 'ler' }
    }
    case 'fala-ditado':
      return decide(base.ler) ? { tipo: 'local', transicao: 'ler' } : base
    case 'local-ouvindo':
      return decide(base.ouvir) ? { tipo: 'local', transicao: 'ouvir' } : base
    default:
      return base
  }
}

function planoSemChaveDaqui({ aoVivo, fala, ouvir, ditado }: Escolhas): Exclude<PlanoMotor, { provider: 'xai' }> {
  if (aoVivo && !fala) return { tipo: 'tempo-real', provider: aoVivo, fonte: 'hub' }
  if (aoVivo) {
    if (ouvir !== 'device') return { tipo: 'fala', ouvir, ler: aoVivo }
    // Sem Ditado aqui (Windows/Linux, Mac sem o ajudante), a entrada local está indisponível: a
    // transcrição paga só com escolha explícita em Voz para texto, nunca o provedor do Ao vivo por tabela.
    return ditado ? { tipo: 'fala-ditado', ler: aoVivo } : { tipo: 'sem-ouvir' }
  }
  if (!ditado) return { tipo: 'sem-voz' }
  return ouvir === 'device' ? { tipo: 'local' } : { tipo: 'local-ouvindo', ouvir }
}

/**
 * Depois que o motor falha: um provedor pago cai para a voz local grátis (com aviso) quando há Ditado;
 * sem ele (Windows/Linux), a chamada avisa e termina, e voltar é "Ligar de novo", por clique. Falha da
 * voz local fica como está, com o motivo na barra.
 */
export const depoisDaFalha = (provedor: boolean, ditado: boolean): 'queda' | 'fim' | 'nada' => (!provedor ? 'nada' : ditado ? 'queda' : 'fim')

/** A queda gratuita depois de um provedor falhar: só no Mac com o Ditado presente. */
export const planoDeQueda = (ditado: boolean): PlanoMotor => (ditado ? { tipo: 'local' } : { tipo: 'sem-voz' })

export const nomeDoMotor = (plano: PlanoMotor): VoiceCallEngine =>
  plano.tipo === 'tempo-real' ? (plano.provider === 'xai' ? 'cloud' : plano.provider) : plano.tipo === 'local' ? 'on_device' : 'cloud'
