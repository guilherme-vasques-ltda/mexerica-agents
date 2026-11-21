// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useState } from 'react'
import { Button, CardForm, CardSection, IconButton, SegmentedChoice, Spinner } from '../../components/Controls'
import { ModalHeader, SheetPage, useDismiss } from '../../components/Overlay'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { mesmaOrigem, origemDe, resultadoIncerto } from '../fork/meus-mcp/estado'
import { errorText, needsSignIn, type InstalledConnector } from './market-models'
import { ErrorText, Field } from './pieces'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { isolado } from '@shared/bidi'
import { usePlugins } from './plugins'
import { cru, frase, textoDe, type Frase } from '../../lib/fork-i18n/frase'

// D175: the MCP Registry is gone; a connection is always native, described by the user here. Saving
// doesn't sign in, turn it on for a bot or prove its tools: it starts off for every bot.

type Mode = 'command' | 'url' | 'config'

// Exemplos de código nos campos: iguais em todo idioma e sempre da esquerda para a direita (Field mono).
const EXEMPLO_CONFIG = '{ "mcpServers": { … } }' // textos-ok: exemplo de código (JSON), igual em todo idioma
const EXEMPLO_URL = 'https://example.com/mcp' // textos-ok: exemplo de URL, igual em todo idioma
const EXEMPLO_COMANDO = 'npx -y @scope/server' // textos-ok: exemplo de comando, igual em todo idioma
const EXEMPLO_CABECALHO = 'Authorization: Bearer …' // textos-ok: exemplo de cabeçalho HTTP, igual em todo idioma
const EXEMPLO_AMBIENTE = 'API_KEY=…' // textos-ok: exemplo de variável de ambiente, igual em todo idioma

/**
 * What this sheet saw of its own write: `unanswered` (the Hub never answered; `plugins.writeGate` says
 * whether a successful read of the list has come since) or `moved` (the link changed before the reply:
 * nothing more from this sheet).
 */
type Uncertain = 'unanswered' | 'moved'

