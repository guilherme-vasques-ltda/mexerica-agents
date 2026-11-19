// D545 / DON-907 C5: menus de sessões e conversa encerrada. Arquivar reutiliza `close`, só para extras abertas
// com anúncio no link atual. Copiar também vale na principal; excluir exige anúncio próprio do Host.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Dialog } from '../../../../components/Overlay'
import { SessionMenu } from '../../sessoes/SessionMenu'
import { createSessionMenuController, type SessionMenuOrigin } from '../../sessoes/menu-controller'
import { Icon } from '../../../../components/Icon'
import { Spinner } from '../../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../../lib/fork-i18n/estado'
import { useModel } from '../../../../lib/observable'
import type { BotStore } from '../../../../store/bot-store'
import type { Bot } from '@shared/models'
import { extrasAllowed } from '../../sessoes/ganchos'
import { sessionTitle, type HubSession } from '../../sessoes/estado'
import { motivoEncerrada, type ControleFalha } from './controle'
import './arquivar.css'

type Falha = { sessionId: string; falha: ControleFalha }

/** A falha de encerrar ou continuar, no mesmo cartão de aviso do compositor (texto pelo código, nunca do Hub). */
function AvisoFalha({ falha, onDismiss }: { falha: ControleFalha; onDismiss: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  return (
    <div className="session-composer-notice is-error idioma-superficie" role="alert" {...superficieIdioma(idioma)}>
      <span>{t(`conversa.arquivar.falha.${falha}`)}</span>
      <button onClick={onDismiss}>{t('conversa.arquivar.falha.fechar')}</button>
    </div>
  )
}

/** D545: menu das abas e da lista, sempre preso ao store/Space e link de origem. */
export function useEncerrar(store: BotStore, chat: Bot | null, lateral = false, onRemoved?: () => void) {
  const t = useT()
  const idioma = useIdioma()
  useModel(store.sessions)
  const chatId = chat?.id ?? ''
  type Origem = SessionMenuOrigin
  const [menu, setMenu] = useState<(Origem & { x: number; y: number }) | null>(null)
  const [confirmar, setConfirmar] = useState<Origem | null>(null)
  const [excluir, setExcluir] = useState<Origem | null>(null)
  const [falha, setFalha] = useState<Falha | null>(null)
  const [copyNotice, setCopyNotice] = useState<'copied' | 'copyFailed' | null>(null)
  useEffect(() => { setMenu(null); setConfirmar(null); setExcluir(null); setFalha(null); setCopyNotice(null) }, [store, store.spaceId, chatId])
  const live = useRef({ store, chat })
  live.current = { store, chat }
  const controller = useMemo(() => createSessionMenuController(() => live.current), [])
  const row = controller.row
  const alvo = controller.current(menu) ? row(menu!.id) : null
  const confirmada = controller.canArchive(confirmar) ? row(confirmar!.id) : null
  const excluirAlvo = lateral && controller.canRemove(excluir) ? row(excluir!.id) : null
  const copiar = (o: Origem, nome: boolean) => {
    setFalha(null)
    setCopyNotice(null)
    void controller.copy(o, nome, (text) => navigator.clipboard.writeText(text)).then((notice) => {
      if (notice) setCopyNotice(notice)
    })
  }
  const aviso = copyNotice ? <div className={`session-composer-notice${copyNotice === 'copyFailed' ? ' is-error' : ''} idioma-superficie`} role={copyNotice === 'copyFailed' ? 'alert' : 'status'} {...superficieIdioma(idioma)}>
    <span>{t(`sessions.menu.${copyNotice}`)}</span>
    <button onClick={() => setCopyNotice(null)}>{t('conversa.arquivar.falha.fechar')}</button>
  </div> : falha ? <AvisoFalha falha={falha.falha} onDismiss={() => setFalha(null)} /> : null
  const camada: ReactNode = <>
    <SessionMenu menuLabel={t('sessions.menu.label', { titulo: menu?.title ?? '' })} open={!!alvo} point={menu} language={superficieIdioma(idioma)} onClose={() => setMenu(null)}
      items={() => alvo && menu ? [
        { title: t('sessions.menu.copyId'), icon: 'doc.on.doc', action: () => copiar(menu, false) },
        { title: t(lateral ? 'sessions.menu.copyName' : 'sessions.menu.copySessionName'), icon: 'doc.text', action: () => copiar(menu, true) },
        { title: t('sessions.menu.archive'), icon: 'archivebox', disabled: !controller.canArchive(menu), action: () => { if (controller.canArchive(menu)) setConfirmar(menu) } },
        ...(lateral ? [{ title: t('sessions.menu.delete'), icon: 'trash', destructive: true, disabled: !controller.canRemove(menu), detail: store.sessions.can(chatId, 'delete') ? undefined : t('sessions.menu.deleteUnavailable'), action: () => { if (controller.canRemove(menu)) setExcluir(menu) } }] : []),
      ] : []} />
    <Dialog language={superficieIdioma(idioma)} open={!!confirmada} onClose={() => setConfirmar(null)} cancel={t('conversa.arquivar.encerrar.cancelar')}
      title={t('conversa.arquivar.encerrar.titulo', { titulo: confirmada ? sessionTitle(confirmada, t) : '' })}
      message={t('conversa.arquivar.encerrar.mensagem')}
      actions={[{ title: t('sessions.menu.archive'), destructive: true, action: () => {
        if (!confirmar) return
        setFalha(null)
        setCopyNotice(null)
        void controller.archive(confirmar).then((r) => {
          if (r && !r.ok) setFalha({ sessionId: confirmar.id, falha: r.falha })
        })
      } }]} />
    <Dialog language={superficieIdioma(idioma)} open={!!excluirAlvo} onClose={() => setExcluir(null)}
      cancel={t('conversa.arquivar.encerrar.cancelar')}
      title={t('sessions.delete.title', { titulo: excluir?.title ?? '' })}
      message={t('sessions.delete.message', { id: excluir?.id ?? '' })}
      actions={[{ title: t('sessions.menu.delete'), destructive: true, action: () => {
        if (!excluir) return
        setFalha(null)
        setCopyNotice(null)
        void controller.remove(excluir).then((r) => {
          if (!r) return
          if (r.ok) onRemoved?.()
          else if (r.falha) setFalha({ sessionId: excluir.id, falha: r.falha })
        })
      } }]} />
    {lateral ? aviso : null}
  </>
  return {
    abrir: (id: string, x: number, y: number, title?: string) => {
      const s = row(id)
      if (s) {
        const origin = controller.capture(id, title ?? sessionTitle(s, t))
        if (origin) setMenu({ ...origin, x, y })
      }
    },
    camada,
    aviso,
    menuId: alvo ? menu!.id : null,
  }
}

/**
 * No lugar do compositor de uma extra encerrada: o motivo em texto leigo, só leitura, e logo ao lado o "Continuar em
 * nova conversa" (só com `continue` anunciado). O botão fica desabilitado até a resposta: cada chamada cria outra sessão.
 */
export function ConversaEncerrada({ store, chat, session, botName, onBack, onOpen }: {
  store: BotStore
  chat: Bot | null
  session: HubSession
  botName: string
  onBack: () => void
  onOpen: (sessionId: string) => void
}) {
  const t = useT()
  const idioma = useIdioma()
  const model = useModel(store.sessions)
  const [falha, setFalha] = useState<ControleFalha | null>(null)
  const alive = useRef(true)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  useEffect(() => { setFalha(null) }, [session.id])
  const chatId = chat?.id ?? ''
  const podeContinuar = extrasAllowed(store, chat) && model.can(chatId, 'continue')
  const ocupado = model.controlBusy(session.id)
  const motivo = motivoEncerrada(session)
  const continuar = () => {
    setFalha(null)
    void model.control(chatId, session.id, 'continue').then((r) => {
      if (!alive.current) return
      if (r.ok) onOpen(r.session.id)
      else setFalha(r.falha)
    })
  }
  return (
    <div className="arquivar-encerrada idioma-superficie" {...superficieIdioma(idioma)}>
      {falha ? <AvisoFalha falha={falha} onDismiss={() => setFalha(null)} /> : null}
      <div className="session-composer-notice arquivar-aviso" role="status">
        <Icon name="archivebox" size={14} color="var(--context-ink-2, var(--ink-2))" />
        <span>
          <strong>{motivo === 'idle' ? t('conversa.arquivar.encerrada.idle') : motivo === 'owner' ? t('conversa.arquivar.encerrada.owner') : t('conversa.arquivar.encerrada.outro')}</strong>{' '}
          {t('conversa.arquivar.encerrada.leitura', { nome: botName })}
        </span>
        <button onClick={onBack}>{t('conversa.arquivar.voltar')}</button>
        {podeContinuar ? (
          <button className="arquivar-continuar" disabled={ocupado} aria-busy={ocupado || undefined} onClick={continuar}>
            {ocupado ? <><Spinner size={8} />{t('conversa.arquivar.continuando')}</> : t('conversa.arquivar.continuar')}
          </button>
        ) : null}
      </div>
    </div>
  )
}
