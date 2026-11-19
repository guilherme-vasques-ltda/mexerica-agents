import { useEffect, useRef, useState } from 'react'
import type { PublicMaterial } from '../../../../shared/fork-workplaces'
import { Button } from '../../../components/Controls'
import { ModalHeader, Sheet, SheetPage } from '../../../components/Overlay'
import type { Workplaces } from '../../../store/fork-workplaces'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { ltr } from '../../../../shared/bidi.ts'
import { GuideReading, type GuideContent } from './guia-local'

/** O renderer recebe e copia exclusivamente as partes públicas. */
export function ChavesPublicas({ material }: { material: PublicMaterial }) {
  const t = useT()
  const [copied, setCopied] = useState<string | null>(null)
  const [error, setError] = useState(false)
  const copy = async (name: string, value: string) => {
    try { await navigator.clipboard.writeText(value); setCopied(name); setError(false) }
    catch { setError(true) }
  }
  return <div className="lugares-publicas">
    {([['lugares.guia.chaveAcesso', material.sshPublic], ['lugares.guia.chaveControle', material.controlPublic],
      ['lugares.guia.idHub', material.hubId], ['lugares.guia.idLugar', material.placeId]] as const).map(([name, value]) => <div key={name}>
        <label>{t(name)}</label><code className="selectable" dir="ltr">{value}</code>
        <Button kind="secondary" onClick={() => void copy(name, value)}>{copied === name ? t('lugares.guia.copiado') : t('lugares.guia.copiar')}</Button>
      </div>)}
    {error && <p role="alert">{t('lugares.guia.erroCopiar')}</p>}
  </div>
}
export function GuiaPreparo({ model }: { model: Workplaces }) {
  const t = useT()
  const idioma = useIdioma()
  const [guideOpen, setGuideOpen] = useState(false)
  const [content, setContent] = useState<GuideContent>({ kind: 'loading' })
  const reading = useRef(new GuideReading())
  const closeGuide = () => { reading.current.cancel(); setGuideOpen(false); setContent({ kind: 'loading' }) }
  useEffect(() => {
    closeGuide()
    return () => reading.current.cancel()
  }, [model, model.generation])
  const openGuide = () => {
    setGuideOpen(true)
    setContent({ kind: 'loading' })
    void reading.current.read(model, setContent)
  }
  return <>
    <section className="lugares-passo lugares-preparo">
      <h3>{t('lugares.guia.passo3')}</h3>
      <p>{t('lugares.guia.porVoce')}</p>
      <Button kind="secondary" onClick={openGuide} disabled={!model.available}>{t('lugares.guia.abrir')}</Button>
      <p>{t('lugares.guia.mesmaVersao')}</p>
    </section>
    <Sheet language={superficieIdioma(idioma)} open={guideOpen} onClose={closeGuide} width={680} height={580} maxHeight="calc(100vh - 80px)" label={t('lugares.guia.rotulo')}>
      <SheetPage className="lugares-guia">
        {/* O portal e o conteúdo seguem o idioma da tela. O roteiro é do Hub e conserva a própria direção. */}
        <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
        <ModalHeader title={t('lugares.guia.titulo')} subtitle={content.kind === 'ready' ? t('lugares.guia.versao', { versao: ltr(content.guide.hubVersion) }) : t('lugares.guia.publico')} onClose={closeGuide} closeTitle={t('lugares.fechar')} />
        <div className="lugares-guia-body">
          {content.kind === 'loading' ? <p role="status">{t('lugares.guia.carregando')}</p>
            : content.kind === 'error' ? <p role="alert">{t('lugares.guia.erro')}</p>
              : <pre className="lugares-guia-text" dir="auto" tabIndex={0} aria-label={t('lugares.guia.roteiro')}>{content.guide.text}</pre>}
        </div>
        </div>
      </SheetPage>
    </Sheet>
  </>
}
