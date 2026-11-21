import { useEffect, useRef, useState } from 'react'
import { Button, CardSection, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { liveClient } from '../../settings/parts'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { isolado } from '../../../../shared/bidi.ts'
import { MarketBadge } from '../../marketplace/pieces'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { amostraDeModelos, chaveDoErro, listarChaves, mostrarApiDireta, removerChave, salvarChave, testarChave, type Acesso, type FornecedorDaChave } from './api-direta'
import { entrarComOpenRouter, fraseDaFalha, FalhaOpenRouter } from './openrouter-login'
import { esquecerModelos } from '../seletor-modelo/catalogo'
import '../../settings/settings.css'

/** Conexões de IA → API direta: uma linha por fornecedor, com a chave guardada no cofre do Hub deste computador. */
export function ApiDireta() {
  const store = useModel(useStore())
  const t = useT()
  const idioma = useIdioma()
  const online = store.connection.kind === 'online' && !store.mismatch
  const visivel = mostrarApiDireta(store.hello?.backends ?? [], store.isLoopback)
  const [fornecedores, setFornecedores] = useState<FornecedorDaChave[] | null>(null)
  const [falhou, setFalhou] = useState(false)

  const reler = async () => {
    const client = liveClient(store)
    if (!client) return
    try {
      setFornecedores(await listarChaves(client))
      setFalhou(false)
    } catch {
      setFalhou(true)
    }
  }
  useEffect(() => {
    if (visivel && online) void reler()
  }, [visivel, online, store.hello?.hostId])

  if (!visivel) return null
  return (
    <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <CardSection title={t('contas.apiDireta.titulo')} footer={t('contas.apiDireta.rodape')}>
        {!online ? <Text tone="secondary">{t('contas.apiDireta.semConexao')}</Text> : null}
        {online && falhou ? <Text size="caption" tone="secondary" role="status">{t('contas.apiDireta.erroLista')}</Text> : null}
        {online && !fornecedores && !falhou ? <Spinner size={12} /> : null}
        {online ? fornecedores?.map((f) => <LinhaDoFornecedor key={f.id} fornecedor={f} aoMudar={reler} />) : null}
      </CardSection>
    </div>
  )
}

/** Uma linha de fornecedor da API direta: colar a chave ou, no OpenRouter, entrar pelo navegador. */
export function LinhaDoFornecedor({ fornecedor: f, aoMudar }: { fornecedor: FornecedorDaChave; aoMudar: () => Promise<void> }) {
  const store = useStore()
  const t = useT()
  const [editando, setEditando] = useState(false)
  const [chave, setChave] = useState('')
  const [ocupado, setOcupado] = useState(false)
  const [resultado, setResultado] = useState<Acesso | { ok: false; motivo: string } | { ok: false; frase: ChaveTexto } | null>(null)
  const [loginAberto, setLoginAberto] = useState(false)
  const ocupadoRef = useRef(false)
  const cancelarLoginRef = useRef<(() => void) | null>(null)
  // Sair da tela no meio da digitação não deixa a chave em estado nenhum.
  useEffect(() => () => setChave(''), [])

  const rodar = (
    trabalho: () => Promise<Acesso | { ok: boolean }>,
    depois?: (r: Acesso) => void,
    mapearFalha?: (e: unknown) => { ok: false; frase: ChaveTexto } | undefined,
  ) => {
    const client = liveClient(store)
    if (!client || ocupadoRef.current) return
    ocupadoRef.current = true
    setOcupado(true)
    setResultado(null)
    void trabalho()
      .then((r) => {
        if ('erro' in r || 'modelos' in r) {
          setResultado(r as Acesso)
          depois?.(r as Acesso)
        }
        // A escolha de modelo do bot relê a lista do motor na próxima abertura.
        esquecerModelos(store.computer.id, 'mexerica')
        return aoMudar()
      })
      .catch((e: unknown) => setResultado(mapearFalha?.(e) ?? { ok: false, motivo: e instanceof Error ? e.message : String(e) }))
      .finally(() => {
        ocupadoRef.current = false
        setOcupado(false)
        setLoginAberto(false)
        cancelarLoginRef.current = null
      })
  }
  const client = () => liveClient(store)!
  const salvar = () => {
    if (!chave.trim()) return
    rodar(() => salvarChave(client(), f.id, chave, () => setChave('')), (r) => r.ok && setEditando(false))
  }
  const entrar = () => {
    const client = liveClient(store)
    if (!client || ocupadoRef.current) return
    setEditando(false)
    const { fim, cancelar } = entrarComOpenRouter(client, window.codync.app.loginRetorno)
    cancelarLoginRef.current = cancelar
    setLoginAberto(true)
    rodar(() => fim, undefined, (e) => (e instanceof FalhaOpenRouter ? { ok: false, frase: fraseDaFalha(e.tipo) } : undefined))
  }
  const cancelar = () => { setChave(''); setEditando(false); setResultado(null) }
  const quantos = f.modelos?.length ?? 0
  const botaoEntrar = f.id === 'openrouter' ? (
    <button className="settings-quiet-button" onClick={entrar}>{t('contas.apiDireta.openrouter.entrar')}</button>
  ) : null

  return (
    <VStack gap={8}>
      <div className="settings-row">
        <VStack grow className="settings-stack">
          <HStack gap={6}>
            <Text tone="text" dir="auto">{isolado(f.nome)}</Text>
            <MarketBadge label={t(f.configurada ? 'contas.apiDireta.comChave' : 'contas.apiDireta.semChave')} tone={f.configurada ? 'added' : 'secondary'} icon={f.configurada ? 'checkmark' : undefined} />
          </HStack>
          {f.configurada && f.modelos ? <Text size="caption" tone="tertiary">{t.plural('contas.apiDireta.modelos', quantos)}</Text> : null}
        </VStack>
        {ocupado ? loginAberto ? (
          <HStack gap={12}>
            <Text size="caption" tone="secondary">{t('contas.apiDireta.openrouter.aguardando')}</Text>
            <button className="settings-quiet-button" onClick={() => cancelarLoginRef.current?.()}>{t('contas.apiDireta.cancelar')}</button>
          </HStack>
        ) : <Spinner /> : editando ? null : f.configurada ? (
          <HStack gap={12}>
            <button className="settings-quiet-button" onClick={() => rodar(() => testarChave(client(), f.id))}>{t('contas.apiDireta.testar')}</button>
            {botaoEntrar}
            <button className="settings-quiet-button" onClick={() => { setEditando(true); setResultado(null) }}>{t('contas.apiDireta.trocar')}</button>
            <button className="settings-quiet-button" onClick={() => rodar(() => removerChave(client(), f.id))}>{t('contas.apiDireta.remover')}</button>
          </HStack>
        ) : (
          <HStack gap={12}>
            {botaoEntrar}
            <button className="settings-quiet-button" onClick={() => { setEditando(true); setResultado(null) }}>{t('contas.apiDireta.adicionar')}</button>
          </HStack>
        )}
      </div>
      {editando ? (
        <VStack gap={8}>
          <HStack gap={8} className="settings-row">
            <input
              type="password"
              aria-label={t('contas.apiDireta.campo', { fornecedor: f.nome })}
              value={chave}
              spellCheck={false}
              autoComplete="off"
              className="settings-key-input"
              dir="ltr"
              onChange={(e) => setChave(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && salvar()}
            />
            {ocupado ? <Spinner /> : <Button onClick={salvar} disabled={!chave.trim()}>{t('contas.apiDireta.salvar')}</Button>}
          </HStack>
          <Text as="div" size="footnote" className="settings-row">
            <Text size="caption" tone="tertiary">{t('contas.apiDireta.dicaSalvar')}</Text>
            <Spacer />
            <button className="settings-quiet-button" onClick={cancelar}>{t('contas.apiDireta.cancelar')}</button>
          </Text>
        </VStack>
      ) : null}
      {resultado ? <ResultadoDoTeste resultado={resultado} /> : null}
    </VStack>
  )
}

/** O bloco de resultado de Testar acesso, partilhado pelas chaves coladas e pelo login do OpenRouter. */
export function ResultadoDoTeste({ resultado }: { resultado: Acesso | { ok: false; motivo: string } | { ok: false; frase: ChaveTexto } }) {
  const t = useT()
  let texto: string
  if (resultado.ok) {
    const { nomes, restantes } = amostraDeModelos(resultado.modelos)
    texto = resultado.modelos.length === 0
      ? t('contas.apiDireta.okSemModelos')
      : restantes > 0 ? t('contas.apiDireta.okComMais', { nomes, n: restantes }) : t('contas.apiDireta.ok', { nomes })
  } else if ('frase' in resultado) {
    texto = (t as unknown as (chave: ChaveTexto) => string)(resultado.frase)
  } else if ('motivo' in resultado) {
    texto = t('contas.apiDireta.erro.desconhecido', { motivo: resultado.motivo })
  } else {
    texto = (t as unknown as (chave: ChaveTexto) => string)(chaveDoErro(resultado.erro))
  }
  return (
    <HStack as="span" gap={5} role="status" className={`settings-key-result ${resultado.ok ? '' : 'failed'}`}>
      <Icon name={resultado.ok ? 'checkmark.circle.fill' : 'exclamationmark.circle.fill'} size={10} />
      <Text size="caption" className="selectable">{texto}</Text>
    </HStack>
  )
}
