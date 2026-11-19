// Skills c2 (D288): a Base central, matriz Skill × Bot da página Skills. Modelo sem DOM nem React.
// A célula projeta só `cfg.skills` (a lista `skills` do bot no mirror) do Hub desta tela; o gesto é o
// do produtor c1 (`captureSkillChoice`), sem lista, RPC ou cópia de autoridade próprios. Aqui ficam só
// as marcas locais de um gesto (salvando, recusado, sem confirmação), presas à conexão em que nasceram.
import { readSkillSelectionStamp, SkillSelectionOutcomeUnknown, type SkillSelectionStamp } from '../../../../shared/fork-skill-selection.ts'
import { Observable } from '../../../lib/observable.ts'
import type { InstalledSkillV2 } from '../../../store/fork-skills.ts'
import { frase, plural, type Frase } from './textos.ts'

/** O que a matriz lê de um bot: nada além do vínculo, do carimbo c1 e do que identifica a coluna. */
export type BotDaMatriz = Readonly<{ id: string; name: string; title?: string | null; skills: readonly string[]; skillSelection?: SkillSelectionStamp | null }>
export type SkillDaMatriz = Pick<InstalledSkillV2, 'id' | 'name' | 'description' | 'installationId' | 'contentStatus'>

/** A conexão que a pessoa via no gesto (mesma tupla do c1: Hub, link vivo e geração). */
export type OrigemMatriz = Readonly<{ hub: string; cliente: unknown; geracao: number }>

type Marca =
  | { tipo: 'salvando'; ligada: boolean; pedido: boolean; antes: string; respondida: boolean }
  | { tipo: 'recusada'; mensagem: Frase }
  | { tipo: 'incerta'; ligada: boolean; antes: string }

export type Celula =
  /** Interruptor. `travada`: visível, mas sem gesto agora, com o motivo. */
  | { tipo: 'controle'; ligada: boolean; travada: Frase | null; recusa: Frase | null }
  | { tipo: 'salvando'; ligada: boolean; pedido: boolean }
  /** O Hub não confirmou o resultado: só o último valor confirmado, e conferir antes de outro gesto. */
  | { tipo: 'incerta'; ligada: boolean }
  /** Vínculo conhecido sem mudança possível daqui. */
  | { tipo: 'texto'; ligada: boolean; motivo: Frase | null }
  /** Vínculo não confirmado: nunca vira Não. */
  | { tipo: 'indisponivel'; motivo: Frase }

export type Linha = Readonly<{
  id: string
  nome: string
  descricao: string
  /** O que a linha diz do conteúdo instalado, ou que o id está ligado sem instalação. */
  aviso: 'alterada' | 'indisponivel' | 'orfa' | 'naoConferida' | null
  skill: SkillDaMatriz | null
  celulas: readonly Celula[]
  /** Ligadas confirmadas e células sem confirmação (incerta ou indisponível), contadas das mesmas células. */
  ligadas: number
  semConfirmacao: number
}>

// D454 (fatia 8b): frases do catálogo, traduzidas só ao mostrar; a loja guarda a frase (lição da R1 do #572).
export const TEXTOS_MATRIZ = {
  semVersao: frase('skills.matriz.semVersao'),
  outraPendente: frase('skills.matriz.outraPendente'),
  ligarAlterada: frase('skills.matriz.ligarAlterada'),
  arquivoIndisponivel: frase('skills.matriz.arquivoIndisponivel'),
  naoConferida: frase('skills.matriz.naoConferida'),
  orfaLigada: frase('skills.matriz.orfaLigada'),
  guardada: frase('skills.matriz.guardada'),
  conexaoMudou: frase('skills.matriz.conexaoMudou'),
}

const chave = (bot: string, skill: string) => `${bot}\u0000${skill}`
const mesma = (a: OrigemMatriz | null, b: OrigemMatriz | null) => !!a && !!b && a.hub === b.hub && a.cliente === b.cliente && a.geracao === b.geracao

/**
 * As marcas dos gestos da matriz de um store. Vivem enquanto a conexão do gesto for a atual: outra
 * conexão (outro Hub, reconexão, A→B→A) descarta todas, sem reenviar nem aplicar nada a outro destino.
 */
export class MarcasMatriz extends Observable {
  private origem: OrigemMatriz | null = null
  private readonly marcas = new Map<string, Marca>()
  /** Aviso da troca de conexão com gesto ainda aberto; some no próximo gesto. */
  aviso: Frase | null = null

