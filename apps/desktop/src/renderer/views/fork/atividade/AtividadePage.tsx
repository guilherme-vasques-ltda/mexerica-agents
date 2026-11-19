// Entrega 4 (MX-7): Ajustes → Atividade. O que os trabalhos de fundo fizeram, em linhas compactas agrupadas por dia;
// clicar numa linha abre os detalhes (modelos, tokens, etapa da falha). O Hub guarda no máximo 50 registros; decisão
// do dono (11/11): páginas numeradas de 10, com o filtro voltando à página 1. Saiu do Bot de fundo (antes: "Atividade recente").
import { useEffect, useMemo, useSyncExternalStore, type CSSProperties, type KeyboardEvent } from 'react'
import { CardForm, CardSection, ChoicePicker, IconButton, Spinner, TabChoice, ValueRow } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Text } from '../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { formatarData, formatarNumero } from '../../../lib/fork-i18n/index.ts'
import { useModel } from '../../../lib/observable'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import type { HistoryEntry } from '../bot-de-fundo/model'
import { Atividade, filtrar, paginaPorTecla, paginar, porDia, resumo, tokens, type Filtro } from './modelo'
import { ltr } from '../../../../shared/bidi.ts'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import './atividade.css'

export function AtividadePage({ openBackground }: { openBackground: () => void }) {
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const [computer, setComputer] = useEstadoDaPagina('atividade/computador', () => app.selectedStore?.computer.id ?? app.local?.computer.id ?? app.computers[0]?.id ?? '')
  const host = computer ? app.store(computer) : null
  return <div className="atividade-page idioma-superficie" {...superficieIdioma(idioma)}>
    <CardForm>
      <div className="atividade-sobre"><Text tone="secondary">{t('fundo.atividade.lead')}</Text><button type="button" className="ajustes-link" onClick={openBackground}>{t('fundo.atividade.abrirFundo')}</button></div>
      {app.computers.length > 1 && <CardSection><ValueRow label={t('fundo.computador')}><ChoicePicker selection={computer} options={app.computers.map((c) => ({ id: c.id, label: c.name }))} onChange={setComputer} language={superficieIdioma(idioma)} /></ValueRow></CardSection>}
      {host ? <Lista key={computer} source={host} /> : <Text tone="secondary">{t('fundo.atividade.semConexao')}</Text>}
    </CardForm>
  </div>
}

