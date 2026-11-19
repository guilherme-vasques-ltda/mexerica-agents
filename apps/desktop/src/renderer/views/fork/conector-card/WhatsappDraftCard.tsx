import { useEffect, useState } from 'react'
import type { Entry } from '@shared/models'
import { Icon } from '../../../components/Icon'
import { Spinner } from '../../../components/Controls'
import { useStoreRef } from '../../../store/context'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { Mensagem } from '../bot-settings/mensagem'
import { erroDoHub } from '../whatsapp/regras'
import { lerRascunho, telaRascunho, type RascunhoWhatsapp } from './rascunho'
import '../../thread/cards/approval-card.css'
import './conector-card.css'

/** Não decisível pelo protocolo: a decisão é o gesto do dono no cartão (MX-18). */
export function WhatsappDraftCard({ entry }: { entry: Entry }) {
  const store = useStoreRef()
  const t = useT()
  const idioma = useIdioma()
  const draft = entry.data.conectorDraft!
  const [presentation, setPresentation] = useState<RascunhoWhatsapp | null>(null)
  const [now, setNow] = useState(Date.now)
  const [failed, setFailed] = useState(false)
  // MX-18: "Enviar" enfileira o rascunho no Hub; "Agora não" só esconde o cartão (o rascunho vence sozinho).
  const [fase, setFase] = useState<'ocioso' | 'enviando' | 'enviado'>('ocioso')
  const [agoraNao, setAgoraNao] = useState(false)
  const [erro, setErro] = useState<Mensagem | null>(null)
  useEffect(() => {
    let alive = true
    setPresentation(null)
    setFailed(false)
    void (async () => {
      try {
        const raw = await (await store.ready()).call<unknown>('donnaConectorRascunho', { draftId: draft.draftId })
        if (alive) setPresentation(lerRascunho(raw, draft.draftId, Date.now()))
      } catch {
        if (alive) setFailed(true)
      }
    })()
    return () => { alive = false }
  }, [store, draft.draftId, entry.rev])
  useEffect(() => {
    const timer = setTimeout(() => { setNow(Date.now()); setPresentation(null) }, Math.max(0, draft.deadline - Date.now() + 10))
    return () => clearTimeout(timer)
  }, [draft.deadline])
  const view = telaRascunho(draft, failed, now, presentation)
  const { closed, visible } = view
  const enviar = () => {
    if (fase !== 'ocioso') return
    setFase('enviando')
    setErro(null)
    void (async () => {
      try {
        await (await store.ready()).call('donnaWhatsappSendDraft', { draftId: draft.draftId })
        setFase('enviado')
      } catch (e) {
        setErro(erroDoHub(e))
        setFase('ocioso')
      }
    })()
  }
  return (
    <section className="approval-card conector-card idioma-superficie" {...superficieIdioma(idioma)} aria-label={t('whatsapp.conversa.rascunho.titulo')}>
      <h3 className="approval-title">{t('whatsapp.conversa.rascunho.titulo')}</h3>
      {fase === 'enviado' ? (
        <p className="approval-note"><Icon name="checkmark.circle" size={13} />{t('whatsapp.conversa.rascunho.naFila')}</p>
      ) : agoraNao ? (
        <p className="approval-note">{t('whatsapp.conversa.rascunho.encerrado')}</p>
      ) : (
        <>
          <p className="approval-note">{t(closed ? 'whatsapp.conversa.rascunho.encerrado' : 'whatsapp.conversa.rascunho.revisao')}</p>
          {visible ? <dl className="approval-facts conector-card-fatos">
            <dt>{t('whatsapp.conversa.rascunho.conta')}</dt><dd><bdi>{visible.account}</bdi></dd>
            <dt>{t('whatsapp.conversa.rascunho.respostaA')}</dt>
            <dd><bdi>{visible.replyTo.sender}</bdi><blockquote dir="auto">{visible.replyTo.excerpt}</blockquote>
              <span>{t('whatsapp.conversa.rascunho.externo')}</span></dd>
            <dt>{t('whatsapp.conversa.rascunho.texto')}</dt><dd className="selectable conector-card-valor" dir="auto">{visible.text}</dd>
          </dl> : !closed ? <p className="approval-note">{t(view.failed ? 'whatsapp.conversa.rascunho.erro' : 'whatsapp.conversa.rascunho.semPrivado')}</p> : null}
          {visible ? (
            <>
              <div className="approval-actions">
                <button className="hold-button primary" disabled={fase === 'enviando'} onClick={enviar}>
                  <span className="hold-button-label"><Icon name="arrow.up" size={13} weight="bold" />{fase === 'enviando' ? <Spinner size={12} /> : t('whatsapp.conversa.rascunho.enviar')}</span>
                </button>
                <span className="conector-card-espaco" />
                <button className="approval-deny" disabled={fase === 'enviando'} onClick={() => setAgoraNao(true)}>{t('whatsapp.conversa.rascunho.agoraNao')}</button>
              </div>
              <p className="approval-note"><Icon name="clock" size={13} />{t('whatsapp.conversa.rascunho.envioNota')}</p>
              {erro ? <p className="approval-note conector-card-erro" role="alert"><Icon name="exclamationmark.circle" size={13} />{'texto' in erro ? <bdi dir="auto">{erro.texto}</bdi> : t('whatsapp.conversa.rascunho.erroEnviar')}</p> : null}
            </>
          ) : null}
        </>
      )}
    </section>
  )
}