  /** Adota a conexão de agora; outra conexão descarta as marcas da anterior. */
  sincronizar(origem: OrigemMatriz | null) {
    if (mesma(this.origem, origem) || (this.origem === null && origem === null)) return
    const abertas = [...this.marcas.values()].some((m) => m.tipo !== 'recusada')
    this.origem = origem
    this.marcas.clear()
    if (abertas) this.aviso = TEXTOS_MATRIZ.conexaoMudou
    this.changed()
  }

  marca(bot: string, skill: string) {
    return this.marcas.get(chave(bot, skill)) ?? null
  }

  /**
   * Um gesto ainda em andamento neste bot (marca vigente: sem resposta, ou respondida e o mirror ainda na
   * versão do gesto). O carimbo c1 é por bot: um segundo gesto sobre a mesma leitura seria recusado.
   */
  salvandoNoBot(bot: BotDaMatriz) {
    for (const k of [...this.marcas.keys()]) {
      if (!k.startsWith(`${bot.id}\u0000`)) continue
      if (this.vigente(bot, k.slice(bot.id.length + 1))?.tipo === 'salvando') return true
    }
    return false
  }

  /** Começa um gesto já capturado pelo c1. `false`: conexão já é outra ou este bot já tem um gesto aberto. */
  iniciar(origem: OrigemMatriz, bot: BotDaMatriz, skill: string, pedido: boolean): boolean {
    const carimbo = readSkillSelectionStamp(bot.skillSelection)
    if (!carimbo || !mesma(this.origem, origem) || this.salvandoNoBot(bot)) return false
    this.aviso = null
    this.marcas.set(chave(bot.id, skill), { tipo: 'salvando', ligada: bot.skills.includes(skill), pedido, antes: carimbo.version, respondida: false })
    this.changed()
    return true
  }

  /** Recusa local do c1 antes de qualquer envio. */
  recusarNoGesto(bot: BotDaMatriz, skill: string, mensagem: Frase) {
    this.aviso = null
    this.marcas.set(chave(bot.id, skill), { tipo: 'recusada', mensagem })
    this.changed()
  }

  /**
   * O fim do envio. Sucesso deixa "salvando" até o mirror mostrar a versão nova do bot (o valor só muda
   * com o Hub). Recusa vira mensagem; resultado incerto vira "sem confirmação". Nada é reenviado.
   */
  terminar(origem: OrigemMatriz, bot: string, skill: string, resultado: { ok: true } | { erro: unknown; mensagem: Frase }) {
    const k = chave(bot, skill)
    const m = this.marcas.get(k)
    if (!mesma(this.origem, origem) || m?.tipo !== 'salvando') return
    if ('ok' in resultado) m.respondida = true
    else if (incerto(resultado.erro)) this.marcas.set(k, { tipo: 'incerta', ligada: m.ligada, antes: m.antes })
    else this.marcas.set(k, { tipo: 'recusada', mensagem: resultado.mensagem })
    this.changed()
  }

  /** Conferir: depois de reler no mesmo destino, a célula volta a mostrar só o que o Hub confirma; não reenvia. */
  conferir(origem: OrigemMatriz, bot: string, skill: string) {
    const k = chave(bot, skill)
    if (!mesma(this.origem, origem) || this.marcas.get(k)?.tipo !== 'incerta') return
    this.marcas.delete(k)
    this.changed()
  }

  /** Ler de novo: depois de reler no mesmo destino, tira o aviso da recusa. Não muda nem reenvia nada. */
  dispensarRecusa(origem: OrigemMatriz, bot: string, skill: string) {
    const k = chave(bot, skill)
    if (!mesma(this.origem, origem) || this.marcas.get(k)?.tipo !== 'recusada') return
    this.marcas.delete(k)
    this.changed()
  }

  /** Salvando/incerta vencidas pelo mirror: a versão do bot mudou desde o gesto, então o valor mostrado já é o confirmado. */
  vigente(bot: BotDaMatriz, skill: string): Marca | null {
    const m = this.marca(bot.id, skill)
    // A recusa fica até o próximo gesto nesta célula: o evento que traz a versão nova não a apaga.
    if (!m || m.tipo === 'recusada') return m
    const agora = readSkillSelectionStamp(bot.skillSelection)?.version ?? null
    if (agora !== m.antes) {
      // Salvando sem resposta continua até a resposta chegar, mesmo se outro evento mudar a versão antes.
      if (m.tipo === 'salvando' && !m.respondida) return m
      // Vencida: some sem aviso (o valor mostrado já é o do Hub).
      this.marcas.delete(chave(bot.id, skill))
      return null
    }
    return m
  }
}

function incerto(erro: unknown) {
  const causa = erro instanceof Error ? erro.cause : null
  return erro instanceof SkillSelectionOutcomeUnknown || causa instanceof SkillSelectionOutcomeUnknown
}

