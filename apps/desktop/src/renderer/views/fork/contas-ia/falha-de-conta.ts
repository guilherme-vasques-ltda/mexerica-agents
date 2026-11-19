// Falha de conta/motor ao preparar um pedido: o Hub grava só `reason: {code, accountId?, backend?}` (nunca rótulo, e-mail
// nem token); aqui o app resolve a conta pela lista que já tem e monta a frase em português simples, com o que fazer.
// Regras puras (sem DOM nem React), testáveis no Node. Falha fechada: sem conta resolvida, nunca inventa estado.
import type { ChaveTexto, Tradutor } from '../../../lib/fork-i18n/index.ts'
import { formatarNumero, localeDe } from '../../../lib/fork-i18n/index.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import type { AccountUsage } from '../../../../shared/fork-engine-accounts.ts'
import { isolado } from '../../../../shared/bidi.ts'
import { engineName, isEngine, whenText } from './regras.ts'

/** O que o Hub grava (contrato Hub ⇄ app). */
export interface FalhaReason { code: string; accountId?: string; backend?: string }

/** Só o que a frase usa de uma conta: rótulo e estado, nunca segredo. */
export interface ContaFalha { id: string; label: string; backend: string; state: string; origin?: string }

export type AcaoFalha = 'contas' | 'bot-motor' | 'bot-pasta' | 'uso-extra'
export interface BotaoFalha { acao: AcaoFalha; rotulo: string }

export interface FraseFalha {
  /** O que aconteceu. */
  texto: string
  /** Qual conta o bot usa (vazio quando não dá para dizer sem inventar). */
  contaDoBot: string
  /** Quais contas estão conectadas. */
  contasConectadas: string
  /** Primeiro o botão certo; depois, "Abrir Contas de IA" (sem repetir). */
  botoes: BotaoFalha[]
}

const CODIGO = /^[a-z][a-z_]{0,63}$/
const ID = /^[\w.:-]{1,128}$/

/** `data.reason` de um Hub novo; qualquer outra forma (Hub antigo, dado torto) vira `null`. */
export function lerReason(valor: unknown): FalhaReason | null {
  if (typeof valor !== 'object' || valor === null) return null
  const { code, accountId, backend } = valor as Record<string, unknown>
  if (typeof code !== 'string' || !CODIGO.test(code)) return null
  return {
    code,
    ...(typeof accountId === 'string' && ID.test(accountId) ? { accountId } : {}),
    ...(typeof backend === 'string' && ID.test(backend) ? { backend } : {}),
  }
}

/** Texto fixo do Hub antigo (sem `reason`): reconhece só o aviso de "preparo do pedido", para acrescentar o botão. */
export const ehAvisoDePreparo = (texto: string | undefined) => /^Não foi possível preparar o pedido/.test(texto ?? '')

const nomeDoMotor = (backend: string | undefined, t: Tradutor) => isEngine(backend) ? engineName(backend) : t('conversa.avisos.falha.motorPadrao')

const ESTADO = {
  connected: 'conversa.avisos.falha.estado.conectada',
  unverified: 'conversa.avisos.falha.estado.naoConfirmada',
  unavailable: 'conversa.avisos.falha.estado.indisponivel',
  disconnected: 'conversa.avisos.falha.estado.desconectada',
} as const satisfies Record<string, ChaveTexto>

/** O estado da conta no idioma do app; estado desconhecido do Hub = "não confirmada" (nunca "conectada"). */
export const estadoDaContaFalha = (t: Tradutor, state: string) => t(ESTADO[state as keyof typeof ESTADO] ?? ESTADO.unverified)

/** Os rótulos das contas conectadas, até `max`, e "e mais N" no fim. */
export function listaDeContas(idioma: AppLanguage, contas: readonly ContaFalha[], max = 4): string {
  const nomes = contas.filter((c) => c.state === 'connected').map((c) => isolado(c.label))
  const lista = new Intl.ListFormat(localeDe(idioma), { type: 'conjunction' })
  return lista.format(nomes.length > max ? [...nomes.slice(0, max), `+${nomes.length - max}`] : nomes)
}

export interface ContextoFalha {
  t: Tradutor
  idioma: AppLanguage
  /** Nome do bot; vazio vira "do bot". */
  bot: string
  /** A lista de Contas de IA do Hub; `null` = ainda não lida ou falhou (nada é afirmado). */
  contas: readonly ContaFalha[] | null
}

/**
 * A frase e os botões de uma falha. A conta vem por `accountId` na lista do Hub; sem a lista ou sem a conta nela,
 * a frase diz que não deu para confirmar e leva a Contas de IA. Nada daqui lê token, e-mail ou texto cru do erro.
 */
