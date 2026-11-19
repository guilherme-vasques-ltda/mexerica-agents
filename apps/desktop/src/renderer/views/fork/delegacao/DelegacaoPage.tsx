// D371 (T1): Ajustes → Delegação, no grupo Pessoal. Só o Hub selecionado (senão o deste computador), como
// Armazenamento: o Hub decide quem é o dono e recusa os outros. Contrato: docs/features/delegacao.md.
// Entrega 4 (MX-7): a página explica quando, por que e como um bot delega, com um exemplo pronto para copiar.
import { useState } from 'react'
import { Button, CardForm, CardSection } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Text } from '../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { CampoRegras } from './CampoRegras'

const PASSOS = [
  { icone: 'clock', titulo: 'delegacao.como.quando.titulo', texto: 'delegacao.como.quando.texto' },
  { icone: 'lightbulb', titulo: 'delegacao.como.porque.titulo', texto: 'delegacao.como.porque.texto' },
  { icone: 'arrow.triangle.branch', titulo: 'delegacao.como.como.titulo', texto: 'delegacao.como.como.texto' },
] as const

export function DelegacaoPage() {
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const store = app.selectedStore ?? app.local
  const [copiado, setCopiado] = useState(false)
  const exemplo = t('delegacao.exemplo.texto')
  const copiar = () => void navigator.clipboard.writeText(exemplo).then(() => { setCopiado(true); setTimeout(() => setCopiado(false), 2000) }, () => {})
  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)}>
      <CardForm>
        <Text tone="secondary">{t('delegacao.pagina.lead')}</Text>
        <CardSection title={t('delegacao.como.titulo')}>
          {PASSOS.map((p) => (
            <div key={p.titulo} className="delegacao-passo">
              <span className="delegacao-passo-icone" aria-hidden="true"><Icon name={p.icone} size={14} /></span>
              <div><strong>{t(p.titulo)}</strong><p>{t(p.texto)}</p></div>
            </div>
          ))}
        </CardSection>
        <CardSection title={t('delegacao.exemplo.titulo')} footer={t('delegacao.exemplo.ajuda')}
          accessory={<Button kind="secondary" onClick={copiar}>{copiado ? t('delegacao.exemplo.copiado') : t('delegacao.exemplo.copiar')}</Button>}>
          <pre className="delegacao-exemplo" dir="auto">{exemplo}</pre>
        </CardSection>
        {/* S1 (D370, frente Sessões): o bloco "Modelos que os ajudantes podem usar" entra aqui, acima das regras. */}
        {store ? <Regras key={store.computer.id} source={store} /> : <Text tone="secondary">{t('delegacao.semConexao')}</Text>}
        <div className="delegacao-limite"><b>{t('delegacao.limite.titulo')}</b> {t('delegacao.limite.texto')}</div>
      </CardForm>
    </div>
  )
}

function Regras({ source }: { source: BotStore }) {
  const store = useModel(source)
  const t = useT()
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  if (!client) return <CardSection title={t('delegacao.regras.titulo')}><Text tone="secondary">{t('delegacao.semConexao')}</Text></CardSection>
  return (
    <CardSection>
      <CampoRegras key={`${store.hello?.hostId ?? ''}|${store.principalEpoch}`} guardarComo={`${store.computer.id}|${store.hello?.hostId ?? ''}|${store.principalEpoch}`} link={client} teamId={null} />
    </CardSection>
  )
}