/**
 * Quanto do Hub desta tela a matriz conhece:
 * - `vivo`: conectado, com as instaladas lidas nesta mesma conexão; dá para mudar.
 * - `guardado`: sem conexão, com o mirror e a lista guardados deste mesmo Hub; só leitura, com aviso.
 */
export type Leitura = 'vivo' | 'guardado'

/**
 * As linhas da matriz. Só chamar com as duas leituras presentes (instaladas e bots do mesmo Hub/Pessoal):
 * sem elas a tela não mostra célula nenhuma. Ids ligados em bots e ausentes das instaladas viram linhas
 * órfãs (o vínculo é real; a instalação, não).
 */
export function linhasDaMatriz(skills: readonly SkillDaMatriz[], bots: readonly BotDaMatriz[], marcas: MarcasMatriz, leitura: Leitura): Linha[] {
  const instaladas = new Set(skills.map((s) => s.id))
  const orfas = [...new Set(bots.filter((b) => readSkillSelectionStamp(b.skillSelection)).flatMap((b) => b.skills))].filter((id) => !instaladas.has(id)).sort()
  const linhas = [
    ...skills.map((s) => ({ id: s.id, nome: s.name, descricao: s.description, skill: s as SkillDaMatriz | null })),
    ...orfas.map((id) => ({ id, nome: id, descricao: '', skill: null })),
  ]
  return linhas.map((l) => {
    const aviso = l.skill === null ? 'orfa' as const
      : l.skill.contentStatus === 'changed' ? 'alterada' as const
        : l.skill.contentStatus === 'unavailable' ? 'indisponivel' as const
          : l.skill.contentStatus === 'verified' ? null : 'naoConferida' as const
    const celulas = bots.map((b) => celula(b, l.id, aviso, marcas, leitura))
    return {
      ...l, aviso, celulas,
      ligadas: celulas.filter((c) => c.tipo !== 'incerta' && c.tipo !== 'indisponivel' && c.ligada).length,
      semConfirmacao: celulas.filter((c) => c.tipo === 'incerta' || c.tipo === 'indisponivel').length,
    }
  })
}

function celula(bot: BotDaMatriz, skill: string, aviso: Linha['aviso'], marcas: MarcasMatriz, leitura: Leitura): Celula {
  // Sem carimbo c1 válido, a lista não é tratada como completa: nada de Sim/Não.
  if (!readSkillSelectionStamp(bot.skillSelection)) return { tipo: 'indisponivel', motivo: TEXTOS_MATRIZ.semVersao }
  const ligada = bot.skills.includes(skill)
  const marca = marcas.vigente(bot, skill)
  if (marca?.tipo === 'salvando') return { tipo: 'salvando', ligada: marca.ligada, pedido: marca.pedido }
  if (marca?.tipo === 'incerta') return { tipo: 'incerta', ligada: marca.ligada }
  if (leitura === 'guardado') return { tipo: 'texto', ligada, motivo: null }
  if (aviso === 'orfa') return { tipo: 'texto', ligada, motivo: ligada ? TEXTOS_MATRIZ.orfaLigada : null }
  if (aviso === 'indisponivel') return { tipo: 'texto', ligada, motivo: TEXTOS_MATRIZ.arquivoIndisponivel }
  if (aviso === 'naoConferida') return { tipo: 'texto', ligada, motivo: TEXTOS_MATRIZ.naoConferida }
  const recusa = marca?.tipo === 'recusada' ? marca.mensagem : null
  // O c1 aceita desligar uma versão alterada e recusa ligá-la.
  if (aviso === 'alterada' && !ligada) return { tipo: 'controle', ligada, travada: TEXTOS_MATRIZ.ligarAlterada, recusa }
  if (marcas.salvandoNoBot(bot)) return { tipo: 'controle', ligada, travada: TEXTOS_MATRIZ.outraPendente, recusa }
  return { tipo: 'controle', ligada, travada: null, recusa }
}

/** Texto do inverso, das mesmas células: sem total categórico quando alguma não está confirmada. */
export function textoInverso(linha: Pick<Linha, 'ligadas' | 'semConfirmacao' | 'celulas'>) {
  const total = linha.celulas.length
  if (linha.semConfirmacao === 0) return frase('skills.matriz.ligadaEm', { ligadas: linha.ligadas, bots: plural('skills.matriz.bots', total) })
  return frase('skills.matriz.confirmadaEm', { bots: plural('skills.matriz.bots', linha.ligadas), semConfirmacao: linha.semConfirmacao })
}
