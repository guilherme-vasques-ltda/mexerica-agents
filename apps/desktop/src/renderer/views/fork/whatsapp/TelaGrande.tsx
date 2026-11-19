import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from 'react'
import type { Bot } from '@shared/models'
import { BotAvatar } from '../../../components/Avatar'
import { Icon } from '../../../components/Icon'
import { useEscape } from '../../../components/Overlay'
import { reduceMotion } from '../../../lib/theme'
import { useStore } from '../../../store/context'
import { useOpenBotSettings } from '../bot-settings/bot-settings-entry'
import type { RightPanel } from '../painel-direito/PainelDireito'
import { estadoNumero, mostraAba, verLigado } from './caixa'
import { NumeroBot } from './NumeroBot'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { comNos, no, semParametros } from '../../settings/rotulo'
import { SuperficieAVista } from './superficie'
import { Aviso, Caixa, useCaixa, type WaControle } from './MiniWhatsapp'
import { TextoDaMensagem } from './partes'
import { origemDoVoo, podeFocar, seguraTecla, recuo, SELETOR_FOCO, teclaTelaGrande, tempoDoToken, tempoDoVoo, transformeMoldura, type AlvoFoco, type Retangulo } from './tela-grande'

// whatsapp R5c (D336): Expandir leva o mini WhatsApp do painel para a tela grande no centro; Recolher volta ao painel,
// na mesma conversa e sempre FIXO (aberto pelo hover, o painel solto fecharia sozinho quando o mouse saísse; o #352
// já guarda "fixo" numa preferência, então fixar é uma linha). Só a moldura anima, com `transform`: a duração de
// `--layout` (0,3 s) e a curva do fixar do #352 (`PANEL_EASE`). O conteúdo troca com `--fade` depois que ela assenta.
// Movimento reduzido: troca na hora. Invertida no meio, a volta parte de onde a moldura está (A1); o foco vai ao
// controle de cada lado quando a troca termina (A2).

type Fase = 'parada' | 'abrindo' | 'fechando'

const token = (nome: string) => tempoDoToken(getComputedStyle(document.documentElement).getPropertyValue(nome))
const ret = (el: Element | null | undefined): Retangulo | null => {
  const r = el?.getBoundingClientRect()
  return r && r.width > 0 && r.height > 0 ? { left: r.left, top: r.top, width: r.width, height: r.height } : null
}

/**
 * A moldura que voa: um retângulo vazio, com o fundo, a borda e a sombra do painel fixo. Sem texto, nada estica.
 * A2: ela guarda o foco durante o voo (tabIndex -1, com rótulo; o Tab não sai dela), porque a origem já sumiu e o
 * destino ainda está escondido; no fim, o foco vai ao controle do destino.
 */
function voar(de: Retangulo, para: Retangulo, rotulo: string): { el: HTMLElement; anim: Animation } | null {
  const { ms, curva } = tempoDoVoo(getComputedStyle(document.documentElement).getPropertyValue('--layout'))
  if (reduceMotion() || !ms || !document.body.animate) return null
  const el = document.createElement('div')
  el.className = 'wa-moldura'
  el.tabIndex = -1
  el.setAttribute('role', 'group')
  el.setAttribute('aria-label', rotulo)
  el.addEventListener('keydown', (e) => { if (seguraTecla(e.key)) e.preventDefault() })
  Object.assign(el.style, { left: `${para.left}px`, top: `${para.top}px`, width: `${para.width}px`, height: `${para.height}px` })
  document.body.appendChild(el)
  const anim = el.animate([{ transform: transformeMoldura(de, para) }, { transform: 'none' }], { duration: ms, easing: curva, fill: 'forwards' })
  el.focus({ preventScroll: true })
  return { el, anim }
}

/** A moldura que assentou some no `--fade` (ou na hora, quando o destino é idêntico a ela). */
function sumir(el: HTMLElement, fade: boolean) {
  const { ms, curva } = token('--fade')
  if (!fade || !ms) return el.remove()
  el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: ms, easing: curva, fill: 'forwards' }).onfinish = () => el.remove()
}

/**
 * O estado do Expandir/Recolher e da conversa aberta, guardado na conversa do bot (ThreadView), para o painel e a
 * tela grande mostrarem a mesma. `onAbaWhatsapp`: o Recolher devolve o painel na aba WhatsApp.
 */
