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
import { amostraDeModelos, aoMudarChaves, chaveDoErro, chavesMudaram, estadoDoTeste, listarChaves, mostrarApiDireta, removerChave, salvarChave, testarChave, type Acesso, type FornecedorDaChave } from './api-direta'
import { esquecerModelos } from '../seletor-modelo/catalogo'
import { ehOpen } from './conexoes'
import '../../settings/settings.css'

/**
 * Conexões de IA → Chaves de API: uma linha por chave já salva no cofre do Hub deste computador (testar, trocar, remover).
 * Para ligar uma chave nova, «Adicionar conexão». Na edição Open nunca aparece.
 */
export function ApiDireta() {
  const store = useModel(useStore())
  const t = useT()
  const idioma = useIdioma()
  const online = store.connection.kind === 'online' && !store.mismatch
  const visivel = !ehOpen(window.codync) && mostrarApiDireta(store.hello?.backends ?? [], store.isLoopback)
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
  useEffect(() => (visivel && online ? aoMudarChaves(() => void reler()) : undefined), [visivel, online, store])

  if (!visivel) return null
  return (
    <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <CardSection title={t('contas.apiDireta.titulo')} footer={t('contas.apiDireta.rodape')}>
        {!online ? <Text tone="secondary">{t('contas.apiDireta.semConexao')}</Text> : null}
        {online && falhou ? <Text size="caption" tone="secondary" role="status">{t('contas.apiDireta.erroLista')}</Text> : null}
        {online && !fornecedores && !falhou ? <Spinner size={12} /> : null}
        {online && fornecedores && !fornecedores.some((f) => f.configurada) ? <Text size="caption" tone="secondary">{t('contas.apiDireta.vazio')}</Text> : null}
        {online ? fornecedores?.filter((f) => f.configurada).map((f) => <LinhaDoFornecedor key={f.id} fornecedor={f} />) : null}
      </CardSection>
    </div>
  )
}

export function LinhaDoFornecedor({ fornecedor: f }: { fornecedor: FornecedorDaChave }) {
  const store = useStore()
  const t = useT()
  const [editando, setEditando] = useState(false)
  const [chave, setChave] = useState('')
  const [ocupado, setOcupado] = useState(false)
  const [resultado, setResultado] = useState<Acesso | { ok: false; motivo: string } | null>(null)
  const ocupadoRef = useRef(false)
  // Sair da tela no meio da digitação não deixa a chave em estado nenhum.
  useEffect(() => () => setChave(''), [])

  const rodar = (trabalho: () => Promise<Acesso | { ok: boolean }>, depois?: (r: Acesso) => void) => {
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
        chavesMudaram()
      })
      .catch((e: unknown) => setResultado({ ok: false, motivo: e instanceof Error ? e.message : String(e) }))
      .finally(() => {
        ocupadoRef.current = false
        setOcupado(false)
      })
  }
  const client = () => liveClient(store)!
  const salvar = () => {
    if (!chave.trim()) return
    rodar(() => salvarChave(client(), f.id, chave, () => setChave('')), (r) => r.ok && setEditando(false))
  }
  const cancelar = () => { setChave(''); setEditando(false); setResultado(null) }
  const quantos = f.modelos?.length ?? 0

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
        {ocupado ? <Spinner /> : editando ? null : f.configurada ? (
          <HStack gap={12}>
            <button className="settings-quiet-button" onClick={() => rodar(() => testarChave(client(), f.id))}>{t('contas.apiDireta.testar')}</button>
            <button className="settings-quiet-button" onClick={() => { setEditando(true); setResultado(null) }}>{t('contas.apiDireta.trocar')}</button>
            <button className="settings-quiet-button" onClick={() => rodar(() => removerChave(client(), f.id))}>{t('contas.apiDireta.remover')}</button>
          </HStack>
        ) : (
          <button className="settings-quiet-button" onClick={() => { setEditando(true); setResultado(null) }}>{t('contas.apiDireta.adicionar')}</button>
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

const ESTADOS = {
  conta_reconhecida: 'contas.apiDireta.estado.contaReconhecida',
  modelos_no_catalogo: 'contas.apiDireta.estado.modelosNoCatalogo',
  acesso_nao_confirmado: 'contas.apiDireta.estado.acessoNaoConfirmado',
  nao_verificado: 'contas.apiDireta.estado.naoVerificado',
  recusada: 'contas.apiDireta.estado.recusada',
} as const

/** O resultado do «Testar acesso»: o estado, a frase do detalhe e, quando há lista de modelos, o aviso de que ela não prova acesso. */
function ResultadoDoTeste({ resultado }: { resultado: Acesso | { ok: false; motivo: string } }) {
  const t = useT()
  const estado = 'motivo' in resultado ? 'nao_verificado' : estadoDoTeste(resultado)
  let detalhe: string
  if (resultado.ok) {
    const { nomes, restantes } = amostraDeModelos(resultado.modelos)
    detalhe = resultado.modelos.length === 0
      ? t('contas.apiDireta.okSemModelos')
      : restantes > 0 ? t('contas.apiDireta.okComMais', { nomes, n: restantes }) : t('contas.apiDireta.ok', { nomes })
  } else {
    detalhe = 'motivo' in resultado ? t('contas.apiDireta.erro.desconhecido', { motivo: resultado.motivo }) : (t as unknown as (chave: ChaveTexto) => string)(chaveDoErro(resultado.erro))
  }
  const positivo = estado === 'conta_reconhecida' || estado === 'modelos_no_catalogo'
  return (
    <VStack gap={2} role="status" data-estado={estado} className={`settings-key-result ${positivo ? '' : 'failed'}`}>
      <HStack as="span" gap={5}>
        <Icon name={positivo ? 'checkmark.circle.fill' : 'exclamationmark.circle.fill'} size={10} />
        <Text size="caption" className="selectable">{t(ESTADOS[estado])}</Text>
      </HStack>
      <Text size="caption" tone="secondary" className="selectable">{detalhe}</Text>
      {estado === 'modelos_no_catalogo' ? <Text size="caption" tone="tertiary">{t('contas.apiDireta.catalogoNaoProva')}</Text> : null}
    </VStack>
  )
}
