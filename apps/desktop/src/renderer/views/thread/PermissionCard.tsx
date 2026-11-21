// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useState } from 'react'
import type { Entry, FileDiff, PermissionOption } from '@shared/models'
import { Disclosure, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import './permission-card.css'
import './cards/approval-card.css'
import { HoldButton } from './cards/HoldButton'
import { useReduceMotion } from '../../lib/theme'
import { useJanelaBaixa } from '../../lib/janela-baixa'
import { useStoreRef } from '../../store/context'
import { useSelector } from '../../lib/observable'
import { emQueda } from '../../store/fork-queda-curta'
import { actionLabel, memoryPermissionNote, permissionDetail, permissionTitle } from './permission-copy'
import { superficieIdioma, useIdioma, tradutor, type Tradutor } from '../../lib/fork-i18n/estado'
export { permissionHeadline, permissionTitle } from './permission-copy'

const rank: Record<string, number> = { allow_once: 0, allow_always: 1, reject_once: 2, reject_always: 3 }

/** Allow before deny, once before always. */
export const sortedOptions = (options: PermissionOption[]) => [...options].sort((a, b) => (rank[a.kind] ?? 9) - (rank[b.kind] ?? 9))

const SESSAO = ['allow_for_session', 'allow_session', 'allow_permissions_session']
const naSessao = (o: PermissionOption) => o.kind === 'allow_always' && SESSAO.includes(o.optionId)

export function optionLabel(t: Tradutor, o: PermissionOption, screen = false) {
  if (screen) {
    switch (o.kind) {
      case 'allow_once': return t('conversa.aprovacao.tela.agora')
      case 'allow_always': return t('conversa.aprovacao.tela.sempre')
      case 'reject_once': return t('conversa.aprovacao.tela.recusar')
      default: return o.name
    }
  }
  if (naSessao(o)) return t('conversa.aprovacao.opcao.sessao')
  switch (o.kind) {
    case 'allow_once':
      return t('conversa.aprovacao.opcao.umaVez')
    case 'allow_always':
      return t('conversa.aprovacao.opcao.sempre')
    case 'reject_once':
      return t('conversa.aprovacao.opcao.recusar')
    case 'reject_always':
      return t('conversa.aprovacao.opcao.nunca')
    default:
      return o.name
  }
}

/**
 * D165 fase F, D169 (components/ApprovalCard): what the bot wants, the facts, and the Hub's own options.
 * Allowing is held (HoldButton); the narrowest allow is the prominent one, broader allows are secondary;
 * refusing is one click. Same option ids and `respond` as before: no new grant, scope or callback, and
 * nothing is answered by itself. No deadline, answer time or undo is drawn: the Hub sends none.
 */
export function PermissionCard({ entry, hostName, answering, respond }: { entry: Entry; hostName: string; answering: string | null; respond: (option: string | null) => void }) {
  const [expanded, setExpanded] = useState(false)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const reducedMotion = useReduceMotion()
  // E4: in a window under 600 px tall the facts go behind Detalhes, so the card fits.
  const baixa = useJanelaBaixa()
  const d = entry.data
  const screen = d.toolKind === 'screen_access' && !!d.screenAccess
  const pending = d.status === 'pending'
  // Defesa para qualquer cartão: detalhe maior que o corte da tela (2000) não pode ser conferido inteiro,
  // então o corte fica visível e aprovar não é oferecido.
  const detalheCortado = !screen && !!d.detail && d.detail.length > 2000
  const hasDetail = !screen && (!!d.command || !!d.detail || !!d.diffs?.length)
  const options = sortedOptions(d.options ?? [])
  // O corte tira só as opções de aprovar; recusar segue disponível.
  const opcoes = detalheCortado ? options.filter((o) => !o.kind.startsWith('allow')) : options
  // The narrowest allow is the prominent one (`allow_once`, else the first allow the Hub offers).
  const prominent = (options.find((o) => o.kind === 'allow_once') ?? options.find((o) => o.kind.startsWith('allow')))?.optionId
  const store = useStoreRef()
  // MX-217: sem conexão, aprovar e recusar ficam à vista, desligados, com o motivo na dica.
  const semConexao = useSelector(store, emQueda)
  const folder = d.cwd ? d.cwd.split('/').filter(Boolean).pop() ?? d.cwd : null
  const result = pending ? null : outcome(t, entry)
  const allowed = (!screen || d.screenAccess?.effect === true) && !pending && d.status === 'answered' && !!d.options?.find((o) => o.optionId === d.selected)?.kind.startsWith('allow')
  const refused = !pending && d.status === 'answered' && !allowed
  const title = screen ? t('conversa.aprovacao.tela.titulo') : permissionTitle(t, d.toolKind, d.author ? store.bots.get(d.author)?.name : null)
  const memoryNote = memoryPermissionNote(t, d.toolKind)
  const detail = screen ? null : permissionDetail(t, d.toolKind, d.title)
  const facts = (
    <dl className="approval-facts">
      <dt>{t('conversa.aprovacao.fato.acao')}</dt><dd>{screen ? t('conversa.aprovacao.tela.acao') : actionLabel(t, d.toolKind)}</dd>
      <dt>{t('conversa.aprovacao.fato.onde')}</dt><dd><bdi>{hostName}</bdi>{folder ? <> · <bdi dir="ltr" className="codigo-ltr">{folder}</bdi></> : null}</dd>
      <dt>{t('conversa.aprovacao.fato.quem')}</dt><dd>{t('conversa.aprovacao.fato.voce')}</dd>
    </dl>
  )
  return (
    <div className="agent-inset idioma-superficie" {...superficieIdioma(idioma)}>
      <section className="approval-card" aria-label={title}>
        {pending ? (
          <span className="approval-badge waiting"><Icon name="clock" size={13} />{t('conversa.aprovacao.esperando')}</span>
        ) : (
          <span className={`approval-badge${allowed ? ' allowed' : ''}`}>{allowed ? <Icon name="checkmark" size={12} weight="semibold" /> : refused ? <Icon name="xmark" size={11} weight="semibold" /> : null}{result}</span>
        )}
        <h3 className="approval-title">{title}</h3>
        {detail ? (
          <Text as="div" mono tone="secondary" lines={expanded ? undefined : baixa ? 2 : 3}
            // O título cru do Hub (`Edit src/a.ts`) é técnico: LTR isolado. Na memória o detalhe é a frase traduzida
            // "Nome técnico: {nome}", que segue a direção do cartão; o nome entra isolado pelo interpolar (FSI…PDI).
            dir={memoryNote ? undefined : 'ltr'} className={`selectable permission-card-title${memoryNote ? ' permission-card-technical' : ' codigo-ltr'}`}>
            {detail}
          </Text>
        ) : null}
        {detalheCortado ? <p className="approval-note">{t('conversa.aprovacao.avisoDetalheCortado')}</p> : null}
        {baixa ? null : facts}
        {screen ? <>
          <p className="approval-note">{t('ajustes.telaPorComputador.aviso')}</p>
          <p className="approval-note">{t('conversa.aprovacao.tela.prazo')}</p>
          <p className="approval-note">{t('conversa.aprovacao.tela.celular')}</p>
          {!pending && d.screenAccess?.effect === false ? <p className="approval-note">{t('conversa.aprovacao.tela.semEfeito')}</p> : null}
        </> : null}
        {memoryNote ? <p className="approval-note">{memoryNote}</p> : null}
        {hasDetail || baixa ? (
          <Disclosure expanded={expanded} onToggle={() => setExpanded((e) => !e)} label={<Text size="caption" weight="medium" tone="secondary">{t('conversa.aprovacao.detalhes')}</Text>}>
            <VStack gap={8} className="permission-card-details">
              {baixa ? facts : null}
              {d.command ? <CodeBox text={d.command} /> : null}
              {d.detail ? <CodeBox text={d.detail.slice(0, 2000)} /> : null}
              {(d.diffs ?? []).map((diff, i) => <DiffView key={i} diff={diff} />)}
            </VStack>
          </Disclosure>
        ) : null}
        {pending ? (
          <>
            <div className="approval-actions">
              {opcoes.map((o) => o.kind.startsWith('allow') ? (
                <HoldButton key={o.optionId} primary={o.optionId === prominent} label={optionLabel(t, o, screen)} disabled={answering !== null || semConexao} onConfirm={() => respond(o.optionId)}>
                  <HStack as="span" gap={6}>{o.optionId === prominent ? <Icon name="checkmark" size={13} weight="semibold" /> : null}{optionLabel(t, o, screen)}{answering === o.optionId ? <Spinner size={12} /> : null}</HStack>
                </HoldButton>
              ) : (
                <button key={o.optionId} className="approval-deny" disabled={answering !== null || semConexao} onClick={() => respond(o.optionId)}>
                  <HStack as="span" gap={6}>{optionLabel(t, o, screen)}{answering === o.optionId ? <Spinner size={12} /> : null}</HStack>
                </button>
              ))}
            </div>
            {semConexao ? <p className="approval-note" role="status"><Icon name="wifi.slash" size={13} />{t('conversa.mensagens.queda.semAprovar')}</p> : null}
            {!semConexao && opcoes.some((o) => o.kind.startsWith('allow')) ? (
              <p className="approval-note"><Icon name="hand.raised" size={13} />{t(reducedMotion ? 'conversa.aprovacao.dica.toque' : 'conversa.aprovacao.dica.segure')}</p>
            ) : null}
          </>
        ) : null}
      </section>
    </div>
  )
}

function outcome(t: Tradutor, entry: Entry) {
  const d = entry.data
  if (d.toolKind === 'screen_access' && d.status === 'answered') {
    if (d.screenAccess?.effect !== true) return t('conversa.aprovacao.tela.semLiberacao')
    return d.selected === 'always' ? t('conversa.aprovacao.tela.permitidoSempre') : t('conversa.aprovacao.tela.permitidoAgora')
  }
  switch (d.status) {
    case 'answered': {
      const chosen = d.options?.find((o) => o.optionId === d.selected)
      if (chosen && naSessao(chosen)) return t('conversa.aprovacao.resultado.sessao')
      switch (chosen?.kind) {
        case 'allow_once':
          return t('conversa.aprovacao.resultado.umaVez')
        case 'allow_always':
          return t('conversa.aprovacao.resultado.sempre')
        case 'reject_once':
          return t('conversa.aprovacao.resultado.recusado')
        case 'reject_always':
          return t('conversa.aprovacao.resultado.nunca')
        default:
          return chosen?.name ?? t('conversa.aprovacao.resultado.respondido')
      }
    }
    case 'cancelled':
      return t('conversa.aprovacao.resultado.cancelado')
    default:
      return t('conversa.aprovacao.resultado.expirou')
  }
}

/**
 * R1 (fatia 2): comando, caminho e diff ficam sempre LTR e isolados do texto em volta, mesmo num cartão RTL
 * (ar-AE): pelo algoritmo bidi, `+const x = 1;` herdando RTL apareceria como `;const x = 1+`, e a pessoa
 * aprovaria algo que leu errado. `dir="ltr"` no bloco e `.codigo-ltr` (unicode-bidi: isolate) em permission-card.css.
 */
export function CodeBox({ text }: { text: string }) {
  return (
    <div className="code-box codigo-ltr" dir="ltr">
      <pre className="selectable code-box-text">{text}</pre>
    </div>
  )
}

export function DiffView({ diff }: { diff: FileDiff }) {
  const lines = diff.patch.split('\n').slice(0, 80)
  return (
    <VStack gap={4} className="codigo-ltr" dir="ltr">
      <HStack gap={6} className="diff-header">
        <Icon name={diff.isNew ? 'doc.badge.plus' : 'doc.text'} size={10} />
        <Text truncate grow>{diff.path.split('/').pop()}</Text>
        <Text tone="added">+{diff.added}</Text>
        <Text tone="removed">−{diff.removed}</Text>
      </HStack>
      <div className="code-box diff-box">
        <div className="code-box-text">
          {lines.map((line, i) => (
            <Text as="div" key={i} tone={line.startsWith('+') ? 'added' : line.startsWith('-') ? 'removed' : 'secondary'} className="diff-line">
              {line || ' '}
            </Text>
          ))}
        </div>
      </div>
    </VStack>
  )
}
