import { Dialog } from '../../../components/Overlay'
import { idiomaDoDocumento, superficieIdioma, tradutor } from '../../../lib/fork-i18n/index.ts'
import { useAccount } from '../../../store/account'
import './account-exit.css'

/**
 * Web: o Sair apaga as chaves do aparelho no navegador (decisão do dono, 16/11/2026), então a pessoa lê o que
 * acontece ANTES de sair. Um diálogo só, montado no portão da conta, vale para o menu da conta, Ajustes e a
 * recuperação de saída. Cancelar recebe o foco e Esc cancela (Dialog). O idioma é o do documento:
 * o portão não lê preferências.
 */
export function ConfirmarSairWeb() {
  const account = useAccount()
  const idioma = idiomaDoDocumento()
  const t = tradutor(idioma)
  return (
    <Dialog
      open={account.confirmingSignOut}
      title={t('conta.web.sair.titulo')}
      message={
        <div className="sair-aviso">
          <p>{t('conta.web.sair.oque')}</p>
          <p>{t('conta.web.sair.proxima')}</p>
          <p className="sair-aviso-fica">{t('conta.web.sair.fica')}</p>
        </div>
      }
      cancel={t('conta.cancelar')}
      language={superficieIdioma(idioma)}
      actions={[{ title: t('conta.web.sair.confirmar'), action: () => void account.signOut() }]}
      onClose={() => account.cancelSignOut()}
    />
  )
}