function Lista({ source }: { source: BotStore }) {
  const host = useModel(source)
  const t = useT()
  const idioma = useIdioma()
  const client = host.connectedClient
  const modelo = useMemo(() => new Atividade(() => host.connectedClient), [host, client])
  const estado = useSyncExternalStore(modelo.subscribe, modelo.getSnapshot)
  // Filtro (preferência de visualização, global) e, por Hub, o item aberto e a página: itens de um Hub não
  // abrem nem paginam o outro (estado-pagina.ts).
  const [filtro, setFiltro] = useEstadoDaPagina<Filtro>('atividade/filtro', 'todas')
  const [aberta, setAberta] = useEstadoDaPagina<string | null>(`atividade/${host.computer.id}/aberta`, null)
  const [pedida, setPagina] = useEstadoDaPagina(`atividade/${host.computer.id}/pagina`, 1)
  useEffect(() => () => modelo.fechar(), [modelo])
  useEffect(() => { void modelo.ler() }, [modelo, host.backgroundEpoch])
  const todas = estado.entradas ?? []  // já do mais novo ao mais antigo (modelo.ts)
  const r = resumo(todas)
  const lista = filtrar(todas, filtro)
  const pag = paginar(lista, pedida)
  // O filtro reinicia na página 1 (decisão do dono, 11/11).
  const trocarFiltro = (f: Filtro) => { setFiltro(f); setPagina(1); setAberta(null) }
  const irPara = (n: number) => { setPagina(n); setAberta(null) }
  const titulo = estado.entradas ? t('fundo.atividade.resumo', { total: formatarNumero(idioma, r.total), falhas: formatarNumero(idioma, r.falhas) }) : t('fundo.atividade.titulo')
  const acoes = <>
    {estado.entradas && todas.length > 0 && <TabChoice label={t('fundo.atividade.filtro')} selection={filtro} options={[{ id: 'todas', label: t('fundo.atividade.filtro.todas') }, { id: 'falhas', label: t('fundo.atividade.filtro.falhas') }]} onChange={trocarFiltro} />}
    {client && (estado.lendo ? <Spinner size={12} /> : <IconButton title={t('fundo.atividade.atualizar')} icon="arrow.clockwise" onClick={() => void modelo.ler()} />)}
  </>
  const vazio = (icone: string, texto: string) => <div className="atividade-vazio" role="status"><Icon name={icone} size={18} /><span>{texto}</span></div>
  return <section className="card-section atividade-secao">
    <div className="card-section-title"><Text as="h3" size="body" weight="semibold" tone="text">{titulo}</Text><span className="atividade-acoes">{acoes}</span></div>
    <div className="card-section-rows atividade-caixa" aria-busy={estado.lendo}>
      {/* Releitura que falhou: as linhas da última leitura continuam, com o aviso em cima. */}
      {client && estado.erro && estado.entradas && <p role="alert" className="atividade-aviso"><Icon name="exclamationmark.circle" size={14} /><span>{t(`fundo.atividade.erro.${estado.erro}`)} {t('fundo.atividade.anteriores')}</span></p>}
      {!client ? vazio('exclamationmark.circle', t('fundo.atividade.semConexao'))
        : estado.erro && !estado.entradas ? vazio('exclamationmark.circle', t(`fundo.atividade.erro.${estado.erro}`))
          : !estado.entradas ? vazio('clock', t('fundo.atividade.lendo'))
            : !todas.length ? vazio('clock', t('fundo.atividade.vazio'))
              : !lista.length ? vazio('checkmark.circle', t('fundo.atividade.semFalhas'))
                : <div className="atividade-lista" aria-label={t('fundo.atividade.titulo')} style={relogio12h(idioma) ? { '--atividade-hora': '68px' } as CSSProperties : undefined}>
                  {porDia(pag.itens).map((g) => <div key={String(g.dia)} role="group" aria-label={rotuloDia(g.dia)}>
                    <div className="atividade-dia">{rotuloDia(g.dia)}</div>
                    {g.entradas.map((e, i) => <Linha key={`${e.id}/${i}`} e={e} bot={host.bots.get(e.botId)?.name ?? null} aberta={aberta === `${e.id}/${i}`} alternar={() => setAberta(aberta === `${e.id}/${i}` ? null : `${e.id}/${i}`)} />)}
                  </div>)}
                </div>}
    </div>
    {client && lista.length > 0 && <Paginador pag={pag} irPara={irPara} />}
    {client && todas.length > 0 && <div className="card-section-footer">{t('fundo.atividade.rodape')}{todas.some((e) => e.function === 'daily_summary') && <> {t('fundo.atividade.resumoDia')}</>}</div>}
  </section>

  function rotuloDia(dia: 'hoje' | 'ontem' | number | 'semData') {
    return dia === 'hoje' ? t('fundo.atividade.hoje') : dia === 'ontem' ? t('fundo.atividade.ontem') : dia === 'semData' ? t('fundo.historico.semData') : formatarData(idioma, dia, { weekday: 'short', day: 'numeric', month: 'short' })
  }
}

/** Idiomas com relógio de 12 h ("06:13 AM") precisam de uma coluna de hora mais larga. */
const relogio12h = (idioma: Parameters<typeof formatarData>[0]) => /\D\s*$/.test(formatarData(idioma, new Date(2026, 0, 1, 18, 5), { hour: '2-digit', minute: '2-digit' }))

