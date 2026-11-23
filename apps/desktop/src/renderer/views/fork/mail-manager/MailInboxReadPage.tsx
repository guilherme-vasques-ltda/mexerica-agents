import { useEffect, useState } from 'react'
import { Button, CardSection } from '../../../components/Controls'
import { useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { FixtureMailInboxGateway, MailInboxReadState } from './mail-inbox-fixtures'
import './mail-inbox-read.css'

export function MailInboxReadPage({ spaceId }: { spaceId: string | null }) {
  const t = useT()
  const [state] = useState(() => new MailInboxReadState(new FixtureMailInboxGateway(t), spaceId ?? ''))
  useModel(state)
  useEffect(() => { void state.setContext(spaceId) }, [state, spaceId])

  return (
    <section className="mm-inbox" aria-label={t('mailInbox.title')}>
      <header className="mm-inbox-header">
        <div><h2>{t('mailInbox.title')}</h2><p>{t('mailInbox.fixtureNotice')}</p></div>
        <Button kind="secondary" disabled={state.status === 'loading'} onClick={() => void state.refresh()}>{t('mailInbox.refresh')}</Button>
      </header>
      {state.status === 'loading' ? <p role="status">{t('mailInbox.loading')}</p> : null}
      {state.status === 'not_connected' ? <p role="status">{t('mailInbox.notConnected')}</p> : null}
      {state.status === 'reconnect_required' ? <p role="alert">{t('mailInbox.reconnect')}</p> : null}
      {state.status === 'unavailable' ? <p role="alert">{state.error === 'context_unavailable' ? t('mailInbox.contextUnavailable') : t('mailInbox.unavailable')}</p> : null}
      {state.status === 'ready' ? (
        <div className="mm-inbox-layout">
          <CardSection title={t('mailInbox.mailboxes')}>
            {state.mailboxes.map((mailbox) => <span key={mailbox.mailboxId} className={`mm-inbox-mailbox${state.mailboxId === mailbox.mailboxId ? ' selected' : ''}`}>{mailbox.label}</span>)}
            {state.mailboxes.length === 0 ? <p>{t('mailInbox.noMailboxes')}</p> : null}
            {state.loadingPage ? <p role="status">{t('mailInbox.loading')}</p> : null}
            {state.threads.length === 0 && !state.loadingPage ? <p>{t('mailInbox.empty')}</p> : null}
            <ul className="mm-inbox-threads">
              {state.threads.map((thread) => (
                <li key={thread.threadId}><button onClick={() => void state.openThread(thread.threadId)} aria-current={state.thread?.threadId === thread.threadId ? 'true' : undefined}>
                  <span>{thread.subject ?? t('mailInbox.noSubject')}</span><time dateTime={thread.lastMessageAt}>{new Date(thread.lastMessageAt).toLocaleString()}</time>
                </button></li>
              ))}
            </ul>
            {state.nextCursor ? <Button kind="secondary" disabled={state.loadingPage} onClick={() => void state.loadNextPage()}>{t('mailInbox.loadMore')}</Button> : null}
          </CardSection>
          <CardSection title={t('mailInbox.thread')}>
            {state.loadingThread ? <p role="status">{t('mailInbox.loading')}</p> : null}
            {!state.thread && !state.loadingThread ? <p>{t('mailInbox.chooseThread')}</p> : null}
            {state.thread ? <article className="mm-inbox-thread">
              <h3 dir="auto">{state.thread.subject ?? t('mailInbox.noSubject')}</h3>
              {state.thread.messages.map((message) => <section className="mm-inbox-message" key={message.messageId}>
                <header><bdi dir="auto">{message.fromDisplay ?? t('mailInbox.unknownSender')}</bdi>{message.fromDomain ? <span>{t('mailInbox.declaredDomain')}: <bdi dir="ltr">{message.fromDomain}</bdi></span> : null}<time dateTime={message.sentAt}>{new Date(message.sentAt).toLocaleString()}</time></header>
                <pre dir="auto">{message.bodyText}</pre>
              </section>)}
              {state.thread.truncated ? <p>{t('mailInbox.truncated')}</p> : null}
            </article> : null}
          </CardSection>
        </div>
      ) : null}
      {state.error ? <p role="alert">{t('mailInbox.error')}</p> : null}
    </section>
  )
}
