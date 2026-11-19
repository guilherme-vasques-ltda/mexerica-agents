import { useRef, useState } from 'react'
import { Button, CardSection, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useAccount } from '../../../store/account'
import { useApp } from '../../../store/context'
import { AccountExitLive, AccountExitPanel, useFocusReturn } from './AccountExitPanel'
import { offersRegistrationRetry, registrationCopy, registrationLabel, retryDeviceRegistration } from './device-registration'
import { aparelhoDe, revokedCopy } from './exit-copy'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import './account-exit.css'
import { AccountProfileSections } from './AccountProfile'
import { capacidades } from '@shared/fork-capacidades'
import { mostraPerfilConta } from '../ajustes/gates-capacidades'

/**
 * Ajustes → Geral → Conta (só com a nuvem ligada). Com a conta confirmada, os cartões de perfil, senha,
 * contas ligadas e verificação em duas etapas vêm abaixo (AccountProfile.tsx, E6b). Uma saída não confirmada ocupa o lugar do login
 * normal até a conta confirmá-la; a mensagem do processo principal fica para os demais erros. Com a
 * sessão normal, uma falha no registro deste aparelho pode ser retomada por clique, e as confirmações
 * antigas da conta aparecem abaixo da linha da conta, sem bloqueá-la.
 */
export function AccountSettingsCard() {
  const account = useAccount()
  const cloud = useModel(useApp().cloud)
  const aparelho = aparelhoDe(window.codync.platform)
  const idioma = useIdioma()
  const t = useT()
  // A retomada vale para a sessão em que foi pedida: outra sessão começa sem ela.
  const [retriedIn, setRetriedIn] = useState<number | null>(null)
  const retried = retriedIn === account.sessionRevision
  const unresolved = account.isExitUnresolved
  const revoked = account.exit === 'revoked' && !account.isSignedIn
  const registration = account.isSignedIn && !unresolved ? registrationCopy(cloud.deviceRegistration, aparelho, retried, idioma) : null
  // A mensagem do processo principal chega pronta (fatia 10): isolada com a direção dela, dentro da superfície do cartão.
  const footer = unresolved ? null : account.errorMessage ? <span dir="auto">{account.errorMessage}</span> : registration?.footer ?? (revoked ? revokedCopy(idioma).hint : null)
  const retry = () => {
    setRetriedIn(account.sessionRevision)
    void retryDeviceRegistration(cloud, account)
  }
  return (
    <>
    {/* R1-02 do #596: a superfície cobre o cartão inteiro (título e rodapé do CardSection), sem criar caixa. */}
    <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
    <CardSection title={t('conta.titulo')} footer={footer}>
      <div className="account-card">
        {unresolved ? (
          <AccountExitPanel surface="settings" />
        ) : (
          <>
            <AccountRow revoked={revoked} registrationDetail={registration?.detail ?? null} retry={account.isSignedIn && offersRegistrationRetry(cloud.deviceRegistration, retried) ? retry : null} />
          </>
        )}
        <AccountExitLive fallback={registration?.announcement ?? ''} />
      </div>
    </CardSection>
    </div>
    {/* E6b: nome, senha, contas ligadas e verificação em duas etapas, só com a conta confirmada e backend de perfil (na Web a ponte devolveria `webUnavailable`). */}
    {mostraPerfilConta(capacidades(window.codync), account) ? <AccountProfileSections key={account.user?.userId} /> : null}
    </>
  )
}

function AccountRow({ revoked, registrationDetail, retry }: { revoked: boolean; registrationDetail: string | null; retry: (() => void) | null }) {
  const account = useAccount()
  const cloud = useModel(useApp().cloud)
  const idle = account.canAct
  const registering = cloud.deviceRegistration === 'registering'
  const row = useRef<HTMLDivElement>(null)
  const holdFocus = useFocusReturn(row, idle && !registering, 'button:not(:disabled)')
  const t = useT()
  const idioma = useIdioma()
  const [title, detail] = account.isSigningOut
    ? [t('conta.linha.saindo'), t('conta.linha.limpando')]
    : account.isSignedIn
      ? [account.user?.email ?? t('conta.linha.sessaoIniciada'), registrationDetail ?? t('conta.linha.acesse')]
      : revoked
        ? [revokedCopy(idioma).title, revokedCopy(idioma).body]
        : [t('conta.linha.semConta'), t('conta.linha.entre')]
  return (
    <div ref={row} className="settings-row">
      <Icon
        name={revoked ? 'checkmark.circle' : 'person.crop.circle'}
        size={26}
        color={revoked ? 'var(--text-success, var(--success))' : 'var(--context-ink-2, var(--ink-2))'}
      />
      <VStack grow className="settings-stack">
        <Text truncate>{title}</Text>
        <Text size="caption" tone={cloud.deviceRegistration === 'failed' && account.isSignedIn && !account.isSigningOut ? 'attention' : 'secondary'}>{detail}</Text>
      </VStack>
      {account.isSigningOut ? (
        <span className="account-exit-spin" aria-hidden>
          <Spinner size={16} />
        </span>
      ) : account.isSignedIn ? (
        <>
          {retry ? (
            <Button kind="secondary" onClick={() => { holdFocus(); retry() }} disabled={!idle || registering}>
              {registering ? (
                <span className="account-exit-spin" aria-hidden>
                  <Spinner size={13} />
                </span>
              ) : null}
              {registrationLabel(cloud.deviceRegistration, idioma)}
            </Button>
          ) : null}
          <Button kind="secondary" onClick={() => void account.requestSignOut()} disabled={!idle}>
            {t(account.isBusy ? 'conta.linha.aguarde' : 'conta.linha.sair')}
          </Button>
        </>
      ) : (
        <>
          {account.socialProviders.includes('apple') ? (
            <Button kind="secondary" onClick={() => void account.signIn('apple')} disabled={!idle}>
              {t('conta.provedor.apple')}
            </Button>
          ) : null}
          {account.socialProviders.includes('google') ? (
            <Button kind="secondary" onClick={() => void account.signIn('google')} disabled={!idle}>
              {t('conta.provedor.google')}
            </Button>
          ) : null}
        </>
      )}
    </div>
  )
}
