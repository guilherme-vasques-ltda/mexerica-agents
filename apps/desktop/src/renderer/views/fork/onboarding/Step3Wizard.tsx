import { useEffect, useRef, useState } from 'react'
import { Button, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useT } from '../../../lib/fork-i18n/estado'
import type { HostClient } from '../../../client/host-client'
import type { HostState } from '../../../../shared/ipc'
import { forkFeatures, showsAccountControls } from '../../../../shared/fork-features.ts'
import type { EscolhaEntrada, ModoEntrada } from '../../../../shared/fork-modo-entrada.ts'
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
import { FundoMexericas } from './FundoMexericas'
import mascote from '../../../../../resources/brand/mexerica/mexerica-mascote-256.png'
import '../idioma/entrada-marca.css'
import './onboarding.css'
import './entrada-guiada.css'

/**
 * As telas da entrada guiada, uma decisão por tela. "Como usar" (100% local ou com conta) só aparece enquanto
 * a escolha não foi gravada; "Infraestrutura" só no caminho com conta.
 */
export type EtapaEntrada = 'boasVindas' | 'modo' | 'nuvem' | 'onde' | 'ia' | 'pronto'

// A etapa sobrevive à ida a Ajustes ("Conectar conta de IA" desmonta o assistente): volta onde a pessoa estava.
let etapaLembrada: { spaceId: string; etapa: EtapaEntrada } | null = null

