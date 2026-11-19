import { HistoryRow } from './SpaceView'
import { frase } from '../../../lib/fork-i18n/frase'
import { useState, type ReactNode } from 'react'
import type { Space } from '@shared/fork-spaces'
import type { AppModel } from '../../../store/app-model'
import type { BotStore } from '../../../store/bot-store'
import type { SpaceContext } from '../../../store/fork-space-context'
import { useIdioma, useT, superficieIdioma } from '../../../lib/fork-i18n/estado'
import { Button } from '../../../components/Controls'
import { Sheet } from '../../../components/Overlay'

/** A tela completa mantém a identidade do Space mesmo indisponível; extras antigos são só leitura. */
export function SpaceScopeStatus({ app, root, space, legacy, onActivate, children }: {
  app: AppModel; root: BotStore; space: Space; legacy: SpaceContext; onActivate: () => void; children: ReactNode
}) {
  const t = useT()
  const idioma = useIdioma()
  const language = superficieIdioma(idioma)
  const [show, setShow] = useState(false)
  const store = app.spaceMirrors.current(root.computer.id)
  const extras = legacy.list.conversations.filter(c => c.id !== c.botId && !c.closed)
  const old = root.hello?.spacesContract !== 1
  if (app.spaceMirrors.active?.hostId !== root.hello?.hostId) return <div className="space-scope-empty" role="status">{t('spaces.origem.outroHub')}</div>
  if (old || !store) return <div className="space-scope-empty" role="status">{t('spaces.selector.old')}</div>
  return <>
    {store.connection.kind !== 'online' ? <div className="space-scope-banner" role="status">{t('spaces.selector.offline')}</div> : null}
    {!space.principalBotId ? <div className="space-scope-empty">
      <p>{t('spaces.view.bergaInativo')}</p>
      <Button disabled={root.hello?.spaceLifecycleContract !== 1 || root.connection.kind !== 'online'} onClick={onActivate}>{t('spaces.sheet.ativar')}</Button>
    </div> : children}
    {extras.length ? <button className="space-legacy-open no-drag" onClick={() => { legacy.select(extras[0]!.id); setShow(true) }}>{t('spaces.selector.legacy')}</button> : null}
    <Sheet open={show} onClose={() => setShow(false)} width={700} language={language}>
      <section className="space-legacy-history">
        <h2>{t('spaces.selector.legacy')}</h2>
        <p>{t('spaces.selector.readOnly')}</p>
        <nav>{extras.map(c => <button key={c.id} onClick={() => legacy.select(c.id)} aria-pressed={legacy.conversationId === c.id}>
          {c.title || t('spaces.selector.legacy')} {c.botId === root.principal.id ? `· ${t('spaces.conversa.bergadoPessoal')}` : ''}
        </button>)}</nav>
        {legacy.history.state === 'loading' ? <p role="status">{t('spaces.view.carregando')}</p> : null}
        {legacy.history.state === 'error' ? <Button onClick={() => legacy.retry()}>{t('spaces.view.atualizar')}</Button> : null}
        <div>{legacy.history.entries.map(e => <HistoryRow key={e.id} entry={e} idioma={idioma} bot={frase('spaces.conversa.bergadoPessoal')} />)}</div>
        {legacy.history.hasMore ? <Button disabled={legacy.history.older} onClick={() => legacy.loadOlder()}>{t('spaces.view.anteriores')}</Button> : null}
      </section>
    </Sheet>
  </>
}