export function useTelaGrande(panel: RightPanel, root: RefObject<HTMLElement | null>, onAbaWhatsapp: () => void, onConversaDoBot: () => void) {
  const [chat, setChat] = useState<string | null>(null)
  const t = useT()
  const [expandida, setExpandida] = useState(false)
  const [fase, setFase] = useState<Fase>('parada')
  const voo = useRef<{ de: Retangulo; para: 'centro' | 'painel' } | null>(null)
  const moldura = useRef<HTMLElement | null>(null)
  const centro = () => {
    const r = ret(root.current?.querySelector('.conversation'))
    return r ? recuo(r, 8) : null
  }
  // A2: a moldura que assentou fica até o foco chegar ao destino (o painel sai do véu na mesma renderização).
  const assentada = useRef<{ el: HTMLElement; fade: boolean } | null>(null)
  const limpar = () => {
    moldura.current?.remove()
    moldura.current = null
    assentada.current?.el.remove()
    assentada.current = null
  }
  // A1: onde a moldura está agora (no meio de um voo), lido antes de removê-la.
  const emVoo = () => ret(moldura.current)
  // A2: o controle que recebe o foco quando a troca terminar.
  const foco = useRef<AlvoFoco | null>(null)
  useEffect(() => limpar, [])

  const expandir = useCallback(() => {
    if (expandida) return
    const de = origemDoVoo(emVoo(), ret(panel.ref.current))
    limpar()
    foco.current = 'tela'
    voo.current = de ? { de, para: 'centro' } : null
    setFase(de && !reduceMotion() ? 'abrindo' : 'parada')
    setExpandida(true)
    panel.setAway(true)
  }, [expandida, panel])

  const recolher = useCallback(() => {
    if (!expandida) return
    const de = origemDoVoo(emVoo(), centro())
    limpar()
    foco.current = 'painel'
    setExpandida(false)
    panel.setAway(false)
    onAbaWhatsapp()
    if (panel.roomy) {
      panel.pinNow()
      voo.current = de ? { de, para: 'painel' } : null
      const anima = !!de && !reduceMotion()
      setFase(anima ? 'fechando' : 'parada')
      panel.setVeil(anima)
    } else {
      // Janela estreita: o fixo não cabe; o painel abre por cima, sem voo.
      voo.current = null
      setFase('parada')
      panel.show()
    }
  }, [expandida, panel, onAbaWhatsapp])

  // O voo começa antes de pintar, com o destino já no lugar (o centro montado ou o painel fixo).
  useLayoutEffect(() => {
    const v = voo.current
    voo.current = null
    if (!v) return
    const para = v.para === 'centro' ? centro() : ret(panel.ref.current)
    const f = para ? voar(v.de, para, v.para === 'centro' ? t('whatsapp.caixa.abrindo') : t('whatsapp.caixa.voltando')) : null
    if (!f) {
      setFase('parada')
      panel.setVeil(false)
      return
    }
    moldura.current = f.el
    f.anim.onfinish = () => {
      if (moldura.current !== f.el) return
      moldura.current = null
      // Expandir: a moldura some no fade enquanto o conteúdo aparece. Recolher: o painel fixo tem o mesmo desenho
      // da moldura, então ela sai na hora e só o conteúdo do painel aparece no fade. Sai depois de entregar o foco.
      assentada.current = { el: f.el, fade: v.para === 'centro' }
      setFase('parada')
      panel.setVeil(false)
    }
  })

  // A2: o foco vai quando nada voa e o painel saiu do véu (com movimento reduzido, na mesma hora), antes de pintar;
  // só então a moldura assentada sai. Durante o voo, ele fica na moldura.
  useLayoutEffect(() => {
    if (!podeFocar(fase, panel.veil)) return
    const alvo = foco.current
    if (alvo) {
      for (const sel of SELETOR_FOCO[alvo]) {
        const el = (alvo === 'painel' ? panel.ref.current?.ownerDocument : root.current)?.querySelector<HTMLElement>(sel)
        if (!el) continue
        el.focus({ preventScroll: true })
        if (el.ownerDocument.activeElement === el) { foco.current = null; break }
      }
    }
    const a = assentada.current
    assentada.current = null
    if (a) sumir(a.el, a.fade)
  })

  // Esc = Recolher (as camadas por cima, como folhas e menus, fecham antes); ⌘1 = Recolher e a conversa do bot.
  // A camada é a conversa do bot: com um modal por cima, ela fica inerte e o Esc vai ao modal (fork-modal-stack).
  useEscape(expandida, recolher, root)
  const latest = useRef({ recolher, onConversaDoBot })
  latest.current = { recolher, onConversaDoBot }
  useEffect(() => {
    if (!expandida) return
    const mac = window.codync?.platform === 'darwin'
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || teclaTelaGrande(e, mac) !== 'conversa' || root.current?.closest('[inert]')) return
      e.preventDefault()
      e.stopPropagation()
      latest.current.recolher()
      latest.current.onConversaDoBot()
      foco.current = 'conversa'
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [expandida, root])

  // D374: o painel continua montado escondido (por CSS) com a tela grande ou fechado; o número revelado nele volta a esconder.
  const painelVisivel = panel.open && !expandida
  const controle: WaControle = useMemo(() => ({ chat, setChat, expandida, painelVisivel, expandir, recolher }), [chat, expandida, painelVisivel, expandir, recolher])
  return { controle, fase }
}

