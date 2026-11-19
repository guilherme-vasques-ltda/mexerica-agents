// DON-906 (C2, D378): a barra de contexto, fina, abaixo da caixa do composer e alinhada à direita, sob o microfone e
// Enviar. Anel pequeno com o percentual ao lado;
// o balão (clique ou teclado) diz, em linguagem de quem conversa, quanto da conversa está ocupada (barra com a marca de
// onde a Mexerica resume sozinha), o que Comprimir agora faz e o desfecho; números e custo ficam em Ver detalhes.
// Sem cartão na conversa. Sem leitura válida do Hub (size ausente), nada aparece; Hub antigo (sem donnaContext) avisa.
import { useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore, type CSSProperties, type ReactNode } from 'react'
import { isWorkingIn, type Bot } from '@shared/models'
import { useModel } from '../../../lib/observable'
import { usePref } from '../../../lib/prefs'
import { conversaPrefs } from '../conversa/preferencias'
import { useStore } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { avisoResultado, avisoSemSuporte, INTERVALO_MS, ModeloContexto, porcento, resumidaEm, tokensCurtos, vistaBalao, type Aviso } from './estado'
import { Icon } from '../../../components/Icon'
import { Spinner } from '../../../components/Controls'
import { CustoConversa } from '../custo/CustoConversa'
import { AnelContexto } from '../compositor/AnelContexto'
import '../idioma/escritas.css'
import { AvisoBalao } from './AvisoBalao'
import './contexto.css'

/** As chaves de aviso e de "resumida" são montadas pelo código (`contexto.aviso.<id>.*`); o teste de catálogo confere todas. */
const livre = (t: Tradutor) => t as unknown as (chave: string, vars?: Record<string, string>) => string

const modelos = new WeakMap<BotStore, ModeloContexto>()
/** DON-907 C9: o mesmo modelo serve ao seletor de modelo (`usoDaConversa`), sem segunda leitura. */
export const modeloDe = (store: BotStore) => {
  let m = modelos.get(store)
  if (!m) {
    m = new ModeloContexto(() => {
      const client = store.connectedClient
      const hostId = store.hello?.hostId
      return client && hostId ? { client, hostId, epoch: store.principalEpoch, spaceId: client.spaceId ?? null } : null
    })
    modelos.set(store, m)
  }
  return m
}

/** O turno da conversa está aberto (como o Parar do composer): comprimir agora espera. */
const emTurno = (bot: Bot | null, chatId: string) => !!bot && (isWorkingIn(bot, chatId) || (bot.status === 'waiting' && (bot.workingChat ?? bot.id) === chatId))

const visivel = () => document.visibilityState === 'visible'
const seguirVisibilidade = (f: () => void) => { document.addEventListener('visibilitychange', f); return () => document.removeEventListener('visibilitychange', f) }

/**
 * `sessionId`: null = a principal; uma string = exatamente aquela sessão extra (nunca inferida do bot nem de outra
 * conversa). O DestinationComposer da UI01 repassa a sessão selecionada na aba visível.
 */
