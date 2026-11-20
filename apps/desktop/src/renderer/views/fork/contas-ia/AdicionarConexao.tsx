import { useEffect, useState } from 'react'
import type { Engine } from '../../../../shared/fork-engine-accounts.ts'
import { Button, CardForm, CardSection, Spinner } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { AgentTile } from '../../marketplace/AgentSheet'
import { Field, MarketBadge } from '../../marketplace/pieces'
import { liveClient } from '../../settings/parts'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { LinhaDoFornecedor } from './ApiDireta'
import { aoMudarChaves, listarChaves, type FornecedorDaChave } from './api-direta'
import { agrupar, buscar, chavesDaTela, chavesDisponiveis, ehOpen, planosDaTela } from './conexoes'
import type { ContasStore } from './contas-store'
import { engineCompany, engineName, engineRegistry } from './regras'
import { useTextosContas } from './use-textos'
import './contas-ia.css'

/**
 * Configurações → Contas de IA → Adicionar: uma tela só, com busca e os dois grupos, Planos (conta ligada pela tela,
 * sem terminal) e Chaves de API. Só mostra o que já funciona; na edição Open, só o Codex.
 */
export function AdicionarConexao({ contas, onPlano, onClose }: { contas: ContasStore; onPlano: (engine: Engine) => void; onClose: () => void }) {
  const store = useModel(useStore())
  useModel(contas)
  const t = useT()
  const idioma = useIdioma()
  const textos = useTextosContas()
  const online = store.connection.kind === 'online' && !store.mismatch
  const contexto = { open: ehOpen(window.codync), isLoopback: store.isLoopback, backends: store.hello?.backends ?? [] }
  const comChaves = chavesDisponiveis(contexto)
  const [consulta, setConsulta] = useState('')
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
    if (comChaves && online) void reler()
  }, [comChaves, online, store.hello?.hostId])
  useEffect(() => (comChaves && online ? aoMudarChaves(() => void reler()) : undefined), [comChaves, online, store])

  const itens = [...planosDaTela(contexto.open, engineName), ...chavesDaTela(contexto, fornecedores ?? [])]
  const grupos = agrupar(buscar(itens, consulta))
  const conectadas = (engine: Engine) => (contas.accounts ?? []).some((a) => a.backend === engine && a.state === 'connected')

  return (
    <SheetPage><div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <ModalHeader icon="person.crop.circle.badge.plus" title={t('contas.conexoes.titulo')} subtitle={t('contas.conexoes.sub', { hub: store.hostName })} closeTitle={textos.fechar} onClose={onClose} />
      <div className="contas-panel-body">
        <CardForm>
          <CardSection>
            <Field value={consulta} onChange={setConsulta} label={t('contas.conexoes.buscar')} placeholder={t('contas.conexoes.buscar')} />
          </CardSection>
          {grupos.map(({ grupo, itens: doGrupo }) => (
            <CardSection key={grupo} title={t(grupo === 'planos' ? 'contas.conexoes.planos' : 'contas.conexoes.chaves')} footer={t(grupo === 'planos' ? 'contas.conexoes.planosRodape' : 'contas.conexoes.chavesRodape')}>
              {doGrupo.map((item) => {
                const fornecedor = item.fornecedor ? fornecedores?.find((f) => f.id === item.fornecedor) : undefined
                if (fornecedor) return <LinhaDoFornecedor key={item.id} fornecedor={fornecedor} />
                if (!item.engine) return null
                const engine = item.engine
                return (
                  <div key={item.id} className="settings-row">
                    <AgentTile registry={engineRegistry(engine)} icon={16} tile={28} radius={8} />
                    <VStack grow className="settings-stack">
                      <HStack gap={6}>
                        <Text tone="text" dir="auto">{item.nome}</Text>
                        {conectadas(engine) ? <MarketBadge label={textos.conectada} tone="added" icon="checkmark" /> : null}
                      </HStack>
                      <Text size="caption" tone="tertiary">{engineCompany(engine)}</Text>
                    </VStack>
                    <Button onClick={() => onPlano(engine)}>{t('contas.conexoes.conectar')}</Button>
                  </div>
                )
              })}
            </CardSection>
          ))}
          {comChaves && online && !fornecedores && !consulta.trim() ? (
            <CardSection title={t('contas.conexoes.chaves')}>
              {falhou ? <Text size="caption" tone="secondary" role="status">{t('contas.apiDireta.erroLista')}</Text> : <Spinner size={12} />}
            </CardSection>
          ) : null}
          {consulta.trim() && grupos.length === 0 ? <Text tone="secondary" role="status">{t('contas.conexoes.nada', { busca: consulta.trim() })}</Text> : null}
        </CardForm>
      </div>
      <HStack gap={8} className="contas-footer">
        <Spacer />
        <Button kind="secondary" onClick={onClose}>{textos.fechar}</Button>
      </HStack>
    </div></SheetPage>
  )
}
