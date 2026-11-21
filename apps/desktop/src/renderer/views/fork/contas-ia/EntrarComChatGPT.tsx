import { useEffect, useRef, useState } from 'react'
import { CardSection, Spinner } from '../../../components/Controls'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { liveClient } from '../../settings/parts'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { isolado } from '../../../../shared/bidi.ts'
import { MarketBadge } from '../../marketplace/pieces'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { mostrarApiDireta } from './api-direta'
import { entrarComChatGPT, fraseDaFalha, FalhaChatGPT, lerEstadoChatGPT, sairDoChatGPT, type EstadoChatGPT } from './chatgpt-login'
import '../../settings/settings.css'

/**
 * Conexões de IA → Plano do ChatGPT: entrar com a conta do ChatGPT para os bots usarem o plano dela. O Hub guarda
 * os tokens cifrados no cofre deste computador e nunca os devolve; aqui só se mostra o estado e se pedem as
 * intenções (entrar, ativar o uso do plano, sair).
 */
export function EntrarComChatGPT() {
  const store = useModel(useStore())
  const t = useT()
  const idioma = useIdioma()
  const online = store.connection.kind === 'online' && !store.mismatch
  const visivel = mostrarApiDireta(store.hello?.backends ?? [], store.isLoopback)
  const [estado, setEstado] = useState<EstadoChatGPT | null>(null)
  const [falhou, setFalhou] = useState(false)
  const [erro, setErro] = useState<ChaveTexto | null>(null)
  const [avisoDeSair, setAvisoDeSair] = useState(false)
  const [avisoDeTroca, setAvisoDeTroca] = useState(false)
  const [ocupado, setOcupado] = useState(false)
  const [concluindo, setConcluindo] = useState(false)
  const [saindo, setSaindo] = useState(false)
  const ocupadoRef = useRef(false)
  const cancelarRef = useRef<(() => void) | null>(null)

  // Cada leitura tem a sua vez: uma resposta antiga não sobrescreve o estado de uma leitura mais nova.
  const geracao = useRef(0)
  const reler = async () => {
    const client = liveClient(store)
    if (!client) return
    const minha = ++geracao.current
    try {
      const lido = await lerEstadoChatGPT(client)
      if (minha !== geracao.current) return
      setEstado(lido)
      setFalhou(false)
    } catch {
      if (minha !== geracao.current) return
      setFalhou(true)
    }
  }
  useEffect(() => {
    if (visivel && online) void reler()
  }, [visivel, online, store.hello?.hostId])
  // Trocar de aba desiste do login em espera: sem o botão de Cancelar, o ouvinte não fica aberto.
  useEffect(() => () => cancelarRef.current?.(), [])

  const entrar = (consentir: boolean, novaConta = false) => {
    const client = liveClient(store)
    const space = store.spaceId ?? store.spaces?.personalSpaceId
    // A sessão é do dono, só usada pelos bots do Space pessoal: sem saber qual é o Pessoal, ou com outro Space
    // aberto, o login não começa.
    const pessoal = store.spaces?.personalSpaceId
    if (!client || !space || pessoal === undefined || space !== pessoal || ocupadoRef.current) return
    setErro(null)
    setAvisoDeSair(false)
    setAvisoDeTroca(false)
    const { fim, cancelar } = entrarComChatGPT(client, window.codync.app.loginRetorno, consentir, space, novaConta, undefined, () => setConcluindo(true))
    cancelarRef.current = cancelar
    ocupadoRef.current = true
    setOcupado(true)
    void fim
      .then((r) => {
        if (!r.ok) setErro('contas.chatgpt.erro.falha')
        else {
          setEstado({ conectado: true, rotulo: r.rotulo ?? undefined, planoHabilitado: r.planoHabilitado, precisaEntrar: false })
          // A conta anterior foi substituída sem o emissor confirmar o encerramento dela.
          if (r.avisoAnteriorSemConfirmacao) setAvisoDeTroca(true)
        }
      })
      .catch((e: unknown) => setErro(e instanceof FalhaChatGPT ? fraseDaFalha(e.tipo) : 'contas.chatgpt.erro.falha'))
      .finally(() => {
        ocupadoRef.current = false
        setOcupado(false)
        setConcluindo(false)
        cancelarRef.current = null
        void reler()
      })
  }

  const sair = () => {
    const client = liveClient(store)
    if (!client || ocupadoRef.current) return
    setErro(null)
    ocupadoRef.current = true
    setOcupado(true)
    setSaindo(true)
    void sairDoChatGPT(client)
      .then((r) => {
        setAvisoDeSair(r.revogado !== true)
        // A leitura de conferência vem a seguir; a tela já não mostra a conta antiga como conectada.
        setEstado({ conectado: false, rotulo: undefined, planoHabilitado: false, precisaEntrar: true })
      })
      .catch(() => setErro('contas.chatgpt.erro.sair'))
      .finally(() => {
        ocupadoRef.current = false
        setOcupado(false)
        setSaindo(false)
        void reler()
      })
  }

  // Fora do Space pessoal a sessão não começa: em vez de botões que nada fazem, o motivo na tela.
  const pessoal = store.spaces?.personalSpaceId
  const foraDoPessoal = pessoal === undefined || (store.spaceId ?? pessoal) !== pessoal

  if (!visivel) return null
  return (
    <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <CardSection title={t('contas.chatgpt.titulo')} footer={t('contas.chatgpt.rodape')}>
        {!online ? <Text tone="secondary">{t('contas.apiDireta.semConexao')}</Text> : null}
        {online && falhou && !estado ? (
          <HStack gap={12}>
            <Text size="caption" tone="secondary" role="status">{t('contas.chatgpt.erro.estado')}</Text>
            <button className="settings-quiet-button" onClick={() => void reler()}>{t('ajustes.atualizacoes.tentar')}</button>
          </HStack>
        ) : null}
        {online && !estado && !falhou ? <Spinner size={12} /> : null}
        {online ? (
          <PlanoDoChatGPT
            estado={estado}
            ocupado={ocupado}
            concluindo={concluindo}
            erro={erro}
            avisoDeSair={avisoDeSair}
            avisoDeTroca={avisoDeTroca}
            entrar={() => entrar(false)}
            ativar={() => entrar(true)}
            outraConta={() => entrar(false, true)}
            sair={sair}
            saindo={saindo}
            foraDoPessoal={foraDoPessoal}
            cancelar={() => cancelarRef.current?.()}
          />
        ) : null}
      </CardSection>
    </div>
  )
}