/** `‹ 1 2 3 4 5 ›` e "X–Y de N". Setas do teclado andam uma página (no RTL, ← avança); Home e End vão às pontas. */
function Paginador({ pag, irPara }: { pag: ReturnType<typeof paginar>; irPara: (n: number) => void }) {
  const t = useT()
  const idioma = useIdioma()
  const rtl = superficieIdioma(idioma).dir === 'rtl'
  const n = (v: number) => formatarNumero(idioma, v)
  const tecla = (e: KeyboardEvent<HTMLElement>) => {
    const alvo = paginaPorTecla(e.key, pag.pagina, pag.paginas, rtl)
    if (alvo === null) return
    e.preventDefault()
    // O React zera `currentTarget` depois do dispatch: guarda o nav antes do quadro seguinte (revisão UI708-01).
    const nav = e.currentTarget
    irPara(alvo)
    requestAnimationFrame(() => nav.querySelector<HTMLElement>(`[data-pagina="${alvo}"]`)?.focus())
  }
  return <div className="atividade-paginas">
    <span className="atividade-intervalo" aria-live="polite">{t('fundo.atividade.intervalo', { de: n(pag.de), ate: n(pag.ate), total: n(pag.total) })}</span>
    {pag.paginas > 1 && <nav aria-label={t('fundo.atividade.paginas')} onKeyDown={tecla}>
      <button type="button" className="atividade-pagina seta" aria-label={t('fundo.atividade.anterior')} disabled={pag.pagina === 1} onClick={() => irPara(pag.pagina - 1)}><Icon name="chevron.left" size={10} weight="semibold" /></button>
      {Array.from({ length: pag.paginas }, (_, i) => i + 1).map((p) => <button key={p} type="button" data-pagina={p} className={`atividade-pagina${p === pag.pagina ? ' atual' : ''}`} aria-label={t('fundo.atividade.pagina', { n: n(p) })} aria-current={p === pag.pagina ? 'page' : undefined} onClick={() => irPara(p)}>{n(p)}</button>)}
      <button type="button" className="atividade-pagina seta" aria-label={t('fundo.atividade.proxima')} disabled={pag.pagina === pag.paginas} onClick={() => irPara(pag.pagina + 1)}><Icon name="chevron.right" size={10} weight="semibold" /></button>
    </nav>}
  </div>
}

function Linha({ e, bot, aberta, alternar }: { e: HistoryEntry; bot: string | null; aberta: boolean; alternar: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const n = tokens(e)
  const falhou = e.status === 'failed'
  return <div className={`atividade-item${aberta ? ' aberta' : ''}`}>
    <button type="button" className="atividade-linha" aria-expanded={aberta} onClick={alternar}>
      <span className={`atividade-ponto ${falhou ? 'falha' : e.status === 'applied' ? 'ok' : ''}`} aria-hidden="true" />
      <span className="atividade-hora">{e.at > 0 ? <time dateTime={new Date(e.at).toISOString()}>{ltr(formatarData(idioma, e.at, { hour: '2-digit', minute: '2-digit' }))}</time> : '—'}</span>
      <span className="atividade-nome"><strong>{t(`fundo.historico.funcao.${e.function}`)}</strong>{bot && <span><bdi>{bot}</bdi></span>}</span>
      <span className={`atividade-status${falhou ? ' falha' : ''}`}>{t(`fundo.historico.status.${e.status}`)}</span>
      <span className="atividade-tokens">{n === null ? '—' : t('fundo.atividade.tokens', { n: formatarNumero(idioma, n, { notation: 'compact', maximumFractionDigits: 1 }) })}</span>
      <Icon name="chevron.right" size={9} weight="semibold" className="atividade-seta" />
    </button>
    {aberta && <dl className="atividade-detalhe">
      <dt>{t('fundo.historico.modelos')}</dt><dd>{e.effectiveModels.length ? <span dir="ltr">{e.effectiveModels.join(' · ')}</span> : t('fundo.historico.naoInformado')}</dd>
      <dt>{t('fundo.historico.tokens')}</dt><dd>{e.usage ? ([
        ['fundo.historico.entrada', e.usage.inputTokens], ['fundo.historico.saida', e.usage.outputTokens], ['fundo.historico.entradaCache', e.usage.cachedInputTokens], ['fundo.historico.gravacaoCache', e.usage.cachedWriteTokens],
      ] as const).map(([rotulo, valor]) => t('fundo.historico.tokenValor', { rotulo: t(rotulo), valor: formatarNumero(idioma, valor) })).join(' · ') : t('fundo.historico.naoInformados')}</dd>
      {(falhou || e.failureStage) && <><dt>{t('fundo.historico.etapaFalha')}</dt><dd>{e.failureStage ? t(`fundo.historico.etapa.${e.failureStage}`) : t('fundo.historico.naoInformada')}</dd></>}
      {e.function === 'daily_summary' && <><dt /><dd className="atividade-nota">{t('fundo.atividade.resumoDia')}</dd></>}
      <dt>{t('fundo.atividade.quando')}</dt><dd>{e.at > 0 ? formatarData(idioma, e.at, { dateStyle: 'medium', timeStyle: 'short' }) : t('fundo.historico.semData')}</dd>
    </dl>}
  </div>
}
