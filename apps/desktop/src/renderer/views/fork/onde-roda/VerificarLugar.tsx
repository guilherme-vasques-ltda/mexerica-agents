import { useEffect } from 'react'
import type { Place } from '../../../../shared/fork-workplaces'
import type { Workplaces } from '../../../store/fork-workplaces'
import { Button, Spinner } from '../../../components/Controls'
import { localeDe } from '../../../lib/fork-i18n/index.ts'
import { useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'

/** Os estados conhecidos têm rótulo no catálogo (`lugares.ver.rotulo.*`); outro valor do Hub fica como "Não verificado". */
const ESTADOS = ['nao_verificado', 'conferido', 'indisponivel', 'assinatura_oficial', 'api_nao_admitida', 'desconectado', 'verificando', 'concluida', 'falhou', 'cancelada_incerta', 'interrompida_incerta'] as const
const rotulo = (t: Tradutor, v: string | undefined) => {
  const e = ESTADOS.find((x) => x === v) ?? 'nao_verificado'
  return t(`lugares.ver.rotulo.${e}`)
}
export function VerificarLugar({ model, place }: { model: Workplaces; place: Place }) {
  const t = useT()
  const idioma = useIdioma()
  const operation = model.operationFor(place.id)
  const active = operation?.phase === 'verificando'
  useEffect(() => {
    if (!operation || operation.phase !== 'verificando') return
    const timer = setInterval(() => { void model.status(operation) }, 1000)
    return () => clearInterval(timer)
  }, [model, operation?.id, operation?.phase])
  const observed = operation?.observation
  return <section className="lugares-passo lugares-verificar" aria-label={t('lugares.ver.secao')}>
    <h3>{t('lugares.ver.titulo')} {active && <Spinner size={12} />}</h3>
    <p role="status" aria-live="polite">{operation ? rotulo(t, operation.phase) : t('lugares.ver.aindaNao')}</p>
    <dl className="lugares-eixos">
      {([['conexao', observed?.connection], ['identidade', observed?.identity], ['preparo', observed?.preparation], ['conta', observed?.account], ['ajudantes', 'indisponivel']] as const)
        .map(([eixo, value]) => <div key={eixo}><dt>{t(`lugares.ver.eixo.${eixo}`)}</dt><dd>{rotulo(t, value)}</dd></div>)}
    </dl>
    {operation?.observedAt && <p>{t('lugares.ver.observado', { quando: new Date(operation.observedAt).toLocaleString(localeDe(idioma)) })}</p>}
    {active ? <Button kind="secondary" disabled={model.busy} onClick={() => void model.cancel(operation)}>{t('lugares.ver.cancelar')}</Button>
      : <Button disabled={model.busy || !place.prepared || !place.pinned || !place.connected} onClick={() => void model.mutate({ type: 'inspect_start' }, place)}>{t('lugares.ver.verificar')}</Button>}
    {active && <p>{t('lugares.ver.cancelarIncerto')}</p>}
    {!operation && <p>{t('lugares.ver.explica')}</p>}
  </section>
}
