import { useEffect, useMemo, useRef, useState } from 'react'
import { Button, CardForm, CardSection, ChoiceList, ChoicePicker, Pill, SegmentedChoice, Spinner, ValueRow } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { usePref } from '../../../lib/prefs'
import { useScreenViewed } from '../../../store/analytics'
import type { BotStore } from '../../../store/bot-store'
import { useApp } from '../../../store/context'
import { liveClient, Reveal } from '../../settings/parts'
import * as voice from '../../settings/voice-settings'
import type { VoiceProvider } from '../../settings/voice-settings'
import { estadoVozHub, type EstadoVozHub } from './estado-hub'
import { capacidades } from '@shared/fork-capacidades'
import { guardarChave, limparChave, testarChave, type Resultado } from './formulario-chave'
import { onDeviceUnavailableReason } from '../../call/voice'
import { avisoLeituraGratis, fonteDaChave, linhaOuvirLocal, modeloSumiu, podeGerirNoHub, resumoChavesNoHub, rodapeChavesNoHub, TEXTO_FALA_SEM_OUVIR, TEXTO_FALA_SEM_OUVIR_ABAS, TEXTO_LEITURA_GRATIS, type EscolhaVoz, type FonteChave } from './voz-model'
import type { ChaveAparelho, EstadoChaves } from '../../../../shared/fork-voz'
import { agirNoAparelho, fonteEfetiva, guardarNoAparelho, TEXTO_SEM_COFRE, textoSituacaoPendente, textoTransicaoMac, textoTransicaoSemLocal, type Fonte, type ModoVoz, type ResultadoTela } from './chave-aparelho'
import { estadoVozAparelho } from './estado-aparelho'
import { RodapeCurto } from '../ajustes/RodapeCurto'
import './voz.css'
import { formatarNumero, type ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { Mensagem } from '../bot-settings/mensagem'
import { frasesAparelho, textosVoz, textoVoz } from './frases'
import { ltr } from '../../../../shared/bidi'

// voz-03 (D237): Ajustes → Voz, as escolhas deste aparelho. voz-04: a chave da OpenAI/Gemini guardada no
// cofre deste aparelho vence; sem ela, a do Hub do bot da conversa (`reserva`). A gestão da chave do Hub
// só aparece para Hubs do próprio computador ou do túnel SSH. A chave daqui entra por um campo de senha,
// vai ao main e não volta à tela; esta tela só vê a situação de cada provedor.

const mac = window.codync.platform === 'darwin'
// Fatia 6b (D454): "neste Mac", "da OpenAI" e afins saem do catálogo (frases.ts); as frases ficam inteiras no
// catálogo voz/ajustes, e o que fica em estado é dado (D507), traduzido ao desenhar.

/** Observa o estado de voz de um Hub enquanto a tela está aberta. */
function useEstadoHub(store: BotStore | null): EstadoVozHub | null {
  const estado = useMemo(() => (store ? estadoVozHub(store) : null), [store])
  useEffect(() => estado?.observar(), [estado])
  useModel(estado)
  return estado
}

const fonteDe = (store: BotStore | null, estado: EstadoVozHub | null, p: VoiceProvider): FonteChave =>
  store && store.connection.kind === 'online' ? fonteDaChave(estado?.status, p) : 'desconhecida'

/** voz-04: quem paga naquele modo — a chave daqui, a do Hub do bot ou nenhuma (plano §2). */
const fonteNoModo = (modo: ModoVoz, aparelho: EstadoChaves | null, store: BotStore | null, estado: EstadoVozHub | null, p: VoiceProvider): Fonte =>
  fonteEfetiva(modo, aparelho?.chaves[p], fonteDe(store, estado, p))

/** Em duas palavras, no cartão da opção paga (chave do catálogo). */
function selo(store: BotStore | null, fonte: Fonte): ChaveTexto {
  if (fonte === 'incerta') return 'voz.ajustes.selo.incerta'
  if (fonte === 'limpeza-pendente') return 'voz.ajustes.selo.limpeza'
  if (fonte === 'aparelho') return 'voz.ajustes.selo.aparelho'
  if (fonte === 'nao-abriu') return 'voz.ajustes.selo.naoAbriu'
  if (fonte === 'so-tempo-real') return 'voz.ajustes.selo.soTempoReal'
  if (!store) return 'voz.ajustes.selo.semStore'
  if (store.connection.kind !== 'online') return 'voz.ajustes.selo.semConexao'
  return fonte === 'hub' ? 'voz.ajustes.selo.hub' : fonte === 'nenhuma' ? 'voz.ajustes.selo.nenhuma' : 'voz.ajustes.selo.consultando'
}

/**
 * A linha abaixo das opções: de onde vem a chave da opção marcada, no modo dela. `comAbas`: na tela de Ajustes as
 * chaves ficam na aba Chaves, não abaixo (ajustes-tela-01).
 */
function linhaDaFonte(store: BotStore | null, fonte: Fonte, p: VoiceProvider, modo: ModoVoz, ditado: boolean, hubTemChave: boolean, comAbas: boolean): { ok: boolean; partes: Mensagem[] } {
  const provedor = voice.voiceInfo[p].name
  const um = (ok: boolean, chave: ChaveTexto, params: Record<string, string> = {}) => ({ ok, partes: [{ chave, params: { provedor, ...params } }] })
  if (fonte === 'incerta') return um(false, 'voz.ajustes.fonte.incerta')
  if (fonte === 'limpeza-pendente') return um(false, 'voz.ajustes.fonte.limpeza')
  if (fonte === 'aparelho') return um(true, 'voz.ajustes.fonte.aparelho')
  if (fonte === 'nao-abriu') return um(false, comAbas ? 'voz.ajustes.fonte.naoAbriuAbas' : 'voz.ajustes.fonte.naoAbriu')
  if (fonte === 'so-tempo-real') {
    // Até o voz-05b (texto aprovado): no Mac, a parte segue no próprio Mac; sem Ditado, o Fala não liga.
    if (ditado) return { ok: false, partes: [{ chave: textoTransicaoMac(modo === 'ler' ? 'ler' : 'ouvir') }] }
    return { ok: false, partes: [textoTransicaoSemLocal(hubTemChave && store ? store.hostName : null)] }
  }
  if (!store) return um(false, 'voz.ajustes.fonte.semStore')
  if (store.connection.kind !== 'online') return um(false, 'voz.ajustes.fonte.semConexao')
  if (fonte === 'hub') return um(true, 'voz.ajustes.fonte.hub')
  if (fonte === 'nenhuma') {
    const chave = comAbas ? 'voz.ajustes.fonte.nenhumaAbas' : podeGerirNoHub(store) ? 'voz.ajustes.fonte.nenhumaGerir' : 'voz.ajustes.fonte.nenhumaAqui'
    return um(false, chave)
  }
  return um(false, 'voz.ajustes.fonte.consultando')
}

/** Observa as chaves deste aparelho enquanto a tela está aberta. */
function useEstadoAparelho() {
  const estado = useMemo(() => estadoVozAparelho(), [])
  useEffect(() => estado.observar(), [estado])
  useModel(estado)
  return estado
}

function LinhaFonte({ ok, partes, p }: { ok: boolean; partes: Mensagem[]; p?: VoiceProvider }) {
  const t = useT()
  const texto = textosVoz(t, partes, p)
  return (
    <div className="settings-row voz-fonte">
      <Icon name={ok ? 'checkmark.seal' : 'info.circle'} size={13} color="var(--context-ink-2, var(--ink-2))" />
      <Text size="footnote" tone="secondary" grow>{texto}</Text>
    </div>
  )
}

/** Escolha de modelo com o que já estava escolhido sempre na lista, e aviso se o provedor o tirou. */
function Modelo({ label, detail, selecao, lista, aoMudar, p }: { label: string; detail?: string; selecao: string; lista: string[]; aoMudar: (m: string) => void; p: VoiceProvider }) {
  const t = useT()
  const language = superficieIdioma(useIdioma())
  const opcoes = lista.includes(selecao) ? lista : [selecao, ...lista]
  return (
    <VStack gap={6}>
      <ValueRow label={label} detail={detail}>
        <ChoicePicker language={language} selection={selecao} options={opcoes.map((m) => ({ id: m, label: ltr(m) }))} onChange={aoMudar} />
      </ValueRow>
      {modeloSumiu(selecao, lista) ? (
        <Text size="footnote" tone="attention">{textoVoz(t, { chave: 'voz.ajustes.modeloSumiu' }, p)}</Text>
      ) : null}
    </VStack>
  )
}

function usarModelo(p: VoiceProvider, tipo: voice.ModelKind) {
  const [modelo, setModelo] = useState(() => voice.chosenModel(p, tipo))
  useEffect(() => setModelo(voice.chosenModel(p, tipo)), [p, tipo])
  return [modelo, (m: string) => { setModelo(m); voice.saveModel(p, tipo, m) }] as const
}

function listaDe(estado: EstadoVozHub | null, p: VoiceProvider, tipo: 'models' | 'transcribe' | 'speech') {
  return estado?.modelos.get(p)?.[tipo] ?? voice.modelsCache(p).get()?.[tipo] ?? []
}

function PausaRow() {
  const t = useT()
  const idioma = useIdioma()
  const [pause, setPause] = usePref(voice.pause)
  const s = (n: number) => ({ id: n, label: t('voz.ajustes.segundos', { n: formatarNumero(idioma, n) }) })
  return (
    <ValueRow label={t('voz.ajustes.pausa')} detail={t('voz.ajustes.pausaDetalhe')}>
      <ChoicePicker language={superficieIdioma(idioma)} selection={pause} options={[s(1), s(1.5), s(2.5)]} onChange={setPause} />
    </ValueRow>
  )
}

/**
 * A tela Voz. `reserva`: o Hub do bot da conversa. `folha`: aberta fora de Ajustes (barra da chamada), com tudo.
 * `aba` (tela de Ajustes, ajustes-tela-01): só o cartão daquela parte; sem ela, as três em sequência.
 */
export function VozPage({ reserva, folha = false, aba = null }: { reserva: BotStore | null; folha?: boolean; aba?: 'ouvir' | 'vivo' | 'chaves' | null }) {
  useScreenViewed('voice_settings')
  const t = useT()
  const idiomaApp = useIdioma()
  const f = frasesAparelho(t, mac)
  const app = useApp()
  const estado = useEstadoHub(reserva)
  const chavesAqui = useEstadoAparelho().estado
  const ditado = window.codync.speech.available
  const [aoVivo, setAoVivo] = usePref(voice.engine)
  const [ouvirSalvo, setOuvir] = usePref(voice.listen)
  const ouvir: EscolhaVoz = ouvirSalvo ?? 'device'
  const provedorVivo = voice.isVoiceProvider(aoVivo) ? aoVivo : null
  const provedorOuvir = voice.isVoiceProvider(ouvir) ? ouvir : null
  const [modo, setModo] = usePref(voice.mode(provedorVivo ?? 'openai'))
  const fala = provedorVivo !== null && modo === 'speech'
  const [rate, setRate] = usePref(voice.rate)
  const [language, setLanguage] = usePref(voice.language)
  // R1-02 do #590 (D507): só os códigos ficam guardados; os nomes saem no idioma do app a cada desenho.
  const locais = useMemo(() => window.codync.speech.locales(), [])
  const idiomas = voice.languageOptions(locais, idiomaApp)
  const hubs = app.managedStores

  /** O modo em que o provedor é usado no Ao vivo: Tempo real ou, no Fala, lendo. */
  const modoVivo = (p: VoiceProvider): ModoVoz => (voice.mode(p).get() === 'speech' ? 'ler' : 'tempo-real')
  const fonte = (modo: ModoVoz, p: VoiceProvider) => fonteNoModo(modo, chavesAqui, reserva, estado, p)
  const opcaoPaga = (p: VoiceProvider, custo: ChaveTexto, modo: ModoVoz) => ({ id: p, label: voice.voiceInfo[p].name, detail: t('voz.ajustes.opcao.detalhe', { custo: t(custo as 'voz.ajustes.custoUso'), selo: textoVoz(t, { chave: selo(reserva, fonte(modo, p)) }) }) })
  const linha = (modo: ModoVoz, p: VoiceProvider) => linhaDaFonte(reserva, fonte(modo, p), p, modo, ditado, fonteDe(reserva, estado, p) === 'hub', aba !== null)
  const mostra = (parte: 'ouvir' | 'vivo' | 'chaves') => aba === null || aba === parte

  const corpo = (
    <CardForm>
      <div className="settings-row voz-topo">
        <Text tone="secondary" grow>{t('voz.ajustes.topo', { neste: f.neste })}</Text>
        <Pill outlined><HStack as="span" gap={5}><Icon name="desktopcomputer" size={11} />{f.aparelho}</HStack></Pill>
      </div>

      {mostra('ouvir') ? <CardSection
        title={t('voz.ajustes.vozParaTexto')}
        footer={t(mac ? 'voz.ajustes.vozParaTexto.rodapeMac' : 'voz.ajustes.vozParaTexto.rodape')}
      >
        {[
          <ChoiceList
            key="ouvir"
            label={t('voz.ajustes.vozParaTexto')}
            selection={ouvir}
            onChange={setOuvir}
            options={[
              ...(ditado ? [{ id: 'device' as const, label: t('voz.ajustes.ditado'), detail: t('voz.ajustes.ditadoDetalhe') }] : []),
              opcaoPaga('openai', 'voz.ajustes.custoUso', 'ouvir'),
              opcaoPaga('gemini', 'voz.ajustes.custoUso', 'ouvir'),
            ]}
          />,
          provedorOuvir
            ? <LinhaFonte key="fonte" {...linha('ouvir', provedorOuvir)} p={provedorOuvir} />
            : <LinhaFonte key="fonte" {...linhaOuvirLocal(ditado, onDeviceUnavailableReason())} />,
          ...(provedorOuvir
            ? [<ModeloOuvir key={`stt-${provedorOuvir}`} p={provedorOuvir} estado={estado} />]
            : !ditado
              ? []
              : [
                  <ValueRow key="idioma" label={t('voz.ajustes.idioma')} detail={t('voz.ajustes.idiomaDetalhe')}>
                    <ChoicePicker language={superficieIdioma(idiomaApp)} selection={language} options={idiomas} onChange={setLanguage} />
                  </ValueRow>,
                ]),
        ]}
      </CardSection> : null}

      {mostra('vivo') ? <CardSection
        title={t('voz.ajustes.aoVivo')}
        footer={t(mac ? 'voz.ajustes.aoVivo.rodapeMac' : 'voz.ajustes.aoVivo.rodape')}
      >
        {[
          <VStack key="vivo" gap={8}>
            <Text size="footnote" tone="secondary">
              {t(mac ? 'voz.ajustes.aoVivo.textoMac' : 'voz.ajustes.aoVivo.texto')}
            </Text>
            <ChoiceList
              label={t('voz.ajustes.aoVivo')}
              selection={aoVivo}
              onChange={setAoVivo}
              options={[
                ...(ditado ? [{ id: 'device', label: t('voz.ajustes.vozMacos'), detail: t('voz.ajustes.vozMacosDetalhe') }] : []),
                opcaoPaga('openai', 'voz.ajustes.custoAudio', modoVivo('openai')),
                opcaoPaga('gemini', 'voz.ajustes.custoUso', modoVivo('gemini')),
              ]}
            />
          </VStack>,
          ...(fala && !ditado && !provedorOuvir
            ? [
                <div key="sem-ouvir" className="settings-row voz-aviso" role="note">
                  <Icon name="exclamationmark.circle" size={13} color="var(--attention, var(--ink-2))" />
                  <Text size="footnote" tone="attention" grow>{t(aba ? TEXTO_FALA_SEM_OUVIR_ABAS : TEXTO_FALA_SEM_OUVIR)}</Text>
                </div>,
              ]
            : []),
          ...(ditado && avisoLeituraGratis(provedorVivo ?? 'device', ouvir)
            ? [
                <div key="gratis" className="settings-row voz-aviso" role="note">
                  <Icon name="exclamationmark.circle" size={13} color="var(--attention, var(--ink-2))" />
                  <Text size="footnote" tone="attention" grow>{t(TEXTO_LEITURA_GRATIS)}</Text>
                </div>,
              ]
            : []),
          provedorVivo
            ? <LinhaFonte key="fonte" {...linha(fala ? 'ler' : 'tempo-real', provedorVivo)} p={provedorVivo} />
            : ditado
              ? <LinhaFonte key="fonte" ok partes={[{ chave: 'voz.ajustes.fonte.macosLer' }]} />
              : <LinhaFonte key="fonte" ok={false} partes={[{ chave: 'voz.ajustes.fonte.escolhaVivo' }]} />,
          ...(provedorVivo
            ? [
                <ValueRow key="modo" label={t('voz.ajustes.jeito')} detail={t(fala ? 'voz.ajustes.jeito.fala' : 'voz.ajustes.jeito.tempoReal')}>
                  <SegmentedChoice selection={modo === 'speech' ? 'speech' : 'realtime'} options={[{ id: 'realtime', label: t('voz.ajustes.tempoReal') }, { id: 'speech', label: t('voz.ajustes.fala') }]} onChange={setModo} />
                </ValueRow>,
                <ModeloVivo key={`model-${provedorVivo}-${fala ? 'fala' : 'tr'}`} p={provedorVivo} fala={fala} estado={estado} />,
                <VozRow key={`voz-${provedorVivo}`} p={provedorVivo} />,
              ]
            : ditado
              ? [
                  <ValueRow key="rate" label={t('voz.ajustes.velocidade')}>
                    <ChoicePicker language={superficieIdioma(idiomaApp)} selection={rate} options={[{ id: 0.42, label: t('voz.ajustes.velocidade.devagar') }, { id: voice.DEFAULT_RATE, label: t('voz.ajustes.velocidade.normal') }, { id: 0.56, label: t('voz.ajustes.velocidade.rapido') }]} onChange={setRate} />
                  </ValueRow>,
                ]
              : []),
          ...(provedorVivo && !fala ? [] : [<PausaRow key="pausa" />]),
          <div key="limites" className="settings-row">
            <Text tone="text">{t('voz.ajustes.limites')}</Text>
            <Spacer min={12} />
            <HStack gap={6} className="voz-limites">
              <Icon name="clock" size={13} color="var(--context-ink-2, var(--ink-2))" />
              <Text size="footnote" tone="secondary">{t('voz.ajustes.limitesTexto')}</Text>
            </HStack>
          </div>,
        ]}
      </CardSection> : null}

      {mostra('chaves') ? <>
        {capacidades(window.codync).cofreLocal ? <ChavesDoAparelho reserva={reserva} estadoHub={estado} /> : null}
        {hubs.map((hub) => <ChavesNoHub key={hub.computer.id} store={hub} />)}
      </> : null}
    </CardForm>
  )

  // D454: a tela de voz (em Ajustes ou na folha da chamada) declara o idioma do app.
  const tela = <div className="idioma-superficie voz-pagina" {...superficieIdioma(idiomaApp)} style={{ display: 'contents' }}>{corpo}</div>
  if (!folha) return tela
  // A folha (portal no body): o cabeçalho também fica na superfície do idioma.
  return (
    <SheetPage className="settings-voice-sheet">
      <div className="idioma-superficie voz-pagina" {...superficieIdioma(idiomaApp)} style={{ display: 'contents' }}>
        <ModalHeader icon="waveform" title={t('voz.ajustes.voz')} closeTitle={t('voz.ajustes.fechar')} />
        <div className="settings-voice-body">{corpo}</div>
      </div>
    </SheetPage>
  )
}

function ModeloOuvir({ p, estado }: { p: VoiceProvider; estado: EstadoVozHub | null }) {
  const t = useT()
  const [modelo, setModelo] = usarModelo(p, 'transcribe')
  return <Modelo label={t('voz.ajustes.modelo')} selecao={modelo} lista={listaDe(estado, p, 'transcribe')} aoMudar={setModelo} p={p} />
}

function ModeloVivo({ p, fala, estado }: { p: VoiceProvider; fala: boolean; estado: EstadoVozHub | null }) {
  const t = useT()
  const [modelo, setModelo] = usarModelo(p, fala ? 'speech' : 'realtime')
  // MX-8: o uso do mês vira a nota da própria linha; o provedor já está escolhido logo acima.
  return <Modelo label={t(fala ? 'voz.ajustes.modeloLeitura' : 'voz.ajustes.modelo')} detail={t.plural('voz.ajustes.esteMes', voice.voiceMinutes(p), { neste: frasesAparelho(t, mac).neste })} selecao={modelo} lista={listaDe(estado, p, fala ? 'speech' : 'models')} aoMudar={setModelo} p={p} />
}

function VozRow({ p }: { p: VoiceProvider }) {
  const t = useT()
  const language = superficieIdioma(useIdioma())
  const [escolhida, setEscolhida] = useState(() => voice.chosenVoice(p))
  return (
    <ValueRow label={t('voz.ajustes.voz')}>
      <ChoicePicker language={language} selection={escolhida} options={voice.voiceInfo[p].voices.map((v) => ({ id: v, label: ltr(v) }))} onChange={(v) => { setEscolhida(v); voice.saveVoice(p, v) }} />
    </ValueRow>
  )
}

/**
 * "Chaves no Hub <nome>": a gestão de hoje da chave do Hub (setVoiceKey, LOCAL_ONLY), só para Hubs que
 * este app alcança no próprio computador ou pelo túnel SSH. Sai quando Contas de IA assumir (contas-voz-01/03).
 */
export function ChavesNoHub({ store: source }: { store: BotStore }) {
  const store = useModel(source)
  const estado = useEstadoHub(source)
  // O Hub que roda neste aparelho (não o do túnel SSH): o cofre dele não é o do app.
  const mesmoAparelho = useApp().local?.computer.id === store.computer.id
  const t = useT()
  if (!store.isLoopback) return null
  return (
    <CardSection title={t('voz.ajustes.hub.titulo')} footer={<RodapeCurto resumo={textoVoz(t, resumoChavesNoHub())} completo={textoVoz(t, rodapeChavesNoHub(mesmoAparelho))} />}>
      {voice.VOICE_PROVIDERS.map((p) => <ChaveNoHub key={p} store={store} estado={estado} p={p} />)}
    </CardSection>
  )
}

/** A folha do botão de onda em Computadores: as chaves de voz daquele Hub. */
export function ChavesNoHubFolha({ store }: { store: BotStore }) {
  const t = useT()
  const idioma = useIdioma()
  return (
    <SheetPage className="settings-voice-sheet">
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
        <ModalHeader icon="waveform" title={t('voz.ajustes.hub.folha')} closeTitle={t('voz.ajustes.fechar')} />
        <div className="settings-voice-body">
          <CardForm>
            <ChavesNoHub store={store} />
          </CardForm>
        </div>
      </div>
    </SheetPage>
  )
}

function ChaveNoHub({ store, estado, p }: { store: BotStore; estado: EstadoVozHub | null; p: VoiceProvider }) {
  const t = useT()
  const info = voice.voiceInfo[p]
  const online = store.connection.kind === 'online'
  const fonte = online ? fonteDaChave(estado?.status, p) : 'desconhecida'
  const [editando, setEditando] = useState(false)
  const [chave, setChave] = useState('')
  const [ocupado, setOcupado] = useState(false)
  const [resultado, setResultado] = useState<Resultado | null>(null)
  const ocupadoRef = useRef(false)

  const rodar = (trabalho: () => Promise<Resultado & { status?: voice.VoiceStatus }>) => {
    if (ocupadoRef.current) return
    ocupadoRef.current = true
    setOcupado(true)
    setResultado(null)
    void trabalho()
      .then((r) => {
        if (r.status) voice.rememberStatus(r.status)
        if (r.ok && r.status) setEditando(false)
        setResultado(r)
      })
      .finally(() => {
        ocupadoRef.current = false
        setOcupado(false)
      })
  }
  const guardar = () => rodar(() => guardarChave(store, liveClient(store), p, chave, () => setChave('')))
  const cancelar = () => { setChave(''); setEditando(false); setResultado(null) }
  // Fechar a tela no meio da digitação não deixa a chave em estado nenhum.
  useEffect(() => () => setChave(''), [])

  const tag = fonte === 'hub' ? t('voz.ajustes.hub.tag') : t(fonte === 'nenhuma' ? 'voz.ajustes.semChave' : online ? 'voz.ajustes.consultando' : 'voz.ajustes.semConexao')
  const detalhe = fonte === 'hub'
    ? textoVoz(t, { chave: 'voz.ajustes.hub.detalhe' }, p)
    : fonte === 'nenhuma'
      ? t(window.codync.speech.available ? 'voz.ajustes.hub.semChaveMacos' : 'voz.ajustes.hub.semChaveNada')
      : online ? '' : t('voz.ajustes.hub.semConexao')

  return (
    <VStack gap={8}>
      <div className="settings-row">
        <span className="voz-marca" aria-hidden><Icon name="server.rack" size={12} /></span>
        <VStack grow className="settings-stack">
          <HStack gap={6}>
            <Text tone="text">{info.name}</Text>
            <span className={`voz-tag${fonte === 'hub' ? ' tem' : ''}`}>{tag}</span>
          </HStack>
          {detalhe ? <Text size="caption" tone="tertiary">{detalhe}</Text> : null}
        </VStack>
        {ocupado ? <Spinner /> : !online || fonte === 'desconhecida' ? null : editando ? null : fonte === 'hub' ? (
          <HStack gap={12}>
            <button className="settings-quiet-button" onClick={() => rodar(() => testarChave(liveClient(store), p))}>{t('voz.ajustes.testar')}</button>
            <button className="settings-quiet-button" onClick={() => { setEditando(true); setResultado(null) }}>{t('voz.ajustes.trocar')}</button>
            <button className="settings-quiet-button voz-perigo" onClick={() => rodar(() => limparChave(store, liveClient(store), p))}>{t('voz.ajustes.limpar')}</button>
          </HStack>
        ) : (
          <button className="settings-quiet-button" onClick={() => { setEditando(true); setResultado(null) }}>{t('voz.ajustes.adicionar')}</button>
        )}
      </div>
      {editando ? (
        // A chave do Hub ainda entra por este campo da tela, como antes; some do campo assim que é enviada.
        <VStack gap={8} className="settings-appear">
          <HStack gap={8} className="settings-row">
            <input
              type="password"
              aria-label={textoVoz(t, { chave: 'voz.ajustes.hub.campo' }, p)}
              placeholder={info.keyPrompt}
              value={chave}
              spellCheck={false}
              autoComplete="off"
              className="settings-key-input"
              dir="ltr"
              onChange={(e) => setChave(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && chave.trim() && guardar()}
            />
            {ocupado ? <Spinner /> : <Button onClick={guardar} disabled={!chave.trim()}>{t('voz.ajustes.guardar')}</Button>}
          </HStack>
          <Text as="div" size="footnote" className="settings-row">
            <button className="settings-link" onClick={() => window.codync.app.openExternal(info.keyPage)}>{textoVoz(t, { chave: 'voz.ajustes.conseguir' }, p)}</button>
            <Spacer />
            <button className="settings-quiet-button" onClick={cancelar}>{t('voz.ajustes.cancelar')}</button>
          </Text>
        </VStack>
      ) : null}
      <Reveal show={resultado !== null}>
        {resultado ? (
          <HStack as="span" gap={5} className={`settings-key-result ${resultado.ok ? '' : 'failed'}`}>
            <Icon name={resultado.ok ? 'checkmark.circle.fill' : 'exclamationmark.circle.fill'} size={10} />
            <bdi dir="auto">{textosVoz(t, resultado.partes, p)}</bdi>
          </HStack>
        ) : null}
      </Reveal>
    </VStack>
  )
}

/**
 * voz-04: "Chaves deste Mac" (quadros 01, 01b e 01s). A chave da OpenAI ou do Gemini guardada no cofre deste
 * aparelho, com Adicionar, Trocar, Testar e Limpar. O campo de senha é desta tela, isolado do resto do
 * estado, e fica vazio assim que a chave é enviada; depois de guardada, a chave não volta à tela.
 */
function ChavesDoAparelho({ reserva, estadoHub }: { reserva: BotStore | null; estadoHub: EstadoVozHub | null }) {
  const t = useT()
  const f = frasesAparelho(t, mac)
  const modelo = useEstadoAparelho()
  const estado = modelo.estado
  const semCofre = estado?.cofre === 'sem-cofre'
  return (
    <CardSection
      title={t('voz.ajustes.aparelho.titulo', { deste: f.deste })}
      footer={<RodapeCurto resumo={t('voz.ajustes.aparelho.resumo', { neste: f.neste })} completo={t('voz.ajustes.aparelho.completo', { neste: f.neste })} />}
    >
      {[
        ...(semCofre
          ? [
              <div key="sem-cofre" className="settings-row voz-aviso" role="note">
                <Icon name="lock.shield" size={13} color="var(--context-ink-2, var(--ink-2))" />
                <Text size="footnote" tone="secondary" grow>{t(TEXTO_SEM_COFRE, { aparelho: f.aparelho })}</Text>
              </div>,
            ]
          : []),
        ...voice.VOICE_PROVIDERS.map((p) => (
          <ChaveDoAparelho
            key={p}
            p={p}
            chave={estado ? estado.chaves[p] : null}
            semCofre={semCofre}
            hub={reserva && fonteDe(reserva, estadoHub, p) === 'hub' ? reserva.hostName : null}
            aplicar={(e) => modelo.aplicar(e)}
          />
        )),
      ]}
    </CardSection>
  )
}

function ChaveDoAparelho({ p, chave, semCofre, hub, aplicar }: { p: VoiceProvider; chave: ChaveAparelho | null; semCofre: boolean; hub: string | null; aplicar: (e: EstadoChaves | null) => void }) {
  const t = useT()
  const tv = (m: Mensagem) => textoVoz(t, m, p, mac)
  const info = voice.voiceInfo[p]
  const ponte = window.codync.voz
  const [editando, setEditando] = useState(false)
  const [confirmarLimpar, setConfirmarLimpar] = useState(false)
  // O único lugar onde a chave existe no renderer: este campo, durante a digitação.
  const [digitada, setDigitada] = useState('')
  const [ocupado, setOcupado] = useState(false)
  const [resultado, setResultado] = useState<ResultadoTela | null>(null)
  const ocupadoRef = useRef(false)
  // Fechar a tela no meio da digitação não deixa a chave em estado nenhum.
  useEffect(() => () => setDigitada(''), [])

  const rodar = (trabalho: () => Promise<ResultadoTela & { estado: EstadoChaves | null }>, fecharCampo = false) => {
    if (ocupadoRef.current) return
    ocupadoRef.current = true
    setOcupado(true)
    setResultado(null)
    void trabalho()
      .then((r) => {
        aplicar(r.estado)
        if (fecharCampo && (r.ok || r.estado?.chaves[p].situacao === 'incerta')) setEditando(false)
        setResultado(r)
      })
      .finally(() => {
        ocupadoRef.current = false
        setOcupado(false)
      })
  }
  const guardar = () => rodar(() => guardarNoAparelho(ponte, p, digitada, () => setDigitada('')), true)
  const agir = (acao: 'limpar' | 'testar' | 'reserva') => { setConfirmarLimpar(false); rodar(() => agirNoAparelho(ponte, acao, p, hub)) }
  const abrirCampo = () => { setEditando(true); setConfirmarLimpar(false); setResultado(null) }
  const cancelar = () => { setDigitada(''); setEditando(false); setResultado(null) }

  const situacao = chave?.situacao ?? null
  /** A pessoa escolheu a chave do Hub enquanto a daqui não abria: vale até reiniciar, Trocar, Limpar ou voltar. */
  const escolheuHub = !!chave?.reserva && situacao !== 'ausente'
  const pendente = textoSituacaoPendente(situacao)
  const naoAbriu = (situacao === 'travada' || situacao === 'recusada') && !escolheuHub
  const tag = situacao === null ? t('voz.ajustes.consultando')
    : pendente ? t(situacao === 'incerta' ? 'voz.ajustes.tag.incerta' : 'voz.ajustes.tag.limpeza')
      : escolheuHub ? (hub ? t('voz.ajustes.tag.usandoHubNome') : t('voz.ajustes.tag.usandoHub'))
      : situacao === 'guardada' ? tv({ chave: 'voz.ajustes.tag.guardada' })
        : naoAbriu ? t(situacao === 'travada' ? 'voz.ajustes.tag.naoAbriu' : 'voz.ajustes.tag.recusada')
          : hub ? t('voz.ajustes.tag.usandoHubNome') : t('voz.ajustes.semChave')
  // Frases inteiras e independentes, uma depois da outra (catálogo voz/ajustes).
  const enquanto: Mensagem = { chave: mac && window.codync.speech.available ? 'voz.ajustes.enquantoMac' : 'voz.ajustes.enquanto' }
  const escolheu = (curto: boolean): Mensagem => hub
    ? { chave: curto ? 'voz.ajustes.escolheuHubCurtoNome' : 'voz.ajustes.escolheuHubNome' }
    : { chave: curto ? 'voz.ajustes.escolheuHubCurto' : 'voz.ajustes.escolheuHub' }
  const partes: Mensagem[] = pendente ? [{ chave: pendente }, escolheuHub ? escolheu(true) : enquanto]
    : escolheuHub ? [escolheu(false)]
      : situacao === 'guardada' ? [{ chave: 'voz.ajustes.guardada.usada' }, { chave: mac && window.codync.speech.available ? 'voz.ajustes.guardada.falaMac' : 'voz.ajustes.guardada.fala' }]
        : naoAbriu ? [{ chave: situacao === 'travada' ? 'voz.ajustes.naoAbriu.travada' : 'voz.ajustes.naoAbriu.recusada' }, { chave: 'voz.ajustes.naoAbriu.continua' }, enquanto, ...(hub ? [{ chave: 'voz.ajustes.naoAbriu.soSeEscolher' } as Mensagem] : [])]
          : situacao === 'ausente' && !semCofre ? [{ chave: hub ? 'voz.ajustes.ausente.hub' : 'voz.ajustes.ausente' }]
            : []
  const detalhe = partes.length ? textosVoz(t, partes, p, mac) : ''
  const temArquivo = !!pendente || situacao === 'guardada' || situacao === 'travada' || situacao === 'recusada'
  const acoes = situacao === null || editando || confirmarLimpar ? null : temArquivo ? (
    <HStack gap={12}>
      {naoAbriu || situacao === 'incerta' ? <button className="settings-quiet-button" onClick={() => agir('testar')}>{t('voz.ajustes.tentarDeNovo')}</button> : null}
      {escolheuHub && !pendente ? <button className="settings-quiet-button" onClick={() => agir('testar')}>{tv({ chave: 'voz.ajustes.usarDaqui' })}</button> : null}
      {(naoAbriu || situacao === 'incerta') && !escolheuHub && hub ? <button className="settings-quiet-button" onClick={() => agir('reserva')}>{t('voz.ajustes.usarHub')}</button> : null}
      {situacao === 'guardada' && !escolheuHub ? <button className="settings-quiet-button" onClick={() => agir('testar')}>{t('voz.ajustes.testar')}</button> : null}
      {semCofre || pendente ? null : <button className="settings-quiet-button" onClick={abrirCampo}>{t('voz.ajustes.trocar')}</button>}
      <button className="settings-quiet-button voz-perigo" onClick={() => { if (situacao === 'limpeza-pendente') agir('limpar'); else { setConfirmarLimpar(true); setResultado(null) } }}>{t('voz.ajustes.limpar')}</button>
    </HStack>
  ) : semCofre ? null : (
    <button className="settings-quiet-button" onClick={abrirCampo}>{tv({ chave: 'voz.ajustes.adicionarAqui' })}</button>
  )

  return (
    <VStack gap={8}>
      <div className="settings-row">
        <span className="voz-marca aparelho" aria-hidden><Icon name="key.fill" size={11} /></span>
        <VStack grow className="settings-stack">
          <HStack gap={6}>
            <Text tone="text">{info.name}</Text>
            <span className={`voz-tag${escolheuHub ? ' tem' : situacao === 'guardada' ? ' segura' : naoAbriu || pendente ? ' alerta' : hub ? ' tem' : ''}`}>
              {situacao === 'guardada' && !escolheuHub ? <Icon name="checkmark.shield" size={10} /> : null}
              {tag}
            </span>
          </HStack>
          {detalhe ? <Text size="caption" tone="tertiary">{detalhe}</Text> : null}
        </VStack>
        {ocupado ? <Spinner /> : naoAbriu || pendente || escolheuHub ? null : acoes}
      </div>
      {/* A chave que não abriu tem mais saídas (quadro 01b): ficam abaixo do texto, não espremidas ao lado. */}
      {(naoAbriu || pendente || escolheuHub) && !ocupado && acoes ? <HStack gap={12} className="settings-row voz-acoes">{acoes}</HStack> : null}
      {editando ? (
        <VStack gap={8} className="settings-appear">
          <HStack gap={8} className="settings-row">
            <input
              type="password"
              aria-label={tv({ chave: 'voz.ajustes.aparelho.campo' })}
              placeholder={info.keyPrompt}
              value={digitada}
              spellCheck={false}
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              className="settings-key-input"
              dir="ltr"
              onChange={(e) => setDigitada(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && digitada.trim() && guardar()}
            />
            <button className="settings-quiet-button" onClick={cancelar}>{t('voz.ajustes.cancelar')}</button>
            {ocupado ? <Spinner /> : <Button onClick={guardar} disabled={!digitada.trim()}>{t('voz.ajustes.guardar')}</Button>}
          </HStack>
          <Text as="div" size="footnote" tone="secondary" className="settings-row">
            <span>{tv({ chave: 'voz.ajustes.aparelho.ajuda' })}{' '}
              <button className="settings-link" onClick={() => window.codync.app.openExternal(info.keyPage)}>{t('voz.ajustes.ondePegar')}</button>
            </span>
          </Text>
        </VStack>
      ) : null}
      {confirmarLimpar ? (
        <div className="voz-confirmar settings-appear" role="group" aria-label={tv({ chave: 'voz.ajustes.limparAria' })}>
          <Icon name="exclamationmark.circle" size={13} color="var(--attention, var(--ink-2))" />
          <VStack gap={6} grow>
            <Text tone="text">{tv({ chave: 'voz.ajustes.limparPergunta' })}</Text>
            <Text size="footnote" tone="secondary">
              {textosVoz(t, [...(hub ? [{ chave: 'voz.ajustes.limparHub' } as Mensagem] : []), { chave: 'voz.ajustes.limparRemove' }], p, mac)}{' '}
              <b>{tv({ chave: 'voz.ajustes.limparVazou' })}</b>{' '}
              <button className="settings-link" onClick={() => window.codync.app.openExternal(info.keyPage)}>{t('voz.ajustes.abrirSite')}</button>
            </Text>
            <HStack gap={8}>
              <button className="styled-button voz-limpar" onClick={() => agir('limpar')}>{t('voz.ajustes.limpar')}</button>
              <Button kind="secondary" onClick={() => setConfirmarLimpar(false)}>{t('voz.ajustes.manter')}</Button>
            </HStack>
          </VStack>
        </div>
      ) : null}
      <Reveal show={resultado !== null}>
        {resultado ? (
          <HStack as="span" gap={5} className={`settings-key-result ${resultado.ok ? '' : 'failed'}`}>
            <Icon name={resultado.ok ? 'checkmark.circle.fill' : 'exclamationmark.circle.fill'} size={10} />
            <bdi dir="auto">{textosVoz(t, resultado.partes, p)}</bdi>
          </HStack>
        ) : null}
      </Reveal>
    </VStack>
  )
}
