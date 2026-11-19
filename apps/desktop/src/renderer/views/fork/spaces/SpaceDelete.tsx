import { SpaceClient } from '@shared/fork-spaces-client'
import { useEffect, useState } from 'react'
import type { Space } from '@shared/fork-spaces'
import { Button } from '../../../components/Controls'
import { deleteSpace, type SpacesOrigin } from '../../../store/fork-spaces'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { cru, ErroFrase, frase, textoDe, type Frase } from '../../../lib/fork-i18n/frase'
import { spaceDeleteReady, spaceLifecycleOk } from '../../../lib/fork-spaces-rail'
import './spaces.css'

// D543 E1: exclusão definitiva de Space, só no aplicativo do computador (o Hub também
// exige). O dono digita o nome do Space; bots ocupados voltam só com os nomes.

export function SpaceDelete({ origin, space, onBack, onClose, onDeleted }: { origin: SpacesOrigin; space: Space; onBack: () => void; onClose: () => void; onDeleted: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const [count, setCount] = useState<number | null>(null)
  const [typed, setTyped] = useState('')
  const [saving, setSaving] = useState(false)
  const [problem, setProblem] = useState<Frase | null>(null)
  const [busy, setBusy] = useState<string[] | null>(null)
  const supported = spaceLifecycleOk(origin.store.hello)
  useEffect(() => {
    if (!supported) return
    const client = new SpaceClient(origin.hostId, space.id, origin.client)
    let live = true
    void client.conversations().then((result) => {
      if (live) setCount(new Set(result.conversations.filter((c) => c.id === c.botId).map((c) => c.botId)).size)
    }, () => { if (live) setProblem(frase('spaces.delete.falhou')) })
    return () => { live = false; client.dispose() }
  }, [origin, space.id, supported])
  const ready = count !== null && spaceDeleteReady(space.name, typed, supported) && !saving
  const save = async () => {
    if (!ready) return
    setSaving(true)
    setProblem(null)
    try {
      const result = await deleteSpace(origin, space.id, typed)
      if ('busy' in result) { setBusy(result.busy.map((b) => b.name)); return }
      onDeleted()
    } catch (error) {
      setProblem(error instanceof ErroFrase ? error.frase : error instanceof Error && error.message ? cru(error.message) : frase('spaces.delete.falhou'))
    } finally { setSaving(false) }
  }
  return <section className="space-birth space-delete idioma-superficie" aria-label={t('spaces.delete.titulo')} {...superficieIdioma(idioma)}>
    <div className="space-birth-heading"><div><h2>{t('spaces.delete.titulo')}</h2><p dir="auto">{space.name}</p></div></div>
    <p>{t('spaces.delete.aviso')}</p>
    <p>{t('spaces.delete.itens', { quantidade: count ?? '…' })}</p>
    <p>{t('spaces.delete.preservado')}</p>
    <p>{t('spaces.delete.copias')}</p>
    {!supported ? <p role="status">{t('spaces.delete.atualize')}</p> : null}
    {busy ? (
      <p role="alert" className="space-delete-busy">{tx(frase('spaces.delete.trabalhando', { nomes: busy.join(', ') }))}</p>
    ) : null}
    <fieldset disabled={saving}>
      <label>{t('spaces.delete.digite', { nome: space.name })}<input value={typed} disabled={!supported} onChange={(e) => { setTyped(e.target.value); setBusy(null) }} dir="auto" /></label>
    </fieldset>
    {problem ? <p role="alert">{tx(problem)}</p> : null}
    <footer>
      <Button kind="secondary" disabled={saving} onClick={onBack}>{t('spaces.birth.voltar')}</Button>
      <button className="styled-button secondary" disabled={saving} onClick={onClose} autoFocus>{t('spaces.birth.cancelar')}</button>
      <Button disabled={!ready} onClick={() => void save()} className="space-delete-confirm">{t(saving ? 'spaces.delete.excluindo' : 'spaces.delete.botao')}</Button>
    </footer>
  </section>
}