/** A parte que só desenha: o estado da conta, os avisos e os botões; quem decide é o componente de cima. */
export function PlanoDoChatGPT({ estado, ocupado, concluindo, erro, avisoDeSair, avisoDeTroca, entrar, ativar, outraConta, sair, saindo, foraDoPessoal, cancelar }: {
  estado: EstadoChatGPT | null
  ocupado: boolean
  concluindo?: boolean
  erro: ChaveTexto | null
  avisoDeSair: boolean
  avisoDeTroca: boolean
  entrar: () => void
  ativar: () => void
  outraConta?: () => void
  sair: () => void
  saindo?: boolean
  foraDoPessoal?: boolean
  cancelar: () => void
}) {
  const t = useT()
  const conectado = estado?.conectado === true
  return (
    <VStack gap={8}>
      {avisoDeSair ? <Text size="caption" role="status">{t('contas.chatgpt.saiuSemConfirmacao')}</Text> : null}
      {avisoDeTroca ? <Text size="caption" role="status">{t('contas.chatgpt.trocouSemConfirmacao')}</Text> : null}
      {erro ? (
        <HStack as="span" gap={5} role="status" className="settings-key-result failed">
          <Text size="caption" className="selectable">{(t as unknown as (chave: ChaveTexto) => string)(erro)}</Text>
        </HStack>
      ) : null}
      {ocupado && saindo ? <Spinner size={12} /> : ocupado ? (
        <HStack gap={12}>
          {concluindo ? (
            <Text size="caption" tone="secondary">{t('contas.chatgpt.concluindo')}</Text>
          ) : (
            <>
              <Text size="caption" tone="secondary">{t('contas.chatgpt.aguardando')}</Text>
              <button className="settings-quiet-button" onClick={cancelar}>{t('contas.chatgpt.cancelar')}</button>
            </>
          )}
        </HStack>
      ) : null}
      {foraDoPessoal && estado ? <Text size="caption" tone="secondary">{t('contas.chatgpt.somentePessoal')}</Text> : null}
      <div className="settings-row">
        <VStack grow className="settings-stack">
          {conectado ? (
            <>
              <HStack gap={6}>
                <Text tone="text" dir="auto">{isolado(estado?.rotulo ?? '')}</Text>
                {estado?.planoHabilitado ? <MarketBadge label={t('contas.chatgpt.planoHabilitado')} tone="added" icon="checkmark" /> : null}
              </HStack>
              {!estado?.planoHabilitado ? <Text size="caption" tone="secondary">{t('contas.chatgpt.planoNaoHabilitado')}</Text> : null}
            </>
          ) : estado?.precisaEntrar ? <Text size="caption" tone="secondary">{t('contas.chatgpt.precisaEntrar')}</Text> : null}
        </VStack>
        {ocupado ? null : conectado ? (
          // O «Sair» fica sempre ao alcance (o Hub não exige o Space pessoal para encerrar a sessão);
          // entrar, ativar e trocar de conta é que valem só com o Pessoal aberto.
          <HStack gap={12}>
            {!estado?.planoHabilitado && !foraDoPessoal ? <button className="settings-quiet-button" onClick={ativar}>{t('contas.chatgpt.ativar')}</button> : null}
            {!foraDoPessoal && outraConta ? <button className="settings-quiet-button" onClick={outraConta}>{t('contas.chatgpt.outraConta')}</button> : null}
            <button className="settings-quiet-button" onClick={sair}>{t('contas.chatgpt.sair')}</button>
          </HStack>
        ) : foraDoPessoal || !estado ? null : estado?.precisaEntrar ? (
          <button className="settings-quiet-button" onClick={entrar}>{t('contas.chatgpt.entrarDeNovo')}</button>
        ) : (
          <button className="settings-quiet-button" onClick={entrar}>{t('contas.chatgpt.entrar')}</button>
        )}
      </div>
    </VStack>
  )
}
