import { useEffect, useState } from 'react'
import { Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useApp, useStore } from '../../../store/context'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { reconectarSSH } from '../../settings/ssh-model'
import './faixa-reconectando.css'

const TENTANDO_MS = 3000

/**
 * MX-217: no lugar do selo "Desconectado", a faixa discreta de uma queda curta. A conversa, o cache e o rascunho
 * ficam; a reconexão é automática (com espera crescente). "Tentar agora" (SSH e Hub deste computador) reabre o túnel
 * já e reconecta só este computador.
 */
export function FaixaReconectando() {
  const app = useApp()
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const [tentando, setTentando] = useState(false)
  useEffect(() => {
    if (!tentando) return
    const timer = setTimeout(() => setTentando(false), TENTANDO_MS)
    return () => clearTimeout(timer)
  }, [tentando])
  const tentar = () => {
    setTentando(true)
    reconectarSSH(store.computer.id)
    for (const loja of app.spaceMirrors.doComputador(store.computer.id)) loja.restartStream()
  }
  return (
    <div className="ctabs-connection faixa-reconectando idioma-superficie" role="status" title={t('conversa.mensagens.queda.detalhe')} {...superficieIdioma(idioma)}>
      <Icon name="wifi.slash" size={11} weight="medium" />
      <span className="faixa-reconectando-texto">{t('conversa.mensagens.queda.titulo')}</span>
      <span className="faixa-reconectando-detalhe">{t('conversa.mensagens.queda.detalhe')}</span>
      {/* Pela nuvem, o Main já reconecta sozinho e não aceita antecipar: sem botão que não faria nada. */}
      {store.isLoopback ? (
        <button className="faixa-reconectando-tentar" disabled={tentando} onClick={tentar}>
          {tentando ? <Spinner size={8} /> : null}
          {t(tentando ? 'conversa.mensagens.queda.tentando' : 'conversa.mensagens.queda.tentar')}
        </button>
      ) : null}
    </div>
  )
}
