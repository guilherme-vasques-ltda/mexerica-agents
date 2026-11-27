import { useEffect, useState } from 'react'
import { showsAccountControls } from '@shared/fork-features'
import type { EscolhaEntrada } from '@shared/fork-modo-entrada'
import { Button, CardSection, ValueRow } from '../../../components/Controls'
import { useT } from '../../../lib/fork-i18n/estado'
import { Superficie } from '../../settings/rotulo'

/**
 * Ajustes → Geral: o modo escolhido na entrada guiada. Quem ficou no 100% local passa para "com conta"
 * aqui; enquanto a versão não tem login, o botão fica desabilitado com "em breve". Some sem a ponte (web).
 */
export function ModoUsoAjusteView({ escolha, contaDisponivel, onEntrar }: {
  escolha: EscolhaEntrada | null
  contaDisponivel: boolean
  onEntrar: () => void
}) {
  const t = useT()
  const conta = escolha?.modo === 'conta'
  return (
    <Superficie>
      <CardSection title={t('onboarding.modoUso.titulo')}>
        <ValueRow
          label={t(conta ? 'onboarding.guia.modo.conta.titulo' : 'onboarding.guia.modo.local.titulo')}
          detail={conta ? t('onboarding.guia.modo.conta.desc') : `${t('onboarding.guia.modo.local.desc')} ${t('onboarding.modoUso.dicaLocal')}`}
        >
          {conta ? null : (
            <Button kind="secondary" disabled={!contaDisponivel} onClick={onEntrar}>
              {contaDisponivel ? t('onboarding.modoUso.entrar') : `${t('onboarding.modoUso.entrar')} · ${t('onboarding.guia.emBreve')}`}
            </Button>
          )}
        </ValueRow>
      </CardSection>
    </Superficie>
  )
}

export function ModoUsoAjuste() {
  const ponte = window.codync.app
  const [leitura, setLeitura] = useState<{ escolha: EscolhaEntrada | null } | null>(null)
  useEffect(() => {
    let ativo = true
    ponte.escolhaEntrada?.().then((lida) => { if (ativo) setLeitura({ escolha: lida }) }, () => {})
    return () => { ativo = false }
  }, [ponte])
  if (!ponte.escolhaEntrada || !leitura) return null
  const escolha = leitura.escolha
  const entrar = () => {
    const comConta: EscolhaEntrada = { modo: 'conta', infraestrutura: 'propria' }
    void ponte.gravarEscolhaEntrada?.(comConta).then((ok) => { if (ok) setLeitura({ escolha: comConta }) }, () => {})
  }
  return <ModoUsoAjusteView escolha={escolha} contaDisponivel={showsAccountControls()} onEntrar={entrar} />
}
