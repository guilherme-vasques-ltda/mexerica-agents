import { useState } from 'react'
import { SpaceBirth } from './SpaceBirth'
import { SpaceDelete } from './SpaceDelete'
import { SpacePrincipal } from './SpacePrincipal'
import { SpacePrincipalLook } from './SpacePrincipalLook'
import type { SpaceDraft } from './SpaceIconPicker'
import { Button } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { railSpaces, spaceLifecycleOk } from '../../../lib/fork-spaces-rail'
import { updateSpace, type SpacesOrigin } from '../../../store/fork-spaces'
import { SpaceIconPicker } from './SpaceIconPicker'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'

// D169 (components/SpaceIconPicker, SpaceRail): the picker in a sheet over the rail's Hub, for a new space or the
// one being edited. It keeps the origin it opened on (Hub, link, generation and list) for its whole life and shows
// that list, so the draft never shifts under the person; saving goes to that Hub at that revision
// (store/fork-spaces), and if any of it moved meanwhile nothing is sent and the sheet asks to be opened again. A
// refusal stays on screen with the draft, a success closes it. A space no longer in the list is said so, never
// turned into a new one.
// D543 E1: a Space sem principal oferece "Ativar o Berga"; todo Space (nunca o Pessoal, que não abre esta folha)
// oferece "Excluir Space", com o nome digitado. As duas ações trocam a folha inteira, como o Birth.

export function SpaceSheet({ origin, id, initialMode = 'pick', onClose, onDeleted }: { origin: SpacesOrigin; id: string | null; initialMode?: 'pick' | 'principal' | 'delete'; onClose: () => void; onDeleted?: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const [birth, setBirth] = useState<SpaceDraft | null>(null)
  const [draft, setDraft] = useState<SpaceDraft | null>(null)
  const [mode, setMode] = useState<'pick' | 'principal' | 'delete' | 'look'>(initialMode)
  const list = origin.snapshot
  const editing = id ? list.spaces.find((s) => s.id === id && s.kind === 'space') ?? null : null
  if (id && !editing?.tile) {
    return (
      <div className="create-sheet idioma-superficie" {...superficieIdioma(idioma)}>
        <Text role="alert" tone="danger">{t('spaces.sheet.sumiu')}</Text>
        <Button kind="secondary" onClick={onClose}>{t('spaces.sheet.fechar')}</Button>
      </div>
    )
  }
  const owner = list.spaces.find((s) => s.id === list.personalSpaceId)?.owner ?? null
  const neighbors = railSpaces(list).filter((s) => s.id !== id).flatMap((s) => (s.tile ? [s.tile] : []))
  if (!id && birth) return <div className="idioma-superficie" {...superficieIdioma(idioma)}><SpaceBirth origin={origin} draft={birth} onBack={() => { setDraft(birth); setBirth(null) }} onClose={onClose} /></div>
  if (id && editing && mode === 'principal') return <div className="idioma-superficie" {...superficieIdioma(idioma)}><SpacePrincipal origin={origin} space={editing} onBack={() => setMode('pick')} onClose={onClose} /></div>
  if (id && editing && mode === 'delete') return <div className="idioma-superficie" {...superficieIdioma(idioma)}><SpaceDelete origin={origin} space={editing} onBack={() => setMode('pick')} onClose={onClose} onDeleted={() => { onDeleted?.(); onClose() }} /></div>
  if (id && editing && mode === 'look') return <div className="idioma-superficie" {...superficieIdioma(idioma)}><SpacePrincipalLook origin={origin} space={editing} onBack={() => setMode('pick')} onClose={onClose} /></div>
  return (
    <SpaceIconPicker
      initial={editing?.tile ? { name: editing.name, tile: editing.tile } : draft}
      saveLabel={id ? undefined : t('spaces.birth.continuar')}
      owner={owner}
      neighbors={neighbors}
      onCancel={onClose}
      zone={id && editing ? (
        <div className="space-zone">
          {!spaceLifecycleOk(origin.store.hello) ? (
            <p className="space-zone-note" role="status">{t('spaces.sheet.atualize')}</p>
          ) : null}
          {!editing.principalBotId ? (
            <div className="space-zone-row">
              <Text as="div" size="callout" tone="secondary">{t('spaces.sheet.semPrincipal')}</Text>
              <Button kind="secondary" disabled={!spaceLifecycleOk(origin.store.hello)} onClick={() => setMode('principal')}>{t('spaces.sheet.ativar')}</Button>
            </div>
          ) : (
            <div className="space-zone-row">
              <Text as="div" size="callout" tone="secondary">{t('spaces.sheet.aparencia')}</Text>
              <Button kind="secondary" disabled={!spaceLifecycleOk(origin.store.hello)} onClick={() => setMode('look')}>{t('spaces.sheet.trocarAparencia')}</Button>
            </div>
          )}
          <div className="space-zone-row space-zone-danger">
            <Button kind="secondary" className="space-zone-delete" disabled={!spaceLifecycleOk(origin.store.hello)} onClick={() => setMode('delete')}>{t('spaces.sheet.excluir')}</Button>
          </div>
        </div>
      ) : undefined}
      onSave={async (draft) => {
        if (id) await updateSpace(origin, id, draft)
        else { setBirth(draft); return }
        onClose()
      }}
    />
  )
}