export function CustomConnectorSheet() {
  const store = useStore()
  const plugins = usePlugins(store)
  const dismiss = useDismiss()
  const t = useT()
  const idioma = useIdioma()
  const [name, setName] = useState('')
  const [mode, setMode] = useState<Mode>('command')
  const [target, setTarget] = useState('')
  const [envText, setEnvText] = useState('')
  const [config, setConfig] = useState('')
  const [saving, setSaving] = useState(false)
  // Lição da R1 do #572: o erro escrito aqui fica como chave + parâmetros e é traduzido ao mostrar.
  const [error, setError] = useState<Frase | null>(null)
  const [uncertain, setUncertain] = useState<Uncertain | null>(null)
  const [rereading, setRereading] = useState(false)
  /** Saved, but its sign-in didn't finish: adding again would make a second copy. */
  const [saved, setSaved] = useState(false)
  // D175 F2: an add the Hub never answered, here or in an earlier sheet, holds new adds until a successful
  // read of the list comes after it; a failed re-read never counts.
  const gate = plugins.writeGate
  const filled = mode === 'config' ? config.trim() !== '' : name.trim() !== '' && target.trim() !== ''
  const ready = filled && !saved && uncertain !== 'moved' && gate.state === 'free' && !rereading

  const computador = isolado(store.hostName)
  const footer = {
    command: t('extensoes.mcp.rodapeComando', { computador }),
    url: t('extensoes.mcp.rodapeUrl'),
    config: t('extensoes.mcp.rodapeConfig', { computador }),
  }[mode]

  const save = async () => {
    // The gesture belongs to this Hub, link and generation; a reply after a change isn't shown as this one's.
    const origin = origemDe(store)
    if (!origin) {
      setError(frase('extensoes.mcp.semConexao'))
      return
    }
    setSaving(true)
    setError(null)
    setUncertain(null)
    const remote = mode === 'url'
    const pairs: Record<string, string> = {}
    for (const line of envText.split('\n')) {
      const at = line.indexOf(remote ? ':' : '=')
      if (at < 0) continue
      const key = line.slice(0, at).trim()
      if (key) pairs[key] = line.slice(at + 1).trim()
    }
    let added: InstalledConnector[]
    try {
      added = mode === 'config'
        ? await plugins.importConnectors(config)
        : [await plugins.addConnector({ name, command: remote ? null : target, url: remote ? target : null, env: remote ? {} : pairs, headers: remote ? pairs : {} })]
    } catch (e) {
      if (!mesmaOrigem(store, origin)) setUncertain('moved')
      // The list was already read again (plugins.write); writeGate says whether that read succeeded.
      else if (resultadoIncerto(e)) setUncertain('unanswered')
      else setError(cru(errorText(e)))
      setSaving(false)
      return
    }
    if (!mesmaOrigem(store, origin)) {
      setUncertain('moved')
      setSaving(false)
      return
    }
    try {
      // Same gesture: a server that asks for sign-in opens its own native sign-in page.
      for (const c of added) if (needsSignIn(c)) await plugins.signIn(c.id)
      dismiss()
    } catch (e) {
      setSaved(true)
      setError(frase('extensoes.mcp.salvoSemLogin', { erro: errorText(e) }))
    }
    setSaving(false)
  }

  /** Reads the list again, nothing else: the unanswered write is never repeated. */
  const reread = async () => {
    setRereading(true)
    try {
      await plugins.reread()
    } finally {
      setRereading(false)
    }
  }

  // Três frases inteiras do catálogo, juntas por espaço (cada uma tem a pontuação dela).
  const uncertainText = uncertain === 'moved'
    ? t('extensoes.mcp.mudouAntes')
    : gate.state === 'pending'
      ? [
          gate.moved ? t('extensoes.mcp.mudouDepois') : t('extensoes.mcp.naoConfirmou'),
          gate.readError ? t('extensoes.mcp.releituraFalhou', { erro: isolado(gate.readError.replace(/[.\s]+$/, '')) }) : t('extensoes.mcp.naoRelida'),
          t('extensoes.mcp.releia'),
        ].join(' ')
      : uncertain === 'unanswered'
        ? t('extensoes.mcp.naoConfirmouRelida')
        : null

  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ModalHeader icon="point.3.connected.trianglepath.dotted" title={t('extensoes.adicionarMcp')} closeTitle={t('extensoes.fechar')} trailing={saving ? <Spinner /> : <IconButton title={t('extensoes.adicionar')} icon="plus" disabled={!ready} onClick={() => void save()} />} />
      <CardForm>
        <CardSection footer={footer}>
          <SegmentedChoice selection={mode} onChange={setMode} options={[{ id: 'command', label: t('extensoes.mcp.comando') }, { id: 'url', label: t('extensoes.mcp.url') }, { id: 'config', label: t('extensoes.mcp.config') }]} />
          {mode === 'config' ? <Field value={config} onChange={setConfig} placeholder={EXEMPLO_CONFIG} lines={[6, 16]} mono /> : null}
          {mode !== 'config' ? <Field value={name} onChange={setName} placeholder={t('extensoes.mcp.nome')} /> : null}
          {mode !== 'config' ? <Field value={target} onChange={setTarget} placeholder={mode === 'url' ? EXEMPLO_URL : EXEMPLO_COMANDO} mono /> : null}
        </CardSection>
        {mode === 'url' ? (
          <CardSection title={t('extensoes.mcp.cabecalhos')} footer={t('extensoes.mcp.cabecalhosRodape', { computador })}>
            <Field value={envText} onChange={setEnvText} placeholder={EXEMPLO_CABECALHO} lines={[2, 6]} mono />
          </CardSection>
        ) : mode === 'command' ? (
          <CardSection title={t('extensoes.mcp.ambiente')} footer={t('extensoes.mcp.ambienteRodape', { computador })}>
            <Field value={envText} onChange={setEnvText} placeholder={EXEMPLO_AMBIENTE} lines={[2, 6]} mono />
          </CardSection>
        ) : null}
        <CardSection>
          <Text size="caption" weight="semibold" tone="text">{t('extensoes.mcp.naoVerificado')}</Text>
          <Text size="caption" tone="secondary">{t('extensoes.mcp.salvarNaoLiga')}</Text>
        </CardSection>
        {uncertainText ? (
          <CardSection>
            <ErrorText text={uncertainText} />
            {gate.state === 'pending' && uncertain !== 'moved' ? (
              <Button kind="secondary" disabled={rereading} onClick={() => void reread()}>{rereading ? t('extensoes.mcp.relendo') : t('extensoes.mcp.reler')}</Button>
            ) : null}
          </CardSection>
        ) : null}
        {error ? <CardSection><ErrorText text={textoDe(error, idioma)} /></CardSection> : null}
      </CardForm>
      </div>
    </SheetPage>
  )
}
