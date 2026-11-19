// ajustes-tela-01: a tela de Ajustes, no lugar da barra lateral e
// da conversa, como Rotinas (mesma camada, cabeçalho e placa: rotinas-central/routines-central.css). A janela
// fica montada e inerte por baixo (WithRoutinesScreen): rascunhos, editor do bot, space e chamada continuam.
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useEffect, useRef } from 'react'
import { Icon } from '../../../components/Icon'
import { useEscape } from '../../../components/Overlay'
import { useModel } from '../../../lib/observable'
import { SettingsView } from '../../settings/SettingsView'
import { settingsScreen } from './lugar-atual'
import { voltarDosAjustes } from './tela'
import './ajustes-tela.css'

export { settingsScreen }

export function AjustesTela() {
  const t = useT()
  const idioma = useIdioma()
  const tela = useModel(settingsScreen)
  const back = useRef<HTMLButtonElement>(null)
  // Como em Rotinas, o foco vai para Voltar ao abrir.
  useEffect(() => back.current?.focus({ preventScroll: true }), [])
  // Uma folha ou diálogo aberto dentro dos Ajustes entra depois na pilha e fecha antes; só então Esc volta.
  useEscape(tela.open, () => voltarDosAjustes(), back)
  if (!tela.page) return null
  return (
    <div className="rc-page aj-page" role="region" aria-label={t('ajustes.tela')}>
      <header className="rc-header drag idioma-superficie" {...superficieIdioma(idioma)}>
        <button ref={back} className="rc-back no-drag" aria-label={t('ajustes.voltar')} title={t('ajustes.voltarDica')} onClick={() => voltarDosAjustes()}>
          <Icon name="arrow.left" size={14} weight="medium" />
        </button>
        <h1 className="rc-title">{t('ajustes.tela')}</h1>
      </header>
      <SettingsView page={tela.page} aba={tela.aba} onPage={(page) => tela.setPage(page)} onAba={(aba) => tela.setAba(aba)} />
    </div>
  )
}
