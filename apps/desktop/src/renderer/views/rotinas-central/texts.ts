// Textos da tela central "Rotinas" (D85). DON-917: vêm do catálogo (fork-i18n, módulo `rotinas`) no idioma do app,
// lido a cada uso: a tela assina o idioma (`useIdioma` em RoutinesCentralView) e re-renderiza quando ele muda.
import type { AppLanguage } from '../../../shared/fork-idioma.ts'
import { localeDe } from '../../lib/fork-i18n/index.ts'
import { idiomaAtual, tradutor, type Tradutor } from '../../lib/fork-i18n/estado'
import type { CalendarView } from '../../lib/routine-calendar'

/** `t` no idioma de agora (lido a cada chamada); `tradutor` sozinho fixaria o idioma de uma vez. */
const t = Object.assign((...args: unknown[]) => (tradutor(idiomaAtual()) as unknown as (...a: unknown[]) => string)(...args), {
  plural: (...args: unknown[]) => (tradutor(idiomaAtual()).plural as unknown as (...a: unknown[]) => string)(...args),
}) as unknown as Tradutor
const VIEWS: CalendarView[] = ['day', 'week', 'month']
const porVisao = (prefixo: 'rotinas.central.anterior' | 'rotinas.central.proximo') =>
  Object.fromEntries(VIEWS.map((v) => [v, t(`${prefixo}.${v}`)])) as Record<CalendarView, string>

/** Texto do usuário (nome, busca) dentro de uma frase: isolado (FSI…PDI), com a direção dele. */
const isolado = (texto: string) => `\u2068${texto}\u2069`

/** O idioma de agora, para os formatadores de data (lib/routine-calendar) chamados pela tela. */
export const idioma = (): AppLanguage => idiomaAtual()

