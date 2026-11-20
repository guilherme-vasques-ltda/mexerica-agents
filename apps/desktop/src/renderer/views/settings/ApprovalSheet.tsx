// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useState } from 'react'
import { Button, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ModalHeader } from '../../components/Overlay'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import type { Approval } from '../../store/app-model'
import { useApp } from '../../store/context'
import { deviceIcon, errorText, Reveal, spacedCode } from './parts'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { comNos, no } from './rotulo'

/**
 * A device asks for access through the account (spec §4.2 B). The code is the only thing that
 * tells a real request from a cloud swapping keys, so the device shows it and the person types it here;
 * Approve stays off until it equals the Hub's code, and the Mac never displays it.
 * Closing puts the request off (the sheet's onClose defers it).
 */
export function ApprovalSheet({ approval, confirmCode }: { approval: Approval; confirmCode?: string }) {
  return confirmCode ? <ApprovalConfirmed approval={approval} code={confirmCode} /> : <ApprovalForm approval={approval} />
}

/**
 * MX-149: only after the Hub saved the device (the `approved` result) the computer shows its own confirmation code,
 * and the person types it on the phone. It never shows before that result, and it is not a secret from the cloud:
 * what protects the pairing is that the person types what the real computer shows.
 */
function ApprovalConfirmed({ approval, code }: { approval: Approval; code: string }) {
  const t = useT()
  const idioma = useIdioma()
  const app = useApp()
  const aparelho = <bdi>{approval.request.deviceName}</bdi>
  return (
    <VStack className="settings-approval idioma-superficie" {...superficieIdioma(idioma)}>
      <ModalHeader closeTitle={t('ajustes.fechar')} icon="checkmark.shield" title={t('ajustes.pedido.pronto')} />
      <VStack align="center" gap={16} className="settings-approval-body">
        <Text as="div" size="title3" weight="semibold" className="settings-centered">
          {comNos(t('ajustes.pedido.digiteNoAparelho', { aparelho: no('aparelho') }), { aparelho })}
        </Text>
        <bdi dir="ltr" className="settings-approval-confirm bidi-isolado settings-appear" aria-label={t('ajustes.pedido.codigoConfirmacao')}>
          {spacedCode(code)}
        </bdi>
        <Text as="div" size="callout" tone="secondary" className="settings-centered">{t('ajustes.pedido.termineConectar')}</Text>
        <Button onClick={() => app.dismissConfirmation()}>{t('ajustes.pedido.concluido')}</Button>
      </VStack>
    </VStack>
  )
}

function ApprovalForm({ approval }: { approval: Approval }) {
  const t = useT()
  const idioma = useIdioma()
  const app = useApp()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const request = approval.request
  const [typed, setTyped] = useState('')
  const matches = request.code !== undefined && request.code !== null && typed === request.code

  const decide = async (approve: boolean) => {
    setBusy(true)
    try {
      const { confirmCode } = await app.decide(approval, approve)
      // The host's accessRequests event removes it; don't wait for that to close. With a confirmCode the sheet stays
      // open on the success state (the code is shown only now, after `approved`); without one (older Hub) it closes.
      if (approve && confirmCode) app.showConfirmation(approval, confirmCode)
      else app.deferApproval(approval)
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(false)
    }
  }

  // Cartão de aprovação (lição da R1 do #565): nome do aparelho e do computador em <bdi>, e-mail e código da esquerda
  // para a direita e isolados; em árabe, o bidi não troca a ordem do que a pessoa confere antes de aprovar.
  const aparelho = <bdi>{request.deviceName}</bdi>

  return (
    <VStack className="settings-approval idioma-superficie" {...superficieIdioma(idioma)}>
      <ModalHeader closeTitle={t('ajustes.fechar')} icon="lock.shield" title={t('ajustes.pedido.titulo')} />
      <VStack align="center" gap={16} className="settings-approval-body">
        <Icon name={deviceIcon(request.platform)} size={40} weight="light" color="var(--context-ink-2, var(--ink-2))" />
        <VStack align="center" gap={4}>
          <Text as="div" size="title3" weight="semibold" className="settings-centered">
            {comNos(t('ajustes.pedido.querUsar', { aparelho: no('aparelho'), computador: no('computador') }), { aparelho: aparelho, computador: <bdi>{approval.store.hostName}</bdi> })}
          </Text>
          {request.email ? <Text as="div" size="callout" tone="secondary">{comNos(t('ajustes.pedido.conectadoComo', { email: no('email') }), { email: <bdi dir="ltr">{request.email}</bdi> })}</Text> : null}
        </VStack>
        {request.code ? (
          <>
            {/* O código só aparece no aparelho: aqui a pessoa DIGITA, e Aprovar só abre se for igual ao calculado pelo Hub.
                Mostrar o código no Mac deixaria aprovar sem comparar (nuvem comprometida trocaria a chave). */}
            <input
              className="settings-approval-input bidi-isolado settings-appear"
              dir="ltr" inputMode="numeric" autoComplete="off" autoFocus spellCheck={false} maxLength={7}
              aria-label={t('ajustes.pedido.digite')} placeholder="000 000"
              value={spacedCode(typed)}
              onChange={(e) => setTyped(e.target.value.replace(/\D/g, '').slice(0, 6))}
              onKeyDown={(e) => { if (e.key === 'Enter' && matches && !busy) void decide(true) }}
            />
            <Text as="div" size="callout" tone="secondary" className="settings-centered">
              {comNos(t('ajustes.pedido.aproveSo', { aparelho: no('aparelho') }), { aparelho })}
            </Text>
            <Reveal show={typed.length === 6 && !matches}>
              <Text as="div" size="caption" tone="danger" className="settings-centered">{t('ajustes.pedido.naoConfere')}</Text>
            </Reveal>
          </>
        ) : (
          <>
            <Spinner />
            <Text as="div" size="callout" tone="secondary">{comNos(t('ajustes.pedido.esperando', { aparelho: no('aparelho') }), { aparelho })}</Text>
          </>
        )}
        <Reveal show={error !== null}>
          <Text as="div" size="caption" tone="danger" className="settings-centered">{error}</Text>
        </Reveal>
        <HStack align="stretch" gap={12}>
          <Button kind="secondary" onClick={() => void decide(false)} disabled={busy}>
            {t('ajustes.pedido.recusar')}
          </Button>
          <Button onClick={() => void decide(true)} disabled={busy || !matches}>
            {t('ajustes.pedido.aprovar')}
          </Button>
        </HStack>
      </VStack>
    </VStack>
  )
}
