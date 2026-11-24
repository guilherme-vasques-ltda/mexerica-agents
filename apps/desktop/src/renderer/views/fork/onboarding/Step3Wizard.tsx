import { useEffect, useRef, useState } from 'react'
import { Button, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useT } from '../../../lib/fork-i18n/estado'
import { AvisoInstalacaoLocal } from '../aviso-instalacao-local/AvisoInstalacaoLocal'
import type { HostClient } from '../../../client/host-client'
import type { HostState } from '../../../../shared/ipc'
import { forkFeatures } from '../../../../shared/fork-features.ts'
import {
  resolveLocalInstallStage,
  type OnboardingDetectPayload,
  type OnboardingStatusPayload,
  type WhereChoice,
} from '../../../../shared/fork-onboarding.ts'
import {
  detectOnboardingReadOnly,
  liveInstallPort,
  ONBOARDING_KEY_PROVIDERS,
  saveAndTestApiKey,
  startOpenRouterSignIn,
  type AiActionResult,
  type OnboardingInstallPort,
} from './adapter'
import { StepProgress } from './StepProgress'
import mascote from '../../../../../resources/brand/mexerica/mexerica-mascote-256.png'
import '../idioma/entrada-marca.css'
import './onboarding.css'

export function Step3Wizard({
  client,
  spaceId,
  hostState,
  devHost = false,
  localHubSupported = false,
  platform,
  status,
  where,
  installPort = liveInstallPort,
  onSelectWhere,
  onOpenComputers,
  onSkip,
  onContinue,
}: {
  client: HostClient | null
  spaceId: string
  hostState: HostState
  devHost?: boolean
  localHubSupported?: boolean
  platform: 'darwin' | 'linux' | 'win32'
  status: OnboardingStatusPayload | null
  where: WhereChoice
  installPort?: OnboardingInstallPort
  onSelectWhere: (where: WhereChoice) => void
  onOpenComputers: () => void
  onSkip: () => void
  onContinue: () => void
}) {
  const t = useT()
  const [consentGiven, setConsentGiven] = useState(false)
  const [detecting, setDetecting] = useState(false)
  const [detected, setDetected] = useState<OnboardingDetectPayload | null>(null)
  const [detectError, setDetectError] = useState(false)

  const [provider, setProvider] = useState<string>(ONBOARDING_KEY_PROVIDERS[0]!)
  const [apiKey, setApiKey] = useState('')
  const [aiBusy, setAiBusy] = useState<'key' | 'openrouter' | null>(null)
  const [aiResult, setAiResult] = useState<AiActionResult | null>(null)
  const cancelOAuthRef = useRef<(() => void) | null>(null)
  const latestContext = useRef({ client, spaceId })
  latestContext.current = { client, spaceId }
  const detectRevision = useRef(0)
  const aiRevision = useRef(0)

  useEffect(() => () => {
    detectRevision.current += 1
    aiRevision.current += 1
    cancelOAuthRef.current?.()
  }, [])

  const localStage = resolveLocalInstallStage({
    hostKind: hostState.kind,
    hostMessage: hostState.kind === 'failed' ? hostState.message : undefined,
    localHostFeature: forkFeatures.localHost,
    macLocalHubFeature: forkFeatures.macLocalHub,
    devHost,
    localHubSupported,
    platform,
    consentGiven,
  })
  const linuxInstallUnavailable = !window.codync.web && platform === 'linux'
    && (localStage.kind === 'needs_consent' || localStage.kind === 'needs_install')

  const hasReadyAi = status?.ai.hasReadyAi ?? false
  const personalSpaceId = status?.spaceContext?.personalSpaceId ?? null
  const isPersonalSpace = !!personalSpaceId && spaceId === personalSpaceId

  const runReadOnlyDetect = async () => {
    if (!client || detecting) return
    const context = latestContext.current
    const request = ++detectRevision.current
    const current = () => request === detectRevision.current && context.client === latestContext.current.client && context.spaceId === latestContext.current.spaceId
    setDetecting(true)
    setDetectError(false)
    try {
      const payload = await detectOnboardingReadOnly(client)
      if (current()) setDetected(payload)
    } catch {
      if (current()) setDetectError(true)
    } finally {
      if (current()) setDetecting(false)
    }
  }

  const handleSaveKey = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!client || aiBusy || !apiKey.trim()) return
    setAiBusy('key')
    setAiResult(null)
    const context = latestContext.current
    const request = ++aiRevision.current
    const current = () => request === aiRevision.current && context.client === latestContext.current.client && context.spaceId === latestContext.current.spaceId
    try {
      const res = await saveAndTestApiKey(client, provider, apiKey, () => setApiKey(''))
      if (current()) setAiResult(res)
    } catch {
      if (current()) setAiResult({ outcome: 'nao_verificado', modelsCount: 0 })
    } finally {
      if (current()) setAiBusy(null)
    }
  }

  const handleOAuth = (kind: 'openrouter') => {
    if (!client || aiBusy) return
    setAiBusy(kind)
    setAiResult(null)
    const context = latestContext.current
    const request = ++aiRevision.current
    const current = () => request === aiRevision.current && context.client === latestContext.current.client && context.spaceId === latestContext.current.spaceId
    const runner = startOpenRouterSignIn(client, spaceId, () => {})
    cancelOAuthRef.current = runner.cancel
    void runner.promise
      .then((res) => {
        if (current()) setAiResult(res)
      })
      .finally(() => {
        if (current()) {
          cancelOAuthRef.current = null
          setAiBusy(null)
        }
      })
  }

  useEffect(() => {
    detectRevision.current += 1
    aiRevision.current += 1
    cancelOAuthRef.current?.()
    cancelOAuthRef.current = null
    setConsentGiven(false)
    setDetected(null)
    setDetecting(false)
    setDetectError(false)
    setAiResult(null)
    setApiKey('')
    setAiBusy(null)
  }, [client, spaceId])

  return (
    <div className="onboarding-step3">
      <div className="onboarding-step3-shell">
        <aside className="onboarding-step3-aside">
          <div className="onboarding-step3-berga">
            <img src={mascote} alt="" className="onboarding-step3-mascot" />
            <div className="onboarding-speech-bubble">
              <p>{t('onboarding.passo3.bergaFala')}</p>
            </div>
          </div>
          {status && where === 'local' ? (
            <div className="onboarding-step3-summary">
              <Icon name={hasReadyAi ? 'checkmark.circle.fill' : 'circle.dotted'} size={16} />
              <span>{t(hasReadyAi ? 'onboarding.ia.pronta' : 'onboarding.ia.pendente')}</span>
            </div>
          ) : null}
        </aside>

        <main className="onboarding-step3-main">
          <header className="onboarding-step3-header">
            <StepProgress step={3} />
            <h1>{t('onboarding.passo3.titulo')}</h1>
            <p className="onboarding-muted">{t('onboarding.passo3.subtitulo')}</p>
          </header>

          <section className="onboarding-where-grid" role="radiogroup" aria-label={t('onboarding.passo3.titulo')}>
            <button
              type="button"
              role="radio"
              aria-checked={where === 'local'}
              className={`onboarding-choice-card ${where === 'local' ? 'is-selected' : ''}`}
              onClick={() => onSelectWhere('local')}
            >
              <Icon name="desktopcomputer" size={20} />
              <div>
                <strong>{t('onboarding.passo3.local.titulo')}</strong>
                <span>{t('onboarding.passo3.local.desc')}</span>
              </div>
            </button>
            <button
              type="button"
              role="radio"
              aria-checked={where === 'remote'}
              className={`onboarding-choice-card ${where === 'remote' ? 'is-selected' : ''}`}
              onClick={() => onSelectWhere('remote')}
            >
              <Icon name="server.rack" size={20} />
              <div>
                <strong>{t('onboarding.passo3.remoto.titulo')}</strong>
                <span>{t('onboarding.passo3.remoto.desc')}</span>
              </div>
            </button>
          </section>

          {where === 'local' ? (
            <section className="onboarding-panel-card" aria-label={t('onboarding.passo3.local.titulo')}>
              {localStage.kind === 'ready' ? (
                <div className="onboarding-status-row is-ok" role="status">
                  <Icon name="checkmark.circle.fill" size={16} />
                  <span>{t('onboarding.passo3.local.pronto')}</span>
                </div>
              ) : localStage.kind === 'starting' ? (
                <div className="onboarding-status-row" role="status">
                  <Spinner size={15} />
                  <span>{t('onboarding.passo3.local.instalando')}</span>
                </div>
              ) : linuxInstallUnavailable ? (
                <p className="onboarding-muted" role="status">{t('marca.hub.instalacaoAssistidaLinuxIndisponivel')}</p>
              ) : localStage.kind === 'needs_consent' ? (
                <div className="onboarding-consent-box">
                  <strong>{t('onboarding.passo3.local.consentimentoTitulo')}</strong>
                  <p>{t('onboarding.passo3.local.consentimentoDesc')}</p>
                  <Button
                    kind="primary"
                    onClick={() => {
                      setConsentGiven(true)
                      installPort.installLocal()
                    }}
                  >
                    {t('onboarding.passo3.local.instalar')}
                  </Button>
                </div>
              ) : localStage.kind === 'needs_install' ? (
                <Button kind="primary" onClick={() => installPort.installLocal()}>
                  {t('onboarding.passo3.local.instalar')}
                </Button>
              ) : localStage.kind === 'failed' ? (
                <div className="onboarding-status-stack">
                  <p className="onboarding-error-text" role="alert">
                    {t('onboarding.passo3.local.falhou', { motivo: localStage.message || t('onboarding.ia.estado.nao_verificado') })}
                  </p>
                  <Button kind="secondary" onClick={() => installPort.restartLocal()}>
                    {t('onboarding.passo3.local.tentarNovamente')}
                  </Button>
                </div>
              ) : (
                <p className="onboarding-muted" role="status">
                  {t(localStage.kind === 'blocked_missing_binary'
                    ? 'onboarding.passo3.local.bloqueadoBinario'
                    : 'onboarding.passo3.local.bloqueadoPlataforma')}
                </p>
              )}
              <AvisoInstalacaoLocal />
            </section>
          ) : (
            <section className="onboarding-panel-card" aria-label={t('onboarding.passo3.remoto.titulo')}>
              <p className="onboarding-muted">{t('onboarding.passo3.remoto.semContrato')}</p>
              <Button kind="secondary" onClick={onOpenComputers}>
                {t('onboarding.passo3.remoto.abrirComputadores')}
              </Button>
            </section>
          )}

          <section className="onboarding-panel-card" aria-label={t('onboarding.ia.titulo')}>
            <h2>{t('onboarding.ia.titulo')}</h2>
            <p className="onboarding-muted">{t('onboarding.ia.subtitulo')}</p>

            {client && where === 'local' ? (
              <>
                <div className="onboarding-detect-row">
                  <Button kind="secondary" disabled={detecting} onClick={() => void runReadOnlyDetect()}>
                    {detecting ? t('onboarding.ia.detectando') : t('onboarding.ia.detectarBotao')}
                  </Button>
                  {detectError ? <span className="onboarding-error-text">{t('onboarding.ia.estado.nao_verificado')}</span> : null}
                </div>

                {detected ? (
                  <div className="onboarding-detect-result" role="status">
                    <p className="onboarding-muted">{t('onboarding.ia.detectadoAviso')}</p>
                    <p>
                      {detected.ollama.reachable
                        ? t('onboarding.ia.ollamaDisponivel', { quantidade: detected.ollama.modelsCount })
                        : t('onboarding.ia.ollamaAusente')}
                    </p>
                  </div>
                ) : null}

                {where === 'local' && status?.capabilities.localKeysAndOAuth && isPersonalSpace ? (
                  <>
                    <div className="onboarding-oauth-row">
                      <Button kind="secondary" disabled={aiBusy !== null} onClick={() => handleOAuth('openrouter')}>
                        {t('onboarding.ia.openrouter')}
                      </Button>
                      {aiBusy === 'openrouter' ? (
                        <Button kind="secondary" onClick={() => cancelOAuthRef.current?.()}>
                          {t('onboarding.ia.cancelarLogin')}
                        </Button>
                      ) : null}
                    </div>

                    <form className="onboarding-key-form" onSubmit={(e) => void handleSaveKey(e)}>
                      <p className="onboarding-key-heading">{t('onboarding.ia.ouChave')}</p>
                      <div className="onboarding-key-fields">
                        <label>
                          <span>{t('onboarding.ia.provedor')}</span>
                          <select value={provider} onChange={(e) => setProvider(e.target.value)} disabled={aiBusy !== null}>
                            {ONBOARDING_KEY_PROVIDERS.map((id) => (
                              <option key={id} value={id}>{t(`onboarding.ia.provedor.${id}`)}</option>
                            ))}
                          </select>
                        </label>
                        <label className="onboarding-key-input-wrap">
                          <span>{t('onboarding.ia.chaveLabel')}</span>
                          <input
                            type="password"
                            autoComplete="off"
                            spellCheck={false}
                            value={apiKey}
                            onChange={(e) => setApiKey(e.target.value)}
                            placeholder={t('onboarding.ia.chavePlaceholder')}
                            disabled={aiBusy !== null}
                          />
                        </label>
                        <button type="submit" className="styled-button primary" disabled={aiBusy !== null || !apiKey.trim()}>
                          {aiBusy === 'key' ? t('onboarding.ia.testando') : t('onboarding.ia.testarSalvar')}
                        </button>
                      </div>
                    </form>
                  </>
                ) : null}

                {aiResult ? (
                  <p className={`onboarding-ai-outcome is-${aiResult.outcome}`} role="status">
                    {aiResult.outcome === 'modelos_no_catalogo'
                      ? t('onboarding.ia.estado.modelos_no_catalogo', { quantidade: aiResult.modelsCount })
                      : t(`onboarding.ia.estado.${aiResult.outcome}`)}
                  </p>
                ) : null}
              </>
            ) : (
              <p className="onboarding-muted">{t('onboarding.ia.somenteRemoto')}</p>
            )}
          </section>

          <footer className="onboarding-step3-footer">
            <Button kind="secondary" onClick={onSkip}>{t('onboarding.pularPorEnquanto')}</Button>
            <Button kind="primary" onClick={onContinue}>
              {t('onboarding.continuar')}
              <Icon name="arrow.right" size={14} />
            </Button>
          </footer>
        </main>
      </div>
    </div>
  )
}
