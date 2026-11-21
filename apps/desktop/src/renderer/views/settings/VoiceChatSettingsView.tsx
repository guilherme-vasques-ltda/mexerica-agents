// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { onDeviceUnavailableReason } from '../call/voice'
import { useEffect, useMemo, useRef, useState } from 'react'
import type { HostClient } from '../../client/host-client'
import { Button, CardForm, CardSection, ChoicePicker, IconButton, Spinner, ValueRow } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ModalHeader, SheetPage } from '../../components/Overlay'
import { HStack, Spacer, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { usePref } from '../../lib/prefs'
import { useStore } from '../../store/context'
import { useScreenViewed } from '../../store/analytics'
import { errorText, liveClient, Reveal } from './parts'
import * as voice from './voice-settings'
import type { VoiceModels, VoiceProvider } from './voice-settings'
import { formatarNumero } from '../../lib/fork-i18n/index.ts'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { isolado } from '@shared/bidi'

/**
 * Voice chat settings: which voice carries calls (on-device speech, or OpenAI / Gemini on the
 * user's own key kept by the computer), the mode, models and voice. The computer is the store
 * in `StoreContext`. `showsHeader` is false inside Settings, which titles the page itself.
 */
export function VoiceChatSettingsView({ showsHeader = true }: { showsHeader?: boolean }) {
  useScreenViewed('voice_settings')
  const t = useT()
  const idioma = useIdioma()
  const [engine, setEngine] = usePref(voice.engine)
  const [rate, setRate] = usePref(voice.rate)
  const [language, setLanguage] = usePref(voice.language)
  const languages = useMemo(() => voice.languageOptions(window.codync.speech.locales(), idioma), [idioma])
  const provider = voice.isVoiceProvider(engine) ? engine : null
  const [mode, setMode] = usePref(voice.mode(provider ?? 'openai'))
  const footer = provider
    ? t('ajustes.vozChamada.rodapeChave', { provedor: voice.voiceInfo[provider].name })
    : window.codync.speech.available
      ? t('ajustes.vozChamada.rodapeLocal')
      : onDeviceUnavailableReason()
  return (
    <SheetPage className={showsHeader ? 'settings-voice-sheet' : undefined}>
      {showsHeader ? <ModalHeader closeTitle={t('ajustes.fechar')} icon="waveform" title={t('ajustes.vozChamada.titulo')} /> : null}
      <div className="settings-voice-body idioma-superficie" {...superficieIdioma(idioma)}>
        <CardForm>
          <CardSection title={t('ajustes.vozChamada.voz')} footer={footer}>
            {[
              <ValueRow key="engine" label={t('ajustes.vozChamada.motor')}>
                <ChoicePicker language={superficieIdioma(idioma)}
                  selection={engine}
                  options={[...(window.codync.speech.available || engine === 'device' ? [{ id: 'device', label: t('ajustes.vozChamada.noComputador') }] : []), ...voice.VOICE_PROVIDERS.map((p) => ({ id: p, label: voice.voiceInfo[p].name }))]}
                  onChange={setEngine}
                />
              </ValueRow>,
              ...(provider
                ? [
                    <ValueRow key="mode" label={t('ajustes.vozChamada.modo')} detail={mode === 'speech' ? t('ajustes.vozChamada.modoFala') : t('ajustes.vozChamada.modoVivo')}>
                      <ChoicePicker language={superficieIdioma(idioma)} selection={mode} options={[{ id: 'realtime', label: t('ajustes.vozChamada.aoVivo') }, { id: 'speech', label: t('ajustes.vozChamada.fala') }]} onChange={setMode} />
                    </ValueRow>,
                    ...(mode === 'speech' ? [<PauseRow key="pause" />] : []),
                  ]
                : [
                    <ValueRow key="language" label={t('ajustes.vozChamada.idioma')} detail={t('ajustes.vozChamada.idiomaDetalhe')}>
                      <ChoicePicker language={superficieIdioma(idioma)} selection={language} options={languages} onChange={setLanguage} />
                    </ValueRow>,
                    <ValueRow key="rate" label={t('ajustes.vozChamada.velocidade')}>
                      <ChoicePicker language={superficieIdioma(idioma)}
                        selection={rate}
                        options={[{ id: 0.42, label: t('ajustes.vozChamada.devagar') }, { id: voice.DEFAULT_RATE, label: t('ajustes.vozChamada.normal') }, { id: 0.56, label: t('ajustes.vozChamada.rapido') }]}
                        onChange={setRate}
                      />
                    </ValueRow>,
                    <PauseRow key="pause" />,
                  ]),
            ]}
          </CardSection>
          {provider ? <ProviderVoiceSettings key={provider} provider={provider} /> : null}
        </CardForm>
      </div>
    </SheetPage>
  )
}

/** How long a silence ends what you say, before it's sent. */
function PauseRow() {
  const [pause, setPause] = usePref(voice.pause)
  const t = useT()
  const idioma = useIdioma()
  const segundos = (valor: number) => t('ajustes.vozChamada.segundos', { valor: formatarNumero(idioma, valor) })
  return (
    <div className="settings-appear">
      <ValueRow label={t('ajustes.vozChamada.pausa')} detail={t('ajustes.vozChamada.pausaDetalhe')}>
      <ChoicePicker language={superficieIdioma(idioma)} selection={pause} options={[1, 1.5, 2.5].map((id) => ({ id, label: segundos(id) }))} onChange={setPause} />
      </ValueRow>
    </div>
  )
}

/** One provider: the key the computer keeps, models, voice and this month's minutes. */
function ProviderVoiceSettings({ provider }: { provider: VoiceProvider }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const info = voice.voiceInfo[provider]
  const [mode] = usePref(voice.mode(provider))
  /** null while the computer hasn't answered yet. */
  const [saved, setSaved] = useState<boolean | null>(null)
  const [replacing, setReplacing] = useState(false)
  const [key, setKey] = useState('')
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null)
  const [chosenVoice, setChosenVoice] = useState(() => voice.chosenVoice(provider))
  const [model, setModel] = useState(() => voice.chosenModel(provider, 'realtime'))
  const [transcribeModel, setTranscribeModel] = useState(() => voice.chosenModel(provider, 'transcribe'))
  const [speechModel, setSpeechModel] = useState(() => voice.chosenModel(provider, 'speech'))
  const [models, setModels] = useState<VoiceModels>(() => voice.modelsCache(provider).get() ?? { models: [], transcribe: [], speech: [] })
  const busyRef = useRef(false)
  const speechMode = mode === 'speech'
  const hasKey = saved === true
  const notConnected = { ok: false, text: t('ajustes.vozChamada.semConexao', { nome: isolado(store.hostName) }) }

  const loadModels = async (client: HostClient) => {
    let list: VoiceModels
    try {
      list = await voice.voiceModels(client, provider)
    } catch {
      return
    }
    setModels(list)
    // A newer default moves every choice the user hasn't made themselves.
    voice.rememberDefaults(provider, list.defaults)
    setModel(voice.chosenModel(provider, 'realtime'))
    setTranscribeModel(voice.chosenModel(provider, 'transcribe'))
    setSpeechModel(voice.chosenModel(provider, 'speech'))
    voice.modelsCache(provider).set(list)
  }

  useEffect(() => {
    let cancelled = false
    void (async () => {
      const client = liveClient(store)
      if (!client) {
        setSaved(false)
        return setResult(notConnected)
      }
      let configured: boolean
      try {
        const status = await voice.voiceStatus(client)
        voice.rememberStatus(status)
        configured = voice.configured(status, provider)
      } catch (e) {
        if (cancelled) return
        setSaved(false)
        return setResult({ ok: false, text: errorText(e) })
      }
      if (cancelled) return
      setSaved(configured)
      if (configured) await loadModels(client)
    })()
    return () => void (cancelled = true)
  }, [provider])

  const pick = (kind: voice.ModelKind, set: (m: string) => void) => (m: string) => {
    set(m)
    voice.saveModel(provider, kind, m)
  }

  const run = (work: (client: HostClient) => Promise<void>) => {
    if (busyRef.current) return
    const client = liveClient(store)
    if (!client) return setResult(notConnected)
    busyRef.current = true
    setBusy(true)
    setResult(null)
    void work(client)
      .catch((e: unknown) => setResult({ ok: false, text: errorText(e) }))
      .finally(() => {
        busyRef.current = false
        setBusy(false)
      })
  }

  /** A pasted key is checked with the provider by the computer before it's kept. With a key saved, Test starts a call credential with the chosen model and voice. */
  const test = () => {
    const typed = key.trim()
    if (!typed && !hasKey) return
    run(async (client) => {
      if (!typed || (!replacing && hasKey)) {
        if (speechMode) await voice.voiceSpeak(client, provider, speechModel, chosenVoice, 'OK')
        else await voice.voiceSession(client, provider, model, chosenVoice)
        setResult({ ok: true, text: t('ajustes.vozChamada.chaveFunciona') })
      } else {
        setSaved(voice.configured(await voice.setVoiceKey(client, provider, typed), provider))
        setKey('')
        setReplacing(false)
        setResult({ ok: true, text: t('ajustes.vozChamada.chaveSalva', { nome: isolado(store.hostName) }) })
      }
      await loadModels(client)
    })
  }

  const remove = () =>
    run(async (client) => {
      setSaved(voice.configured(await voice.setVoiceKey(client, provider, ''), provider))
      setResult(null)
    })

  const picker = (selection: string, list: string[], onChange: (m: string) => void) => {
    // The current choice stays listed before the provider's list arrives.
    const options = list.includes(selection) ? list : [selection, ...list]
    return <ChoicePicker language={superficieIdioma(idioma)} selection={selection} options={options.map((m) => ({ id: m, label: m }))} onChange={onChange} />
  }

  const dimmed = hasKey ? 'settings-dimmable' : 'settings-dimmable dimmed'
  const keyArea = (
    <VStack key="key" gap={8}>
      {hasKey && !replacing ? (
        // The key is on the computer: say so, and offer test / replace / remove.
        <div className="settings-row">
          <Icon name="key.fill" size={13} color="var(--context-ink-2, var(--ink-2))" />
          <VStack grow className="settings-stack">
            <Text tone="text" truncate>{t('ajustes.vozChamada.chaveSalvaTitulo')}</Text>
            <Text size="caption" tone="tertiary" truncate>{t('ajustes.vozChamada.ficaEm', { nome: isolado(store.hostName) })}</Text>
          </VStack>
          {busy ? (
            <Spinner />
          ) : (
            <>
              <IconButton title={t('ajustes.vozChamada.testar')} icon="checkmark.seal" onClick={test} />
              <IconButton title={t('ajustes.vozChamada.trocar')} icon="pencil" onClick={() => { setReplacing(true); setResult(null) }} />
              <IconButton title={t('ajustes.vozChamada.remover')} icon="trash" onClick={remove} />
            </>
          )}
        </div>
      ) : (
        // Paste a key; the computer checks it with the provider before keeping it.
        <VStack gap={10}>
          <HStack gap={8} className="settings-row">
            <input
              type="password"
              aria-label={t('ajustes.vozChamada.chaveApi')}
              placeholder={info.keyPrompt}
              value={key}
              spellCheck={false}
              autoComplete="off"
              className="settings-key-input"
              onChange={(e) => setKey(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && test()}
            />
            {busy ? (
              <Spinner />
            ) : (
              <Button onClick={test} disabled={!key.trim()}>
                {t('ajustes.vozChamada.salvar')}
              </Button>
            )}
          </HStack>
          <Text as="div" size="footnote" className="settings-row">
            <button className="settings-link" onClick={() => window.codync.app.openExternal(info.keyPage)}>
              {t('ajustes.vozChamada.conseguir', { provedor: info.name })}
            </button>
            <Spacer />
            {replacing ? (
              <button className="settings-quiet-button" onClick={() => { setReplacing(false); setKey(''); setResult(null) }}>
                {t('ajustes.vozChamada.cancelar')}
              </button>
            ) : null}
          </Text>
        </VStack>
      )}
      <Reveal show={result !== null}>
        {result ? (
          <HStack as="span" gap={5} className={`settings-key-result ${result.ok ? '' : 'failed'}`}>
            <Icon name={result.ok ? 'checkmark.circle.fill' : 'exclamationmark.circle.fill'} size={10} />
            {result.text}
          </HStack>
        ) : null}
      </Reveal>
    </VStack>
  )

  return (
    <CardSection title={info.name} footer={hasKey ? null : t('ajustes.vozChamada.adicioneChave', { provedor: info.name })}>
      {[
        keyArea,
        ...(speechMode
          ? [
              <div key="stt" className={dimmed}><ValueRow label={t('ajustes.vozChamada.falaTexto')}>{picker(transcribeModel, models.transcribe, pick('transcribe', setTranscribeModel))}</ValueRow></div>,
              <div key="tts" className={dimmed}><ValueRow label={t('ajustes.vozChamada.textoFala')}>{picker(speechModel, models.speech, pick('speech', setSpeechModel))}</ValueRow></div>,
            ]
          : [<div key="model" className={dimmed}><ValueRow label={t('ajustes.vozChamada.modelo')}>{picker(model, models.models, pick('realtime', setModel))}</ValueRow></div>]),
        <div key="voice" className={dimmed}>
          <ValueRow label={t('ajustes.vozChamada.voz')}>
            <ChoicePicker language={superficieIdioma(idioma)}
              selection={chosenVoice}
              options={info.voices.map((v) => ({ id: v, label: v }))}
              onChange={(v) => {
                setChosenVoice(v)
                voice.saveVoice(provider, v)
              }}
            />
          </ValueRow>
        </div>,
        <div key="minutes" className={dimmed}>
          <ValueRow label={t('ajustes.vozChamada.esteMes')}>{t('ajustes.vozChamada.minutos', { minutos: voice.voiceMinutes(provider) })}</ValueRow>
        </div>,
      ]}
    </CardSection>
  )
}