export function fraseDaFalha(reason: FalhaReason, { t, idioma, bot, contas }: ContextoFalha): FraseFalha {
  const F = 'conversa.avisos.falha.' as const
  const nome = bot.trim()
  const conta = reason.accountId && contas ? contas.find((c) => c.id === reason.accountId) ?? null : null
  const backend = conta?.backend ?? reason.backend
  const motor = nomeDoMotor(backend, t)
  const botNome = nome || '—'
  const p = { bot: botNome, motor, conta: conta?.label ?? '' }
  const reconectar: BotaoFalha = { acao: 'contas', rotulo: t(`${F}acao.reconectar`) }
  const abrir: BotaoFalha = { acao: 'contas', rotulo: t(`${F}acao.abrirContas`) }
  const trocar: BotaoFalha = { acao: 'bot-motor', rotulo: t(`${F}acao.trocarConta`) }
  const trocarModelo: BotaoFalha = { acao: 'bot-motor', rotulo: t(`${F}acao.trocarContaModelo`) }

  let texto = t(`${F}frase.generico`, p)
  let principal: BotaoFalha = abrir
  const naoSei = () => { texto = t(`${F}frase.naoSei`, p); principal = abrir }
  const daConta = (chave: 'desconectada' | 'naoConfirmada' | 'indisponivel' | 'semUso') => {
    if (reason.accountId && contas && !conta) { texto = t(`${F}frase.sumiu`, p); principal = trocar; return }
    if (!conta) return naoSei()
    texto = t(`${F}frase.${chave}`, p)
    principal = reconectar
  }
  const estadoDa = () => conta?.state === 'unverified' ? 'naoConfirmada' : conta?.state === 'unavailable' ? 'indisponivel' : conta?.state === 'connected' ? 'semUso' : 'desconectada'

  switch (reason.code) {
    case 'account_disconnected':
    case 'account_unavailable': daConta(estadoDa()); break
    case 'account_not_found': texto = t(`${F}frase.sumiu`, p); principal = trocar; break
    case 'account_incompatible': texto = t(`${F}frase.incompativel`, p); principal = trocar; break
    case 'cli_unavailable': texto = t(`${F}frase.programa`, p); principal = reconectar; break
    case 'model_unavailable': texto = t(`${F}frase.modelo`, p); principal = trocarModelo; break
    case 'model_option_unavailable': texto = t(`${F}frase.opcao`, p); principal = trocarModelo; break
    case 'setup_in_progress': texto = t(`${F}frase.emLogin`, p); break
    case 'space_account_selection_required': texto = t(`${F}frase.espaco`, p); break
    case 'engine_unavailable': texto = t(`${F}frase.motorIlegivel`, p); principal = trocar; break
    case 'routing_selection_unavailable': texto = t(`${F}frase.rota`, p); break
    case 'engine_choice_execution_mismatch': texto = t(`${F}frase.divergente`, p); principal = trocar; break
    case 'engine_not_ready': {
      // Motor fora dos gerenciados (ex.: omp descontinuado): conectar conta não resolve — troca-se o motor do bot.
      const semMotorGerenciado = reason.backend !== undefined && !isEngine(reason.backend)
      texto = t(semMotorGerenciado ? `${F}frase.motorIndisponivel` : `${F}frase.motorSemPreparo`, p)
      principal = semMotorGerenciado ? { acao: 'bot-motor', rotulo: t(`${F}acao.trocarMotor`) } : { acao: 'contas', rotulo: t(`${F}acao.conectar`) }
      break
    }
    // MEX-1391/F6-B: nenhuma conta da fila pode receber o pedido; o botão permite uso extra até o reset (o Hub confere).
    case 'quota_near_limit': texto = t(`${F}frase.quotaPerto`, p); principal = { acao: 'uso-extra', rotulo: t(`${F}acao.usoExtra`) }; break
    case 'quota_exhausted': texto = t(`${F}frase.quotaEsgotada`, p); principal = abrir; break
    case 'fallback_no_proof': texto = t(`${F}frase.semProva`); principal = abrir; break
    case 'folder_unavailable': texto = t(`${F}frase.pasta`, p); principal = { acao: 'bot-pasta', rotulo: t(`${F}acao.escolherPasta`) }; break
    default: texto = t(`${F}frase.generico`, p)
  }

  // Qual conta o bot usa: só o que a lista confirma. Sem `accountId` = acesso do próprio Hub.
  // A linha só entra quando a frase não disse a conta (senão repetiria).
  const contaDoBot = conta ? (texto.includes(conta.label) ? '' : t(`${F}contaDoBot`, { motor, conta: conta.label, estado: estadoDaContaFalha(t, conta.state) }))
    : !reason.accountId && reason.code !== 'folder_unavailable' ? t(`${F}semConta`) : ''
  const contasConectadas = !contas ? t(`${F}contasDesconhecidas`)
    : contas.some((c) => c.state === 'connected') ? t(`${F}contasConectadas`, { lista: listaDeContas(idioma, contas) })
    : t(`${F}nenhumaConectada`)
  const botoes = principal === abrir ? [abrir] : [principal, abrir]
  return { texto, contaDoBot, contasConectadas, botoes }
}

