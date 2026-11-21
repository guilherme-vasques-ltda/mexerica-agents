import { CardForm } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { StoreContext, useApp } from '../../../store/context'
import { ApiDireta } from './ApiDireta'
import { ContasDoHub } from './ContasDoHub'
import { MotoresDoHub } from './MotoresDoHub'
import { useTextosContas } from './use-textos'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import './falha-de-conta.css'

/**
 * Configurações → Contas de IA: por Hub que o app conhece, as contas e, logo abaixo, os motores dele.
 * ajustes-tela-01: na tela de Ajustes, cada aba mostra uma das duas partes (`aba`); sem `aba`, as duas.
 */
/** O que é conta, motor e perfil antigo, em uma linha cada, ao lado do título (abre e fecha, no teclado também). */
export function GlossarioDeContas() {
  const t = useT()
  const idioma = useIdioma()
  return (
    <details className="contas-glossario" {...superficieIdioma(idioma)}>
      <summary>{t('contas.glossario.titulo')}</summary>
      <ul>
        <li>{t('contas.glossario.conta')}</li>
        <li>{t('contas.glossario.motor')}</li>
        <li>{t('contas.glossario.perfilAntigo')}</li>
      </ul>
    </details>
  )
}

export function ContasIaPage({ aba = null }: { aba?: 'contas' | 'motores' | null }) {
  const textos = useTextosContas()
  const app = useApp()
  const open = window.codync.appScheme === 'mexerica-open'
  const stores = app.computers.flatMap((c) => app.store(c.id) ?? [])
  return (
    <CardForm>
      {!open && aba !== 'motores' ? <GlossarioDeContas /> : null}
      {stores.length === 0 ? <Text tone="secondary">{textos.hubDesconectado}</Text> : null}
      {stores.map((store) => (
        <StoreContext.Provider key={store.computer.id} value={store}>
          {open || aba !== 'motores' ? <ContasDoHub /> : null}
          {open || aba !== 'motores' ? <ApiDireta /> : null}
          {!open && aba !== 'contas' ? <MotoresDoHub /> : null}
        </StoreContext.Provider>
      ))}
    </CardForm>
  )
}
