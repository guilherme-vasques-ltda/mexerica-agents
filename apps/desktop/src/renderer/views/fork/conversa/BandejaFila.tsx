// DON-907 (D431): a bandeja da fila, colada em cima do campo de mensagem e no mesmo bloco dele
// (docs/reference/design/don-907-fila-proposta). Uma linha fina por mensagem, alça para arrastar, seta que recolhe; as ações
// aparecem no hover ou no foco por teclado. Regras em ./fila.ts; o Hub decide (host/src/agent/bot/fork_fila.rs).
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { isWorking, type Bot, type Entry } from '@shared/models'
import { Icon } from '../../../components/Icon'
import { listEqual, useModel, useSelector } from '../../../lib/observable'
import { chatEntriesOf, useStoreRef } from '../../../store/context'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { atalhoDaFila, destinoDoArraste, falhaDaFila, mover, pedidoDaAcao, type MetodoFila, type TeclaFila } from './fila'
import { filaComPedidos, origemDoPedido, pedidoNaFila, posicaoEntreMensagens } from './pedidos'
import './fila.css'
import { activityText } from '../../../lib/fork-atividade'

/** O texto da linha: a mensagem do dono ou, no pedido de outro bot, o texto que o bot mandou. */
const textoDaLinha = (e: Entry): string => (pedidoNaFila(e) ? e.data.botMessage?.text : e.data.text) ?? ''

export interface Edicao { entry: Entry; posicao: number; texto: string }

/**
 * A fila desta conversa, a edição em curso e as ações (cada uma vai ao Hub; erro vira aviso traduzido).
 * `edicaoInicial` é só para teste (abre a fila já editando uma mensagem). MX-214: `bloqueada` (aviso de senha colada aberto) desliga toda ação que envia: Mandar agora, o atalho e salvar a edição.
 */
export function useFila(botId: string, sessionId: string | null, bloqueada = false, edicaoInicial: Edicao | null = null) {
  const store = useStoreRef()
  // MX-4 (C9): a fila acorda com as entradas desta conversa, os nomes (origem dos pedidos) e as
  // sessões/espaços citadas — não com qualquer `changed()` da loja.
  useSelector(store, (s) => [chatEntriesOf(s, botId), s.conversations, ...s.bots.values()], listEqual)
  useModel(store.sessions)
  const t = useT()
  const entries = store.allEntries(botId)
  const itens = useMemo(() => filaComPedidos(entries, botId, sessionId), [entries, botId, sessionId])
  const [edicao, setEdicao] = useState<Edicao | null>(edicaoInicial)
  const [recolhida, setRecolhida] = useState(false)
  // A mensagem em edição saiu da fila (foi mandada, removida em outro aparelho): a edição acaba aqui.
  const agora = edicao ? itens.findIndex((e) => e.id === edicao.entry.id) : -1
  const ativa = edicao && agora >= 0 ? { ...edicao, posicao: agora } : null
  // O CAS do Hub usa a revisão de quando a edição começou; se só a ordem mudou desde então (o texto é o mesmo),
  // vale a revisão atual: reordenar não invalida a edição, mas um texto trocado em outro aparelho sim.
  const revisao = (e: Edicao) => {
    const atual = itens.find((x) => x.id === e.entry.id)
    return atual && textoDaLinha(atual) === textoDaLinha(e.entry) ? atual.rev : e.entry.rev
  }

  const agir = async (method: MetodoFila, entry: Entry, extra: Record<string, unknown> = {}) => {
    const p = pedidoDaAcao(method, botId, entry, extra)
    const error = await store.queuedAction(p.method, p.body)
    if (error) store.setError(t(`conversa.fila.erro.${falhaDaFila(error)}`))
    return !error
  }
  const editar = (posicao: number) => {
    const entry = itens[posicao]
    if (entry) setEdicao({ entry, posicao, texto: textoDaLinha(entry) })
  }
  const mandarAgora = (entry: Entry) => { if (!bloqueada) void agir('queuedSendNow', entry) }
  return {
    itens,
    edicao: ativa,
    recolhida,
    alternar: () => setRecolhida((r) => !r),
    editar,
    editarUltima: () => editar(itens.length - 1),
    texto: (texto: string) => setEdicao((e) => (e ? { ...e, texto } : e)),
    cancelar: () => setEdicao(null),
    bloqueada,
    salvar: async () => {
      if (!ativa || bloqueada) return
      if (await agir('queuedEdit', ativa.entry, { text: ativa.texto, expectedRevision: revisao(ativa) })) setEdicao(null)
    },
    mandarAgora,
    remover: (entry: Entry) => void agir('queuedRemove', entry),
    // O Hub conta só as mensagens do dono: o índice visual (com pedidos no meio) vira o índice entre elas.
    mover: (entry: Entry, para: number) => {
      const position = posicaoEntreMensagens(itens, entry, para)
      if (position !== null) void agir('queuedMove', entry, { position, expectedRevision: entry.rev })
    },
    /** Atalhos do campo vazio (Shift+Enter manda a 1ª, ↑ edita a última). Devolve se a tecla foi usada. */
    tecla(e: TeclaFila, campoVazio: boolean): boolean {
      const atalho = ativa || bloqueada ? null : atalhoDaFila(e, campoVazio, itens.length)
      if (atalho === 'mandarPrimeira') mandarAgora(itens[0]!)
      else if (atalho === 'editarUltima') editar(itens.length - 1)
      return atalho !== null
    },
  }
}
export type Fila = ReturnType<typeof useFila>

