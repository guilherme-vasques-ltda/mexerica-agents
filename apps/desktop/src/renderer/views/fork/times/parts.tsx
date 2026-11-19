import { memo, useMemo, type ReactNode } from 'react'
import { Icon } from '../../../components/Icon'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import type { TimesBot, TimesContact, TimesGraph, TimesMember, TimesRole } from '@shared/fork-times'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { ProfileAvatar } from '../../PanelMenus'
import { useChatInfo } from './use-chat-info'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { frase, textoDe, type Frase } from '../../../lib/fork-i18n/frase'

// Peças comuns da tela Times: o rosto oficial a partir do DTO mínimo do Hub, os textos e um grupo de opções que
// aceita opção desativada com motivo (o segmentado do app não tem). D454 (fatia 9c): os textos são frases do
// catálogo `times.quadro.*`; cada componente os pega no idioma da tela com `useTimesTextos()`.

/** O rosto oficial (BotFace, D115), estático: o DTO não traz estado de trabalho, e a tela não inventa um. */
export const TimesFace = memo(function TimesFace({ bot, size }: { bot: TimesBot | null; size: number }) {
  if (!bot) return <span className="times-face-missing" style={{ width: size, height: size }} aria-hidden />
  const config = normalizarAvatarDoBot(avatarDoBot(bot), bot.name)
  return <DonnabotAvatar config={config} size={size} animated={false} label={bot.name} />
})

export function OwnerFace({ size }: { size: number }) {
  return <span className="times-owner-face" style={{ width: size, height: size }} aria-hidden><ProfileAvatar /></span>
}

const Q = 'times.quadro.'
/** Os textos comuns da tela Times, como frases (traduzidas no idioma da tela por `useTimesTextos`). */
export const TF = {
  title: frase(`${Q}titulo`),
  lead: frase(`${Q}lead`),
  back: frase(`${Q}voltar`),
  board: frase(`${Q}quadro`),
  list: frase(`${Q}lista`),
  newTeam: frase(`${Q}novaEquipe`),
  owner: frase(`${Q}voce`),
  loose: frase(`${Q}semEquipe`),
  reportsTo: frase(`${Q}reportaA`),
  below: frase(`${Q}ficaAbaixo`),
  reportsMeaning: frase(`${Q}reportaSignifica`),
  routesMeaning: frase(`${Q}rotasSignificam`),
  noPower: frase(`${Q}semPoder`),
  contactMeaning: frase(`${Q}contatoSignifica`),
  dragOnlyLayout: frase(`${Q}arrastarSoDisposicao`),
  chat: frase(`${Q}conversaDica`),
  chatInfo: frase(`${Q}conversa`),
  retry: frase(`${Q}tentar`),
  readonly: frase(`${Q}soLeitura`),
  noHub: frase(`${Q}semHub`),
  moved: frase(`${Q}outroComputador`),
  loading: frase(`${Q}abrindo`),
  send: frase(`${Q}conferirMudanca`),
} as const

export const ROLE_FRASE: Record<TimesRole, Frase> = { manager: frase(`${Q}papel.manager`), executor: frase(`${Q}papel.executor`) }
export const CONTACT_FRASE: Record<TimesContact, Frase> = { reports: frase(`${Q}contato.reports`), urgent_only: frase(`${Q}contato.urgent_only`), on_request: frase(`${Q}contato.on_request`) }

/** Nome de um bot pelo ID, como frase: o nome é dado; um ID sem bot (apagado) aparece como tal, nunca vazio. */
export function nomeDoBot(bots: ReadonlyMap<string, TimesBot>, id: string | null | undefined): Frase {
  if (!id) return { texto: '—' }
  const name = bots.get(id)?.name
  return name !== undefined ? { texto: name } : frase(`${Q}botRemovido`)
}