export function BarraContexto({ botId, sessionId, inicio = null }: {
  botId: string
  sessionId: string | null
  /** D473: o chip do modelo e a mãozinha das aprovações, à esquerda da mesma linha fina (o contexto fica à direita). */
  inicio?: ReactNode
}) {
  const store = useStore()
  const modelo = useModel(modeloDe(store))
  // Superfície traduzida (DON-903): textos, números e hora no idioma escolhido.
  const idioma = useIdioma()
  const t = useT()
  const superficie = superficieIdioma(idioma)
  // Ajustes → Conversa → Contexto; desligado, nada aparece e nada é pedido.
  const [ligado] = usePref(conversaPrefs.contextMeter)
  const bot = store.bots.get(botId) ?? null
  const online = store.connection.kind === 'online'
  const working = emTurno(bot, botId)
  const naTela = useSyncExternalStore(seguirVisibilidade, visivel)
  const origem = `${store.hello?.hostId ?? ''}|${store.principalEpoch}|${store.connectedClient ? 1 : 0}`
  const suportado = modelo.suportado()
  const leitura = online ? modelo.of(botId, sessionId) : null
  const enviando = modelo.enviando(botId, sessionId)
  // Pedido enviado ou compressão aceita sem desfecho: o botão fica desligado até terminar.
  const emAndamento = online && modelo.emAndamento(botId, sessionId)
  const resultado = online ? modelo.resultado(botId, sessionId) : null
  const recusa = online ? modelo.recusa(botId, sessionId) : null

  useEffect(() => () => modelo.descartar(botId, sessionId), [modelo, botId, sessionId, origem, store.connectedClient, store.connectedClient?.spaceId])

  // Lê já e a cada 2 s, só montado, online, com a janela visível e se o Hub tem o RPC.
  useEffect(() => {
    if (!ligado || !online || !naTela || !suportado) return
    void modelo.ler(botId, sessionId)
    const t = setInterval(() => void modelo.ler(botId, sessionId), INTERVALO_MS)
    return () => clearInterval(t)
  }, [modelo, botId, sessionId, origem, ligado, online, naTela, suportado])

  // Pulso uma vez quando chega uma compressão nova (não na primeira leitura nem ao trocar de conversa).
  const visto = useRef<{ chave: string; at: number | null } | null>(null)
  const [pulso, setPulso] = useState(false)
  const chave = JSON.stringify([origem, botId, sessionId])
  const ultima = leitura?.lastCompaction?.at ?? null
  useEffect(() => {
    if (!leitura) return
    const antes = visto.current
    visto.current = { chave, at: ultima }
    if (antes && antes.chave === chave && ultima !== null && (antes.at === null || ultima > antes.at)) {
      setPulso(true)
      const t = setTimeout(() => setPulso(false), 900)
      return () => clearTimeout(t)
    }
  }, [chave, ultima, !!leitura])

  const [fixo, setFixo] = useState(false)
  const [detalhes, setDetalhes] = useState(false)
  const botao = useRef<HTMLButtonElement>(null)
  const balaoId = useId()
  const motivoId = useId()
  const detalhesId = useId()
  // D378: o balão abre acima da caixa inteira do composer (irmã anterior da barra), sem cobrir o campo nem Enviar.
  // Mede a caixa real enquanto aberto: ela cresce com várias linhas, anexos e resposta citada.
  const barra = useRef<HTMLDivElement>(null)
  const medidor = useRef<HTMLDivElement>(null)
  const [posicao, setPosicao] = useState<CSSProperties | undefined>(undefined)
  useLayoutEffect(() => {
    const caixa = barra.current?.previousElementSibling
    const m = medidor.current
    if (!fixo || !caixa || !m) return
    const medir = () => {
      const topo = (caixa.querySelector('.composer-surface') ?? caixa).getBoundingClientRect().top
      const acima = m.getBoundingClientRect().top - topo
      setPosicao({ insetBlockEnd: `calc(100% + ${Math.round(acima)}px + var(--sp-4))`, maxBlockSize: `${Math.max(120, Math.floor(topo - 16))}px` })
    }
    medir()
    const observador = new ResizeObserver(medir)
    observador.observe(caixa)
    window.addEventListener('resize', medir)
    return () => { observador.disconnect(); window.removeEventListener('resize', medir) }
  }, [fixo])

  // Hub antigo (sem `donnaContext`): o anel fica, sem número, e o balão diz o que falta (em vez de sumir).
  const semSuporte = ligado && online && !leitura && modelo.hubAntigo()
  // Sem medida nova (o Hub a omite até a próxima resposta), o desfecho aparece sozinho na barra.
  if (ligado && !leitura && resultado && !semSuporte) {
    const a = avisoResultado(idioma, resultado, null)
    return (
      <div className="ctx-barra idioma-superficie" {...superficie}>
        {inicio ? <div className="ctx-inicio">{inicio}</div> : null}
        <div className={`ctx-linha ctx-status t-${a.tom}`} role="status">{livre(t)(`contexto.aviso.${a.id}.titulo`)}. {livre(t)(`contexto.aviso.${a.id}.detalhe`)}</div>
      </div>
    )
  }
  if (!ligado || (!leitura && !semSuporte)) return inicio ? <div className="ctx-barra idioma-superficie" {...superficie}><div className="ctx-inicio">{inicio}</div></div> : null
  const vista = leitura ? vistaBalao({ idioma, leitura, online, working, enviando, emAndamento, recusa, resultado }) : null
  const pct = vista?.pct ?? 0
  const alto = vista?.alto ?? false
  const aviso: Aviso | null = vista ? vista.aviso : avisoSemSuporte()
  const mostrar = fixo
  const pctTexto = porcento(idioma, pct)
  const ariaAnel = semSuporte ? t('contexto.anel.ariaSemSuporte')
    : t(leitura?.automaticPaused ? 'contexto.anel.ariaPausado' : 'contexto.anel.aria', { pct: pctTexto })
  // O título "Conversa 13% cheia" com só o número em negrito (ele aparece uma vez na frase, em qualquer idioma).
  const [antesPct, depoisPct] = t('contexto.balao.titulo', { pct: pctTexto }).split(pctTexto)
  const resumida = leitura ? resumidaEm(idioma, leitura.lastCompaction, Date.now()) : null

  // D533: mesma leitura e botão; apenas o desenho do anel ganha os gomos.
  return (
    <div className="ctx-barra idioma-superficie" {...superficie} ref={barra}>
    {inicio ? <div className="ctx-inicio">{inicio}</div> : null}
    <div ref={medidor} className={`ctx-medidor${alto ? ' is-alto' : ''}${leitura?.automaticPaused || semSuporte ? ' is-pausado' : ''}${pulso ? ' is-pulso' : ''}`}
      onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) { setFixo(false) } }}
      onKeyDown={(e) => {
        if (e.key === 'Escape' && mostrar) { e.stopPropagation(); setFixo(false); botao.current?.focus() }
      }}>
      <button ref={botao} type="button" className="press ctx-anel idioma-superficie" {...superficie} aria-label={ariaAnel}
        aria-expanded={mostrar} aria-controls={balaoId} onClick={() => setFixo((f) => !f)}>
        <AnelContexto percentual={pct} />
        <span className="ctx-pct" aria-hidden="true">{semSuporte ? '–' : pctTexto}</span>
      </button>
      {/* O balão se posiciona pela caixa do composer; conteúdo e barra seguem a direção do idioma. */}
      <div id={balaoId} className={`ctx-balao${mostrar ? ' shown' : ''}`} style={posicao} hidden={!mostrar}>
        <div className="ctx-conteudo idioma-superficie" {...superficie} role="group" aria-label={t('contexto.balao.aria')}>
          {leitura && vista ? (
            <>
              <h3 className="ctx-cabeca">
                {vista.pronto ? <span className="ctx-ok"><Icon name="checkmark" size={11} weight="bold" /></span> : null}
                <span>{antesPct}<b className="ctx-valor">{pctTexto}</b>{depoisPct}</span>
              </h3>
              <div className={`ctx-medida${alto ? ' is-alto' : ''}`} role="meter" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label={t('contexto.medida.aria')}>
                {vista.antesPct !== null ? <span className="ctx-antes" style={{ inlineSize: `${vista.antesPct}%` }} /> : null}
                <span className="ctx-fill" style={{ inlineSize: `${pct}%` }} />
                {vista.marca !== null ? <span className="ctx-marca" style={{ insetInlineStart: `${vista.marca}%` }} /> : null}
                {vista.botao === 'comprimindo' ? <span className="ctx-onda" /> : null}
              </div>
              <p className="ctx-estado">
                <span className="ctx-estado-frase">{t(`contexto.estado.${vista.faixa}`)}</span>
                {vista.detalhe === 'marca' && vista.marca !== null ? ` ${t('contexto.estado.marca', { marca: porcento(idioma, vista.marca) })}`
                  : vista.detalhe === 'quase' ? ` ${t('contexto.estado.quaseDetalhe')}` : null}
              </p>
              <div className="ctx-acao">
                <button type="button" className="styled-button secondary ctx-comprimir" disabled={vista.desligado}
                  aria-describedby={motivoId}
                  onClick={() => void modelo.comprimir(botId, sessionId, leitura.contextId)}>
                  {vista.botao === 'comprimindo' ? <span aria-hidden="true" className="ctx-giro"><Spinner size={13} /></span> : vista.botao === 'tentar' ? <Icon name="arrow.clockwise" size={12} weight="semibold" /> : null}
                  {t(vista.botao === 'comprimindo' ? 'contexto.comprimindo' : vista.botao === 'tentar' ? 'contexto.ajustes.tentarDeNovo' : 'contexto.comprimir')}
                </button>
                {/* Todo clique tem retorno visível, e o motivo de estar desligado é texto, nunca só opacidade. */}
                {aviso ? <AvisoBalao id={motivoId} aviso={aviso} t={t} /> : <p id={motivoId} className="ctx-ajuda">{t('contexto.ajuda')}</p>}
              </div>
              <div className="ctx-rodape">
                <span>{resumida ? livre(t)(`contexto.resumida.${resumida.id}`, resumida.vars) : null}</span>
                <button type="button" className="ctx-detalhes" aria-expanded={detalhes} aria-controls={detalhesId} onClick={() => setDetalhes((d) => !d)}>
                  {t(detalhes ? 'contexto.detalhes.ocultar' : 'contexto.detalhes.ver')}
                  <Icon className={detalhes ? 'gira' : undefined} name="chevron.down" size={10} weight="semibold" />
                </button>
              </div>
              {detalhes ? (
                <div id={detalhesId} className="ctx-det">
                  <dl>
                    <div><dt>{t('contexto.detalhes.espaco')}</dt><dd>{t('contexto.detalhes.espacoValor', { usado: tokensCurtos(idioma, leitura.used), total: tokensCurtos(idioma, leitura.size) })}</dd></div>
                    <div><dt>{t('contexto.detalhes.gatilho')}</dt><dd>{t('contexto.detalhes.gatilhoValor', { gatilho: tokensCurtos(idioma, leitura.trigger) })}</dd></div>
                  </dl>
                  <CustoConversa botId={botId} sessionId={sessionId} aberto={mostrar && detalhes} />
                  <p className="ctx-nota-glossario">{t('contexto.detalhes.glossario')}</p>
                </div>
              ) : null}
            </>
          ) : (
            <>
              <h3 className="ctx-cabeca"><span>{t('contexto.balao.aria')}</span></h3>
              {aviso ? <AvisoBalao aviso={aviso} t={t} /> : null}
            </>
          )}
        </div>
      </div>
    </div>
    </div>
  )
}
