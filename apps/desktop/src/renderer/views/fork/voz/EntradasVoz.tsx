import { useEffect, useState } from 'react'
import { aceleradorDoEvento, rotuloAtalho } from '../../../../shared/fork-voz-entrada.ts'
import { Button, CardSection, Switch } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { usePref } from '../../../lib/prefs'
import { atalhoVoz, estadoAtalho } from './entrada-voz'

const plataforma = window.codync.platform

const ERRO = {
  invalido: 'voz.ajustes.entrada.atalho.erro.invalido',
  reservado: 'voz.ajustes.entrada.atalho.erro.reservado',
  'em-uso': 'voz.ajustes.entrada.atalho.erro.emUso',
  indisponivel: 'voz.ajustes.entrada.atalho.erro.indisponivel',
} as const

/**
 * Ajustes → Voz → Ao vivo. O atalho é desligado de fábrica e sem combinação padrão.
 */
export function EntradasVoz() {
  const t = useT()
  const [{ ativo, acelerador }, guardar] = usePref(atalhoVoz)
  const resultado = useModel(estadoAtalho).resultado
  const [gravando, setGravando] = useState(false)
  const [recusada, setRecusada] = useState<keyof typeof ERRO | null>(null)

  useEffect(() => { if (!ativo) setGravando(false) }, [ativo])

  useEffect(() => {
    if (!gravando) return
    const aperta = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape') return setGravando(false)
      const combinacao = aceleradorDoEvento(e, plataforma)
      if (!combinacao) return
      setGravando(false)
      guardar({ ativo: true, acelerador: combinacao })
    }
    window.addEventListener('keydown', aperta, true)
    return () => window.removeEventListener('keydown', aperta, true)
  }, [gravando, guardar])

  useEffect(() => setRecusada(resultado && !resultado.ok ? resultado.motivo : null), [resultado])

  return (
    <CardSection title={t('voz.ajustes.entrada.titulo')} footer={t('voz.ajustes.entrada.rodape')}>
      {[
        <div key="atalho" className="settings-row">
          <Switch on={ativo} onChange={(on) => guardar({ ativo: on, acelerador })}>{t('voz.ajustes.entrada.atalho')}</Switch>
        </div>,
        ativo ? (
          <div key="combinacao" className="settings-row">
            <Text tone="secondary">{t('voz.ajustes.entrada.atalho.descricao')}</Text>
            <Text tone="text" aria-label={t('voz.ajustes.entrada.atalho.combinacao')}>{acelerador ? rotuloAtalho(acelerador, plataforma) : '—'}</Text>
            <Button kind="secondary" onClick={() => setGravando(true)}>{t('voz.ajustes.entrada.atalho.escolher')}</Button>
            <Text as="div" tone={recusada ? 'danger' : 'secondary'} role="status" dir="auto">
              {gravando ? t('voz.ajustes.entrada.atalho.gravando')
                : recusada ? t(ERRO[recusada])
                : !acelerador ? t('voz.ajustes.entrada.atalho.semCombinacao')
                : resultado?.ok ? t('voz.ajustes.entrada.atalho.ativo', { combinacao: rotuloAtalho(resultado.acelerador ?? acelerador, plataforma) })
                : ''}
            </Text>
          </div>
        ) : null,
      ]}
    </CardSection>
  )
}