/** O resumo de uso da linha da conta e se alguma janela está perto do limite (o Hub decide: `nearLimit`, >= 90%). */
export interface ResumoUso { texto: string; perto: boolean }

/**
 * MEX-1294/F6-A: o resumo de uso da linha da conta — "62% da sessão, 40% da semana, volta sex 14:00"; com alguma janela
 * perto do limite, "Perto do limite: 92% da sessão, …". Só janelas `session`/`week` com origem comprovada; sem leitura, a
 * linha fica como estava (nunca 0%). O "volta" é o da janela perto do limite (a mais cheia, se duas), senão o da sessão,
 * senão o da semana. O texto cru do provedor nunca entra: o Hub manda o instante e a hora sai no idioma do app.
 */
export function resumoUso(usage: AccountUsage | null | 'erro', t: Tradutor, idioma: AppLanguage, now = Date.now()): ResumoUso | null {
  if (!usage || usage === 'erro' || usage.state !== 'available' || !usage.source) return null
  const sessao = usage.windows.find((w) => w.id === 'session')
  const semana = usage.windows.find((w) => w.id === 'week')
  const pct = (w: { percent: number }) => formatarNumero(idioma, Math.round(Math.max(0, Math.min(100, w.percent))))
  const partes: string[] = []
  if (sessao) partes.push(t('contas.usoResumoSessao', { pct: isolado(pct(sessao)) }))
  if (semana) partes.push(t('contas.usoResumoSemana', { pct: isolado(pct(semana)) }))
  if (!partes.length) return null
  const perto = [sessao, semana].filter((w): w is NonNullable<typeof w> => !!w && w.nearLimit === true).sort((a, b) => b.percent - a.percent)
  const reset = (perto[0] ?? sessao)?.resetsAt ?? sessao?.resetsAt ?? semana?.resetsAt ?? null
  if (reset) partes.push(t('contas.usoResumoVolta', { quando: isolado(whenText(reset, idioma, now)) }))
  const corpo = partes.join(t('contas.usoResumoSeparador'))
  return { texto: perto.length ? t('contas.usoResumoPerto', { resumo: corpo }) : corpo, perto: perto.length > 0 }
}

/** A linha "Conta: …" do cabeçalho da conversa e da configuração do bot. `null` = ainda não dá para dizer (lista a caminho). */
export function linhaDaConta(
  { t, contas, accountId, backend, falhou, usoResumo }: { t: Tradutor; contas: readonly ContaFalha[] | null; accountId: string | null; backend: string | null; falhou?: boolean; usoResumo?: ResumoUso | null },
): { texto: string; ok: boolean; dica?: string; perto?: boolean; base?: string; uso?: string } | null {
  const C = 'conversa.avisos.falha.cabecalho.' as const
  if (!accountId) return { texto: t(`${C}sem`), ok: true, dica: t(`${C}semDica`) }
  if (!contas) return falhou ? { texto: t(`${C}naoSei`), ok: false } : null
  const conta = contas.find((c) => c.id === accountId)
  if (!conta) return { texto: t(`${C}naoSei`), ok: false }
  const texto = t(`${C}conta`, { conta: conta.label, motor: nomeDoMotor(conta.backend || backend || undefined, t), estado: estadoDaContaFalha(t, conta.state) })
  const ok = conta.state === 'connected'
  // Conta caída vale mais que o uso: sem conexão o resumo não aparece (a leitura nem é pedida).
  // `base` e `uso` separados: na faixa estreita da conversa a base encolhe primeiro e o "volta …" fica sempre à vista.
  if (!usoResumo || !ok) return { texto, ok }
  return { texto: `${texto} · ${usoResumo.texto}`, ok, base: texto, uso: usoResumo.texto, ...(usoResumo.perto ? { perto: true } : {}) }
}