export function Step3Wizard({
  client,
  spaceId,
  hostState,
  devHost = false,
  localHubSupported = false,
  platform,
  status,
  where,
  etapaInicial,
  installPort = liveInstallPort,
  onSelectWhere,
  onOpenComputers,
  onConnectAi,
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
  etapaInicial?: EtapaEntrada
  installPort?: OnboardingInstallPort
  onSelectWhere: (where: WhereChoice) => void
  onOpenComputers: () => void
  onConnectAi?: () => void
  onSkip: () => void
  onContinue: () => void
}) {
  const t = useT()
  const [etapa, setEtapa] = useState<EtapaEntrada>(
    () => etapaInicial ?? (etapaLembrada?.spaceId === spaceId ? etapaLembrada.etapa : 'boasVindas'),
  )
  const contaDisponivel = showsAccountControls()
  const [perguntaModo, setPerguntaModo] = useState(etapa === 'modo' || etapa === 'nuvem')
  const [modo, setModo] = useState<ModoEntrada>(etapa === 'nuvem' ? 'conta' : 'local')
  useEffect(() => {
    let ativo = true
    window.codync.app.escolhaEntrada?.().then((escolha) => { if (ativo && escolha === null) setPerguntaModo(true) }, () => {})
    return () => { ativo = false }
  }, [])
  const gravarEscolha = (escolha: EscolhaEntrada) => {
    void window.codync.app.gravarEscolhaEntrada?.(escolha).catch(() => {})
  }
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

  const etapas: EtapaEntrada[] = [
    'boasVindas',
    ...(perguntaModo ? (modo === 'conta' ? ['modo', 'nuvem'] as const : ['modo'] as const) : []),
    'onde', 'ia', 'pronto',
  ]
  const indice = etapas.indexOf(etapa)
  const avancar = () => {
    if (etapa === 'modo' && modo === 'local') gravarEscolha({ modo: 'local', infraestrutura: null })
    if (etapa === 'nuvem') gravarEscolha({ modo: 'conta', infraestrutura: 'propria' })
    irPara(etapas[indice + 1]!)
  }
  const escolha = ({ selecionada, emBreve = false, icone, titulo, desc, onEscolher }: {
    selecionada: boolean
    emBreve?: boolean
    icone: string
    titulo: string
    desc: string
    onEscolher?: () => void
  }) => (
    <button
      type="button"
      role="radio"
      aria-checked={selecionada}
      disabled={emBreve}
      className={`eg-opcao eg-escolha${selecionada ? ' is-escolhida' : ''}`}
      onClick={onEscolher}
    >
      <span className="eg-opcao-icone" aria-hidden="true"><Icon name={icone} size={20} /></span>
      <span className="eg-opcao-corpo">
        <span className="eg-opcao-titulo">
          <strong>{titulo}</strong>
          {emBreve ? <span className="eg-selo">{t('onboarding.guia.emBreve')}</span> : null}
        </span>
        <span className="eg-opcao-desc">{desc}</span>
      </span>
      <span className="eg-escolha-marca" aria-hidden="true">
        <Icon name={selecionada ? 'checkmark.circle.fill' : 'circle'} size={20} />
      </span>
    </button>
  )
  const irPara = (proxima: EtapaEntrada) => {
    etapaLembrada = { spaceId, etapa: proxima }
    setEtapa(proxima)
  }
  const sair = (acao: () => void) => {
    etapaLembrada = null
    acao()
  }
  const localPronto = where === 'local' && localStage.kind === 'ready'
  const conectaAqui = !!status?.capabilities.localKeysAndOAuth && isPersonalSpace

  const estadoLocal = localStage.kind === 'ready' ? (
    <div className="onboarding-status-row is-ok" role="status">
      <Icon name="checkmark.circle.fill" size={16} />
      <span>{t('onboarding.guia.onde.local.pronto')}</span>
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
  )

  const conteudo = etapa === 'boasVindas' ? (
    <section className="eg-etapa eg-boas-vindas" aria-labelledby="eg-titulo">
      <img src={mascote} alt="" className="eg-mascote eg-mascote-entra" />
      <p className="eg-ola">{t('onboarding.guia.boasVindas.ola')}</p>
      <h1 id="eg-titulo">{t('onboarding.guia.boasVindas.titulo')}</h1>
      <p className="eg-texto">{t('onboarding.guia.boasVindas.texto')}</p>
    </section>
  ) : etapa === 'modo' ? (
    <section className="eg-etapa" aria-labelledby="eg-titulo">
      <h1 id="eg-titulo">{t('onboarding.guia.modo.titulo')}</h1>
      <p className="eg-texto">{t('onboarding.guia.modo.texto')}</p>
      <div className="eg-escolhas" role="radiogroup" aria-labelledby="eg-titulo">
        {escolha({
          selecionada: modo === 'local',
          icone: 'lock.shield',
          titulo: t('onboarding.guia.modo.local.titulo'),
          desc: t('onboarding.guia.modo.local.desc'),
          onEscolher: () => setModo('local'),
        })}
        {escolha({
          selecionada: modo === 'conta',
          emBreve: !contaDisponivel,
          icone: 'person.crop.circle',
          titulo: t('onboarding.guia.modo.conta.titulo'),
          desc: t('onboarding.guia.modo.conta.desc'),
          onEscolher: () => setModo('conta'),
        })}
      </div>
    </section>
  ) : etapa === 'nuvem' ? (
    <section className="eg-etapa" aria-labelledby="eg-titulo">
      <h1 id="eg-titulo">{t('onboarding.guia.nuvem.titulo')}</h1>
      <p className="eg-texto">{t('onboarding.guia.nuvem.texto')}</p>
      <div className="eg-escolhas" role="radiogroup" aria-labelledby="eg-titulo">
        {escolha({
          selecionada: true,
          icone: 'server.rack',
          titulo: t('onboarding.guia.nuvem.propria.titulo'),
          desc: t('onboarding.guia.nuvem.propria.desc'),
        })}
        {escolha({
          selecionada: false,
          emBreve: true,
          icone: 'cloud',
          titulo: t('onboarding.guia.nuvem.nossa.titulo'),
          desc: t('onboarding.guia.nuvem.nossa.desc'),
        })}
      </div>
    </section>
  ) : etapa === 'onde' ? (
    <section className="eg-etapa" aria-labelledby="eg-titulo">
      <h1 id="eg-titulo">{t('onboarding.guia.onde.titulo')}</h1>
      <p className="eg-texto">{t('onboarding.guia.onde.texto')}</p>
      {where === 'local' ? (
        <>
          <div className={`eg-opcao ${localPronto ? 'is-pronta' : ''}`}>
            <span className="eg-opcao-icone" aria-hidden="true"><Icon name="desktopcomputer" size={20} /></span>
            <div className="eg-opcao-corpo">
              <div className="eg-opcao-titulo">
                <strong>{t('onboarding.guia.onde.local.titulo')}</strong>
                <span className="eg-selo">{t('onboarding.guia.onde.local.recomendado')}</span>
              </div>
              <span className="eg-opcao-desc">{t('onboarding.guia.onde.local.desc')}</span>
              <div className="eg-opcao-estado">{estadoLocal}</div>
            </div>
          </div>
          <button type="button" className="eg-link" onClick={() => onSelectWhere('remote')}>
            {t('onboarding.guia.onde.outro')}
          </button>
        </>
      ) : (
        <>
          <div className="eg-opcao">
            <span className="eg-opcao-icone" aria-hidden="true"><Icon name="server.rack" size={20} /></span>
            <div className="eg-opcao-corpo">
              <strong>{t('onboarding.guia.onde.remoto.titulo')}</strong>
              <span className="eg-opcao-desc">{t('onboarding.guia.onde.remoto.desc')}</span>
              <p className="onboarding-muted">{t('onboarding.passo3.remoto.semContrato')}</p>
              <div>
                <Button kind="secondary" onClick={() => sair(onOpenComputers)}>
                  {t('onboarding.passo3.remoto.abrirComputadores')}
                </Button>
              </div>
            </div>
          </div>
          {hostState.kind !== 'disabled' ? (
            <button type="button" className="eg-link" onClick={() => onSelectWhere('local')}>
              {t('onboarding.guia.onde.voltarLocal')}
            </button>
          ) : null}
        </>
      )}
    </section>
  ) : etapa === 'ia' ? (
    <section className="eg-etapa" aria-labelledby="eg-titulo">
      <h1 id="eg-titulo">{t('onboarding.guia.ia.titulo')}</h1>
      <p className="eg-texto">{t('onboarding.guia.ia.texto')}</p>
      {hasReadyAi ? (
        <div className="onboarding-status-row is-ok" role="status">
          <Icon name="checkmark.circle.fill" size={16} />
          <span>{t('onboarding.guia.ia.pronta')}</span>
        </div>
      ) : null}
      {client && where === 'local' ? (
        <>
          {/* Ponto de encaixe: a conexão da conta (Codex e afins) é a de Ajustes → Contas de IA, chamada aqui sem lógica própria. */}
          {onConnectAi && !hasReadyAi && !conectaAqui ? (
            <div>
              <Button kind="primary" onClick={onConnectAi}>{t('onboarding.guia.ia.conectar')}</Button>
            </div>
          ) : null}
          <details className="eg-mais" open={conectaAqui || !onConnectAi}>
            <summary>{t('onboarding.guia.ia.outrasOpcoes')}</summary>
            <div className="eg-mais-corpo">
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

              {conectaAqui ? (
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
            </div>
          </details>
        </>
      ) : (
        <p className="onboarding-muted">{t('onboarding.ia.somenteRemoto')}</p>
      )}
    </section>
  ) : (
    <section className="eg-etapa eg-pronto" aria-labelledby="eg-titulo">
      <img src={mascote} alt="" className="eg-mascote eg-mascote-comemora" />
      <h1 id="eg-titulo">{t('onboarding.guia.pronto.titulo')}</h1>
      <p className="eg-texto">{t(hasReadyAi ? 'onboarding.guia.pronto.texto' : 'onboarding.guia.pronto.textoSemIa')}</p>
      <ul className="eg-resumo">
        <li className={localPronto || where === 'remote' ? 'is-ok' : undefined}>
          <Icon name={localPronto ? 'checkmark.circle.fill' : 'circle.dotted'} size={16} />
          <span>{t('onboarding.guia.pronto.computador')}</span>
          <em>{t(localPronto ? 'onboarding.guia.pronto.ok' : 'onboarding.guia.pronto.pendente')}</em>
        </li>
        <li className={hasReadyAi ? 'is-ok' : undefined}>
          <Icon name={hasReadyAi ? 'checkmark.circle.fill' : 'circle.dotted'} size={16} />
          <span>{t('onboarding.guia.pronto.ia')}</span>
          <em>{t(hasReadyAi ? 'onboarding.guia.pronto.ok' : 'onboarding.guia.pronto.pendente')}</em>
        </li>
      </ul>
    </section>
  )

  return (
    <div className="entrada-guiada" data-etapa={etapa}>
      <FundoMexericas />
      <div className="eg-palco">
        <div className="eg-cartao">
          <ol className="eg-etapas" aria-label={t('onboarding.etapa', { atual: indice + 1, total: etapas.length })}>
            {etapas.map((nome, i) => (
              <li key={nome} className={i < indice ? 'is-feita' : i === indice ? 'is-agora' : undefined} aria-current={i === indice ? 'step' : undefined}>
                <span className="eg-etapa-num" aria-hidden="true">
                  {i < indice ? <Icon name="checkmark" size={11} /> : i + 1}
                </span>
                <span className="eg-etapa-nome">{t(`onboarding.guia.passos.${nome}`)}</span>
              </li>
            ))}
          </ol>

          <div className="eg-conteudo" key={etapa}>{conteudo}</div>

          <footer className="eg-rodape">
            {indice > 0 ? (
              <Button kind="secondary" onClick={() => irPara(etapas[indice - 1]!)}>
                <Icon name="arrow.left" size={14} />
                {t('onboarding.voltar')}
              </Button>
            ) : (
              <button type="button" className="eg-link" onClick={() => sair(onSkip)}>{t('onboarding.pularPorEnquanto')}</button>
            )}
            {etapa === 'pronto' ? (
              <Button kind="primary" onClick={() => sair(onContinue)}>
                <Icon name="bubble.left.and.bubble.right" size={14} />
                {t('onboarding.guia.pronto.conversar')}
              </Button>
            ) : (
              <Button kind="primary" onClick={avancar}>
                {t(etapa === 'boasVindas' ? 'onboarding.guia.boasVindas.comecar' : 'onboarding.continuar')}
                <Icon name="arrow.right" size={14} />
              </Button>
            )}
          </footer>
          {indice > 0 && etapa !== 'pronto' ? (
            <button type="button" className="eg-link eg-pular" onClick={() => sair(onSkip)}>{t('onboarding.pularPorEnquanto')}</button>
          ) : null}
        </div>
      </div>
    </div>
  )
}
