import { useEffect, useRef, useState } from 'react'
import type { Workplaces } from '../../../store/fork-workplaces'
import { useT } from '../../../lib/fork-i18n/estado'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import { chaveCadastro } from './chaves'

/** Formato técnico do ID da Boat, igual em todo idioma. */
const FORMATO_ID = 'bx_…' // textos-ok: formato técnico do ID (código), não é texto de interface

export function CadastrarLugar({ model, hubId }: { model: Workplaces; hubId: string }) {
  const t = useT()
  // Nome e recurso digitados: ficam no pai, por Hub. Um formulário nunca mostra o texto de outro Hub (estado-pagina.ts).
  const [name, setName] = useEstadoDaPagina(chaveCadastro(hubId, 'nome'), '')
  const [resource, setResource] = useEstadoDaPagina(chaveCadastro(hubId, 'recurso'), '')
  const [saved, setSaved] = useState(false)
  const mounted = useRef(false)
  const draftRevision = useRef(0)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  return <form className="lugares-form" onSubmit={(event) => {
    event.preventDefault()
    const generation = model.generation
    const revision = draftRevision.current
    setSaved(false)
    void model.mutate({ type: 'create', name: name.trim(), resource_id: resource.trim() }, null).then((reply) => {
      // A lista recebe a resposta válida pelo store. Nenhuma resposta escolhe
      // outro lugar; sair e voltar ao cadastro cria outra montagem do formulário.
      if (!reply?.place || !mounted.current || model.generation !== generation || !model.available) return
      if (draftRevision.current === revision) { setName(''); setResource('') }
      setSaved(true)
    })
  }}>
    <h3>{t('lugares.cad.titulo')}</h3>
    <p>{t('lugares.cad.texto')}</p>
    <label>{t('lugares.cad.nome')}<input autoFocus dir="auto" value={name} maxLength={120} required onChange={(e) => { draftRevision.current++; setName(e.target.value) }} autoComplete="off" /></label>
    <label>{t('lugares.cad.id')}<input dir="ltr" value={resource} required pattern="bx_[A-Za-z0-9_]+" maxLength={100} onChange={(e) => { draftRevision.current++; setResource(e.target.value) }} placeholder={FORMATO_ID} autoComplete="off" spellCheck={false} /></label>
    <p>{t('lugares.cad.idNaoMuda')}</p>
    <button className="styled-button primary" type="submit" disabled={model.busy}>{t('lugares.cad.salvar')}</button>
    {saved && <p role="status">{t('lugares.cad.salvo')}</p>}
  </form>
}
