// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useState } from 'react'
import { canKeepConversation, type Entry, type HandoffCard } from '@shared/models'
import { Button, CardForm, CardSection, Disclosure, IconButton, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ModalHeader, Sheet, SheetPage } from '../../components/Overlay'
import { Text } from '../../components/Text'
import { useStore as useLiveStore } from '../../store/context'
import { Field } from '../marketplace/pieces'
import './handoff.css'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'

function Line({ icon, text }: { icon: string; text?: string }) {
  return (
    <span className="handoff-line">
      <Icon name={icon} size={11} />
      <span>{text}</span>
    </span>
  )
}

/** The bot's chat starts over: what the new conversation carries, which the user can edit, or turn down to carry on. */
export function HandoffNotice({ entry, card }: { entry: Entry; card: HandoffCard }) {
  const store = useLiveStore()
  const [expanded, setExpanded] = useState(false)
  const [draft, setDraft] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const t = useT()
  const idioma = useIdioma()
  const carried = <pre className="handoff-text handoff-carried">{card.text}</pre>
  if (card.status !== 'draft') {
    return (
      <div className="handoff-folded">
        <Disclosure expanded={expanded} onToggle={() => setExpanded(!expanded)} label={<Line icon="arrow.triangle.branch" text={entry.data.text} />}>
          {carried}
        </Disclosure>
      </div>
    )
  }
  const save = async () => {
    if (draft === null || !draft.trim()) return
    setSaving(true)
    if (await store.editHandoff(entry, draft.trim())) setDraft(null)
    setSaving(false)
  }
  return (
    <div className="handoff-card">
      <Line icon="arrow.triangle.branch" text={entry.data.text} />
      <Disclosure expanded={expanded} onToggle={() => setExpanded(!expanded)} label={<Text size="footnote" weight="medium">{t('conversa.avisos.passagem.leva')}</Text>}>
        {carried}
      </Disclosure>
      <div className="handoff-actions">
        {store.answering.has(entry.id) ? <Spinner /> : <>
          {/* DON-907 C9a e Sessões PR5: troca de motor, continuação e recomeço não têm conversa antiga para manter. */}
          {canKeepConversation(card) ? <Button kind="secondary" onClick={() => store.keepConversation(entry)}>{t('conversa.avisos.passagem.manter')}</Button> : null}
          <IconButton title={t('conversa.avisos.passagem.editarLeva')} icon="pencil" onClick={() => setDraft(card.text ?? '')} />
        </>}
      </div>
      <Sheet language={superficieIdioma(idioma)} open={draft !== null} onClose={() => setDraft(null)} width={620}>
        {draft !== null && <SheetPage>
          <ModalHeader closeTitle={t('conversa.fechar')}
            title={t('conversa.avisos.passagem.editar')}
            trailing={saving ? <Spinner /> : <IconButton title={t('conversa.avisos.passagem.salvar')} icon="checkmark" disabled={!draft.trim()} onClick={() => { void save() }} />}
          />
          <CardForm>
            <CardSection footer={t('conversa.avisos.passagem.rodape')}>
              <Field value={draft} onChange={setDraft} label={t('conversa.avisos.passagem.rotulo')} lines={[10, 24]} mono />
            </CardSection>
          </CardForm>
        </SheetPage>}
      </Sheet>
    </div>
  )
}