/** "Donna está respondendo…", leve, acima do bloco, só enquanto há fila. */
export function StatusDaFila({ fila, bot }: { fila: Fila; bot: Bot | null }) {
  const t = useT()
  const idioma = useIdioma()
  if (!bot || !fila.itens.length || !isWorking(bot)) return null
  return (
    <div className="fila-status" role="status">
      <span className="fila-status-pilula">
      <span className="fila-status-ponto" aria-hidden="true" />
      <span className="fila-status-texto">{t('conversa.fila.respondendo', { nome: bot.name })}{activityText(t, idioma, bot) ? ` · ${activityText(t, idioma, bot)}` : ''}</span>
      </span>
    </div>
  )
}

export function BandejaFila({ fila }: { fila: Fila }) {
  const t = useT()
  const store = useStoreRef()
  // MX-4 (C9): a bandeja desenha nomes de bot, títulos de sessão e de conversa dos pedidos.
  useSelector(store, (s) => [s.conversations, ...s.bots.values()], listEqual)
  useModel(store.sessions)
  const [arraste, setArraste] = useState<{ de: number; para: number } | null>(null)
  const linhas = useRef<(HTMLDivElement | null)[]>([])
  const centros = useRef<number[]>([])
  // A lista rola quando passa de 3 itens: o arraste auto-rola na borda e a edição traz a linha à vista.
  const caixa = useRef<HTMLDivElement | null>(null)
  const ponteiro = useRef<number | null>(null)
  const arrastando = useRef(false)
  const quadro = useRef(0)
  const { itens } = fila
  const editandoId = fila.edicao?.entry.id
  const editandoPos = fila.edicao?.posicao
  // A lista não mostra barra de rolagem (o app as zera no theme.css): os fades das bordas são o aviso
  // de que há mais linhas — embaixo sempre que passa de 3, em cima depois de rolar.
  const [rolagem, setRolagem] = useState({ topo: false, fundo: false })
  const medirRolagem = () => {
    const box = caixa.current
    if (!box) return
    const topo = box.scrollTop > 1
    const fundo = box.scrollTop + box.clientHeight < box.scrollHeight - 1
    setRolagem((r) => (r.topo === topo && r.fundo === fundo ? r : { topo, fundo }))
  }

  const pararArraste = () => {
    arrastando.current = false
    ponteiro.current = null
    if (quadro.current) {
      cancelAnimationFrame(quadro.current)
      quadro.current = 0
    }
  }
  useEffect(() => () => {
    arrastando.current = false
    ponteiro.current = null
    if (quadro.current) cancelAnimationFrame(quadro.current)
  }, [])

  /** Editar (↑ ou lápis) rola até a linha: ela pode estar fora das 3 visíveis. */
  useEffect(() => {
    if (editandoId === undefined) return
    const box = caixa.current
    const el = box?.querySelector<HTMLElement>('.fila-linha.editando')
    if (!box || !el) return
    const r = el.getBoundingClientRect()
    const b = box.getBoundingClientRect()
    if (r.top < b.top) box.scrollTop += r.top - b.top
    else if (r.bottom > b.bottom) box.scrollTop += r.bottom - b.bottom
  }, [editandoId, editandoPos])

  // Mede de novo quando a fila muda de tamanho ou a bandeja reabre (o scrollport é outro elemento).
  useLayoutEffect(medirRolagem, [itens.length, fila.recolhida])

  if (!itens.length) return null
  const vistas = arraste ? mover(itens, arraste.de, arraste.para) : itens
  const resumo = (e: Entry) => textoDaLinha(e).trim() || t('conversa.fila.soAnexos')
  const origem = (e: Entry) => origemDoPedido(e, {
    bot: (id) => store.bots.get(id)?.name,
    sessao: (chat, id) => store.sessions.of(chat)?.find((x) => x.id === id)?.title,
    conversa: (chat) => store.conversations.get(chat)?.title,
  })

  /** Posição do ponteiro no conteúdo da lista (imune à rolagem: muda junto com o scrollTop). */
  const yNaLista = (clientY: number) => {
    const box = caixa.current
    return box ? clientY - box.getBoundingClientRect().top + box.scrollTop : clientY
  }

  /** Um quadro por movimento: perto da borda, rola e recalcula o destino com a lista já deslocada. */
  const rolarNaBorda = () => {
    quadro.current = 0
    const box = caixa.current
    const y = ponteiro.current
    if (!box || y === null || !arrastando.current) return
    const r = box.getBoundingClientRect()
    const margem = 20
    // Negativo na zona do topo (sobe; satura fora da caixa), positivo na de baixo.
    const excesso = y < r.top + margem ? y - r.top - margem : y > r.bottom - margem ? y - r.bottom + margem : 0
    if (excesso === 0) return
    const passo = Math.sign(excesso) * Math.ceil(12 * Math.min(1, Math.abs(excesso) / margem))
    const antes = box.scrollTop
    box.scrollTop = Math.min(Math.max(0, box.scrollTop + passo), box.scrollHeight - box.clientHeight)
    if (box.scrollTop === antes) return
    setArraste((a) => {
      if (!a) return a
      const para = destinoDoArraste(centros.current, yNaLista(y), a.de)
      return para === a.para ? a : { ...a, para }
    })
    quadro.current = requestAnimationFrame(rolarNaBorda)
  }

  const pegar = (de: number) => (e: ReactPointerEvent<HTMLElement>) => {
    if (e.button !== 0 || arrastando.current || !caixa.current) return
    e.preventDefault()
    // A captura fica na lista, não na alça: a prévia reordena o DOM e o Blink solta a
    // captura de quem é movido — na lista (que não se move) o pointerup sempre chega.
    caixa.current.setPointerCapture(e.pointerId)
    const box = caixa.current
    const base = box ? box.getBoundingClientRect().top - box.scrollTop : 0
    centros.current = linhas.current.slice(0, itens.length).map((el) => {
      const r = el?.getBoundingClientRect()
      return r ? r.top + r.height / 2 - base : 0
    })
    arrastando.current = true
    ponteiro.current = e.clientY
    if (!quadro.current) quadro.current = requestAnimationFrame(rolarNaBorda)
    setArraste({ de, para: de })
  }
  const arrastar = (e: ReactPointerEvent<HTMLElement>) => {
    if (!arraste) return
    ponteiro.current = e.clientY
    if (!quadro.current) quadro.current = requestAnimationFrame(rolarNaBorda)
    const para = destinoDoArraste(centros.current, yNaLista(e.clientY), arraste.de)
    if (para !== arraste.para) setArraste({ ...arraste, para })
  }
  const soltar = () => {
    if (!arraste) return
    pararArraste()
    const { de, para } = arraste
    setArraste(null)
    if (para !== de) fila.mover(itens[de]!, para)
  }
  const cancelarArraste = () => {
    pararArraste()
    setArraste(null)
  }

  return (
    <div className={`fila-bandeja${fila.recolhida ? ' recolhida' : ''}`}>
      <div className="fila-cabeca">
        <span className="fila-titulo">{t('conversa.fila.titulo')}</span>
        <span className="fila-conta">· {itens.length}</span>
        {fila.recolhida ? <span className="fila-primeira">· {resumo(itens[0]!)}</span> : null}
        <span className="fila-espaco" />
        {!fila.recolhida && !fila.edicao ? <span className="fila-atalhos">{t('conversa.fila.atalhos')}</span> : null}
        <button className="press fila-seta" aria-expanded={!fila.recolhida} title={fila.recolhida ? t('conversa.fila.expandir') : t('conversa.fila.recolher')}
          aria-label={fila.recolhida ? t('conversa.fila.expandir') : t('conversa.fila.recolher')} onClick={fila.alternar}>
          <Icon name="chevron.down" size={10} weight="semibold" />
        </button>
      </div>
      {fila.recolhida ? null : (
        <div className="fila-caixa">
          <div className="fila-linhas" role="list" aria-label={t('conversa.fila.lista')} ref={caixa} tabIndex={0}
            onScroll={medirRolagem}
            onPointerMove={arrastar} onPointerUp={soltar} onPointerCancel={cancelarArraste}>
            {vistas.map((entry, i) => {
              const de = itens.indexOf(entry)
              const editando = fila.edicao?.entry.id === entry.id
              const doBot = pedidoNaFila(entry)
              return (
                <div key={entry.id} ref={(el) => { linhas.current[i] = el }} role="listitem" tabIndex={0}
                  className={`fila-linha${doBot ? ' pedido' : ''}${editando ? ' editando' : ''}${arraste?.de === de ? ' arrastando' : ''}`}
                  onKeyDown={(e) => {
                    if (e.target !== e.currentTarget || !e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown')) return
                    e.preventDefault()
                    const para = de + (e.key === 'ArrowUp' ? -1 : 1)
                    if (!doBot && para >= 0 && para < itens.length && !pedidoNaFila(itens[para]!)) fila.mover(entry, para)
                  }}>
                  {doBot ? <span className="fila-alca fila-alca-vazia" aria-hidden="true" /> : (
                    <span className="fila-alca" aria-hidden="true" title={t('conversa.fila.arrastar')}
                      onPointerDown={pegar(de)}>⋮⋮</span>
                  )}
                  {doBot ? <span className="fila-origem">{t('conversa.fila.de', { origem: origem(entry) })}</span> : null}
                  <span className="fila-texto" title={textoDaLinha(entry)}>{resumo(entry)}</span>
                  {doBot && entry.data.botMessage?.edited ? <span className="fila-editado">{t('conversa.fila.editado')}</span> : null}
                  <span className="fila-acoes">
                    <button className="press fila-acao fila-icone fila-agora" title={t('conversa.fila.mandarAgora')} aria-label={t('conversa.fila.mandarAgora')} disabled={fila.bloqueada} onClick={() => fila.mandarAgora(entry)}>
                      <Icon name="arrow.up" size={12} weight="semibold" />
                    </button>
                    <button className="press fila-acao fila-icone" title={t('conversa.fila.editar')} aria-label={t('conversa.fila.editar')} onClick={() => fila.editar(de)}>
                      <Icon name="pencil" size={12} />
                    </button>
                    <button className="press fila-acao fila-icone" title={t('conversa.fila.remover')} aria-label={t('conversa.fila.remover')} onClick={() => fila.remover(entry)}>
                      <Icon name="trash" size={12} />
                    </button>
                  </span>
                </div>
              )
            })}
          </div>
          <div className={`fila-dica topo${rolagem.topo ? ' on' : ''}`} aria-hidden="true" />
          <div className={`fila-dica fundo${rolagem.fundo ? ' on' : ''}`} aria-hidden="true" />
        </div>
      )}
    </div>
  )
}

/** "✎ Editando a Nª da fila · Cancelar", em cima do texto em edição. */
export function BarraEdicao({ fila }: { fila: Fila }) {
  const t = useT()
  if (!fila.edicao) return null
  return (
    <div className="fila-editbar">
      <Icon name="pencil" size={11} />
      <span>{t('conversa.fila.editando', { n: String(fila.edicao.posicao + 1) })}</span>
      <span className="fila-espaco" />
      <button className="press fila-cancelar" onClick={fila.cancelar}>{t('conversa.fila.cancelar')}</button>
    </div>
  )
}