/** Os textos da tela no idioma dela: `T` e os rótulos já traduzidos, e os nomes de bot. */
export function useTimesTextos() {
  const idioma = useIdioma()
  return useMemo(() => {
    const tx = (f: Frase) => textoDe(f, idioma)
    const T = Object.fromEntries(Object.entries(TF).map(([k, f]) => [k, tx(f)])) as { readonly [K in keyof typeof TF]: string }
    const ROLE_LABEL: Record<TimesRole, string> = { manager: tx(ROLE_FRASE.manager), executor: tx(ROLE_FRASE.executor) }
    const CONTACT_LABEL: Record<TimesContact, string> = { reports: tx(CONTACT_FRASE.reports), urgent_only: tx(CONTACT_FRASE.urgent_only), on_request: tx(CONTACT_FRASE.on_request) }
    const nameOf = (bots: ReadonlyMap<string, TimesBot>, id: string | null | undefined) => tx(nomeDoBot(bots, id))
    const reportsLabel = (bots: ReadonlyMap<string, TimesBot>, m: TimesMember) => (m.reportsTo.kind === 'owner' ? T.owner : nameOf(bots, m.reportsTo.botId))
    /** O separador de uma lista de nomes no idioma ("، " em árabe, "、" em chinês). */
    const sep = tx(frase(`${Q}separador`))
    return { idioma, tx, T, ROLE_LABEL, CONTACT_LABEL, nameOf, reportsLabel, sep, superficie: superficieIdioma(idioma) }
  }, [idioma])
}

export function areaPath(graph: TimesGraph, nodeId: string | null): string {
  const names: string[] = []
  const seen = new Set<string>()
  let at = nodeId
  while (at && !seen.has(at)) {
    seen.add(at)
    const node = graph.nodes.find((n) => n.id === at)
    if (!node) break
    names.unshift(node.name)
    at = node.parentId
  }
  return names.join(' › ')
}

/** Um grupo de opções exclusivas; cada opção pode vir desativada com o motivo (na dica e no leitor de tela). */
export function OptionGroup<T extends string>({ label, value, options, onChange, disabled }: {
  label: string
  value: T
  options: { id: T; label: string; reason?: string | null }[]
  onChange: (id: T) => void
  disabled?: boolean
}) {
  return (
    <div className="times-options" role="radiogroup" aria-label={label}>
      {options.map((o) => {
        const off = disabled || !!o.reason
        return (
          <button
            key={o.id}
            type="button"
            role="radio"
            aria-checked={o.id === value}
            aria-disabled={off || undefined}
            aria-description={o.reason ?? undefined}
            title={o.reason ?? undefined}
            className={o.id === value ? 'on' : undefined}
            onClick={() => { if (!off && o.id !== value) onChange(o.id) }}
          >
            {o.label}
          </button>
        )
      })}
    </div>
  )
}

export function Note({ children }: { children: ReactNode }) {
  return <p className="times-note">{children}</p>
}

/**
 * D249 E6: Conversa do time ainda não existe; o (i) do cabeçalho diz isso numa dica, fora da vista principal.
 * Abre com o ponteiro (inclusive sobre a própria dica) e com o foco do teclado; Esc fecha sem tirar o foco,
 * e só reabre depois que ponteiro e foco saírem. Sem ação: nada simula envio.
 */
export function TimesChatInfo() {
  const info = useChatInfo()
  const { T } = useTimesTextos()
  return (
    <span ref={info.trigger} className="times-info" onMouseEnter={info.enterHover} onMouseLeave={(e) => info.leaveHover(e.clientX, e.clientY)}>
      <button
        type="button"
        className="times-info-button"
        aria-label={T.chatInfo}
        aria-describedby="times-chat-info"
        onFocus={(e) => { if (e.currentTarget.matches(':focus-visible')) info.focus() }}
        onBlur={info.blur}
      >
        <Icon name="info.circle" size={15} />
      </button>
      <span ref={info.tip} id="times-chat-info" role="tooltip" className={`times-info-tip${info.shown ? ' is-open' : ''}`}>{T.chat}</span>
    </span>
  )
}