/** A tela grande no centro: o cartão de 44 px ("<bot> › WhatsApp", estado, Só leitura, Configurações, Recolher) e a caixa. */
export function TelaGrande({ bot, controle, fase }: { bot: Bot; controle: WaControle; fase: Fase }) {
  const store = useStore()
  const caixa = useCaixa(store)
  const openSettings = useOpenBotSettings()
  const wa = caixa.bot(bot.id)
  const numero = wa ? estadoNumero(wa) : null
  const t = useT()
  const idioma = useIdioma()
  // O número saiu ou o plugin foi desligado (D350): a aba some, e a tela grande volta ao painel.
  const some = !!caixa.leitura.leitura && !mostraAba(caixa.leitura.leitura.plugin.enabled, wa)
  useEffect(() => { if (some) controle.recolher() }, [some, controle])
  return (
    // D387: dentro da conversa escondida por CSS, a tela grande é a superfície à vista.
    <SuperficieAVista.Provider value>
    <div className={`wa-tela idioma-superficie${fase === 'abrindo' ? ' voando' : ''}`} {...superficieIdioma(idioma)} role="region" aria-label={t('whatsapp.caixa.regiao', { bot: bot.name })}>
      <div className="wa-x-chrome">
        <div className="ctabs wa-x-top" role="toolbar" aria-label={t('whatsapp.caixa.regiao', { bot: bot.name })}>
          <span className="ctab main on wa-x-who"><BotAvatar bot={bot} size={24} /><bdi dir="auto">{bot.name}</bdi><span className="c" aria-hidden>›</span>{t('whatsapp.pareamento.wa.whatsapp')}</span>
          {numero ? <span className={`wa-chip${numero.ruim ? ' bad' : ''}`}><span className="d" aria-hidden />{semParametros(t)(numero.texto)}{numero.phone ? <> · <NumeroBot phone={numero.phone} ativo={controle.expandida} /></> : null}</span> : null}
          <span className="sp" />
          <button className="wa-chip" onClick={() => openSettings?.(store, bot.id, 'whatsapp')}><Icon name="gearshape" size={12} scaled={false} />{t('whatsapp.caixa.configuracoes')}</button>
          <button className="wa-ibtn wa-x-recolher" title={t('whatsapp.caixa.recolherDica')} onClick={controle.recolher}><Icon name="arrow.down.right.and.arrow.up.left" size={12} scaled={false} />{t('whatsapp.caixa.recolher')}</button>
        </div>
      </div>
      {!wa ? (caixa.leitura.erroLeitura
        ? <Aviso icone="exclamationmark.triangle" titulo={t('whatsapp.caixa.naoLeuWhatsapp')} acao={t('whatsapp.caixa.tentar')} onAcao={() => void caixa.leitura.ler()} erro><TextoDaMensagem m={caixa.leitura.erroLeitura} /></Aviso>
        : <div className="wa-loading" aria-busy>{t('whatsapp.caixa.carregando')}</div>)
        : !verLigado(wa)
          ? <Aviso icone="eye.slash" titulo={t('whatsapp.caixa.verDesligado')} acao={t('whatsapp.caixa.abrirConfig')} onAcao={() => openSettings?.(store, bot.id, 'whatsapp')}>
              {comNos(t('whatsapp.caixa.ligueVer', { ver: no('ver') }), { ver: <b>{t('whatsapp.estados.opcao.verConversas')}</b> })}
            </Aviso>
          : <Caixa bot={bot} wa={wa} caixa={caixa} controle={controle} grande faixa={numero?.faixa ?? null} />}
    </div>
    </SuperficieAVista.Provider>
  )
}