export const texts = {
  get title() { return t('rotinas.central.titulo') },
  get back() { return t('rotinas.central.voltar') },
  get open() { return t('rotinas.central.abrir') },
  get newRoutine() { return t('rotinas.central.nova') },
  get newRoutineTitle() { return t('rotinas.central.novaRotina') },
  get chooseBot() { return t('rotinas.central.escolhaBot') },
  chooseBotAt: (quando: string) => t('rotinas.central.escolhaBotEm', { quando }),
  forBot: (nome: string) => t('rotinas.central.paraBot', { nome: isolado(nome) }),
  get noAgentBots() { return t('rotinas.central.semBotsAgentes') },
  get modeLabel() { return t('rotinas.central.modo') },
  get searchRoutines() { return t('rotinas.central.buscar') },
  get searchRoutinesLabel() { return t('rotinas.central.buscarRotulo') },
  get noResults() { return t('rotinas.central.semResultado') },
  noResultsFor: (busca: string) => t('rotinas.central.semResultadoPara', { busca: isolado(busca) }),
  get filters() { return t('rotinas.central.filtros') },
  get list() { return t('rotinas.central.lista') },
  get calendar() { return t('rotinas.central.calendario') },
  get allBots() { return t('rotinas.central.todosBots') },
  get today() { return t('rotinas.central.hoje') },
  todayOn: (data: string) => t('rotinas.central.hojeEm', { data }),
  get viewLabel() { return t('rotinas.central.periodo') },
  get views(): { id: CalendarView; label: string }[] { return VIEWS.map((id) => ({ id, label: t(`rotinas.central.visao.${id}`) })) },
  get previous() { return porVisao('rotinas.central.anterior') },
  get next() { return porVisao('rotinas.central.proximo') },
  more: (n: number) => t('rotinas.central.mais', { n }),
  moreLabel: (n: number, dia: string) => t.plural('rotinas.central.maisEm', n, { dia }),
  routineCount: (n: number) => t.plural('rotinas.central.contagem', n),
  botCount: (n: number) => t.plural('rotinas.central.contagemBots', n),
  openDay: (dia: string) => t('rotinas.central.abrirDia', { dia }),
  get onDemand() { return t('rotinas.central.sobDemanda') },
  get onDemandHint() { return t('rotinas.central.sobDemandaDica') },
  get frequent() { return t('rotinas.central.frequentes') },
  get frequentHint() { return t('rotinas.central.frequentesDica') },
  /** Os nomes entre aspas, numa lista do idioma ("“A”, “B” e “C”"), e a frase inteira no plural certo. */
  truncated: (names: string[]) => {
    const atual = idiomaAtual()
    const nomes = new Intl.ListFormat(localeDe(atual), { type: 'conjunction' }).format(names.map((nome) => t('rotinas.central.nomeCitado', { nome: isolado(nome) })))
    return tradutor(atual).plural('rotinas.central.foraDaGrade', names.length, { nomes })
  },
  get timeGrid() { return t('rotinas.central.grade') },
  get timeGridHelp() { return t('rotinas.central.gradeAjuda') },
  get monthGridHelp() { return t('rotinas.central.mesAjuda') },
  createAt: (quando: string) => t('rotinas.central.criarEm', { quando }),
  /** "quinta-feira, 29 de outubro, 09:00": dia e hora juntos, na ordem do idioma. */
  dayTime: (dia: string, hora: string) => t('rotinas.central.diaHora', { dia, hora }),
  get monthGrid() { return t('rotinas.central.gradeMes') },
  // Barra lateral.
  get sidebar() { return t('rotinas.central.lateral') },
  get showSidebar() { return t('rotinas.central.mostrarLateral') },
  get hideSidebar() { return t('rotinas.central.fecharLateral') },
  get miniCalendar() { return t('rotinas.central.miniCalendario') },
  get previousMonth() { return t('rotinas.central.anterior.month') },
  get nextMonth() { return t('rotinas.central.proximo.month') },
  get myBots() { return t('rotinas.central.bots') },
  get searchBots() { return t('rotinas.central.buscarBots') },
  get clearSearch() { return t('rotinas.central.limparBusca') },
  get botList() { return t('rotinas.central.listaBots') },
  dragBot: (nome: string) => t('rotinas.central.arrasteBot', { nome: isolado(nome) }),
  onlyBot: (nome: string) => t('rotinas.central.soDoBot', { nome: isolado(nome) }),
  get schedule() { return t('rotinas.central.agendar') },
  /** `when`: a próxima hora cheia (`null`) ou um dia e hora. */
  scheduleBot: (nome: string, when: { dia: string; hora: string } | null) =>
    when ? t('rotinas.central.agendarEm', { nome: isolado(nome), ...when }) : t('rotinas.central.agendarProximaHora', { nome: isolado(nome) }),
  get noBots() { return t('rotinas.central.semBots') },
  get noMatch() { return t('rotinas.central.nenhumBot') },
  get dragHint() { return t('rotinas.central.arrasteDica') },
  // Estados.
  get loading() { return t('rotinas.central.carregando') },
  get loadError() { return t('rotinas.central.erroCarregar') },
  get oldHub() { return t('rotinas.central.hubAntigo') },
  get noHub() { return t('rotinas.central.semHub') },
  get retry() { return t('rotinas.central.tentarDeNovo') },
  get emptyTitle() { return t('rotinas.central.vazioTitulo') },
  get emptyBody() { return t('rotinas.central.vazioTexto') },
  get emptyCalendar() { return t('rotinas.central.vazioCalendario') },
  get filteredEmpty() { return t('rotinas.central.vazioFiltros') },
  get clearFilters() { return t('rotinas.central.limparFiltros') },
  // Lista.
  get sections() { return { scheduled: t('rotinas.central.secao.scheduled'), manual: t('rotinas.central.secao.manual'), onDemand: t('rotinas.central.secao.onDemand'), paused: t('rotinas.central.secao.paused') } },
  get manual() { return t('rotinas.central.manual') },
  get manualHint() { return t('rotinas.central.manualDica') },
  runNow: (nome: string) => t('rotinas.central.rodarAgoraNome', { nome: isolado(nome) }),
  nextRun: (quando: string) => t('rotinas.central.proxima', { quando }),
  dayAt: (dia: string, hora: string) => t('rotinas.central.diaAs', { dia, hora }),
  get noNextRun() { return t('rotinas.central.semProxima') },
  get lastFailed() { return t('rotinas.central.ultimaFalhou') },
  get lastNeedsOwner() { return t('rotinas.central.ultimaPrecisa') },
  get lastUnconfirmed() { return t('rotinas.central.ultimaSemConfirmacao') },
  get needsAttention() { return t('rotinas.central.atencao') },
  toggle: (nome: string) => t('rotinas.central.alternar', { nome: isolado(nome) }),
  get paused() { return t('rotinas.central.pausada') },
  get awaitingRelease() { return t('rotinas.central.aguardaLiberacao') },
  get globalPaused() { return t('rotinas.central.suspensa') },
  get pauseUnknown() { return t('rotinas.central.pausaIndisponivel') },
  get event() { return t('rotinas.central.evento') },
  get webhook() { return t('rotinas.central.webhook') },
  get deletedBot() { return t('rotinas.central.botApagado') },
  get bot() { return t('rotinas.central.bot') },
  get localZone() { return t('rotinas.central.fusoLocal') },
  /** O estado de uma execução em andamento, ou `null` para outro estado. */
  status: (estado: string): string | null =>
    ['pending', 'starting', 'running', 'recovering', 'failed', 'interrupted', 'cancelled'].includes(estado)
      ? t(`rotinas.estado.${estado as 'pending'}`)
      : null,
  /** Nome, bot, dia e hora de um item, para leitores de tela. */
  item: (nome: string, bot: string, dia: string, hora: string) => t('rotinas.central.item', { nome: isolado(nome), bot: isolado(bot), dia, hora }),
  // D84: "Onde roda" (aparece quando existirem os computadores da D83).
  runsOn: (onde: string) => t('rotinas.central.rodaEm', { onde }),
  runsOnLabel: (onde: string) => t('rotinas.central.rodaEmRotulo', { onde }),
  get hub() { return t('rotinas.central.hub') },
  get close() { return t('rotinas.editor.fechar') },
}
