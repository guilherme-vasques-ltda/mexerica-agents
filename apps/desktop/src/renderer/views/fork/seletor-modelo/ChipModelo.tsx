// DON-907 C9: o chip do modelo no compositor. Mostra o modelo e o esforço da conversa; abre o seletor só quando o Hub
// anuncia a troca por conversa neste link e época (`sessions.can(chat, 'engine')`, D372/UI01b) e a sessão traz o
// `engine` com revisão. Sem isso, é só leitura. A troca grava por `sessionEngineSet`, com a confirmação D375; outro
// motor vai com a conta destino do cadastro do Hub (`donnaEngineAccounts`).
import { useEffect, useRef, useState, type CSSProperties } from 'react'
import type { Bot } from '@shared/models'
import { useT } from '../../../lib/fork-i18n/estado'
import { useModel, useSelector } from '../../../lib/observable'
import { useStoreRef } from '../../../store/context'
import { useOpenBotSettings } from '../bot-settings/bot-settings-entry'
import { engineSetup } from '../../../store/fork-engine-setup'
import type { HostClient } from '../../../client/host-client'
import { usePref } from '../../../lib/prefs'
import { seletorPrefs } from './preferencias'
import { semDesinstalados } from './desinstalados'
import { carregarModelos, esquecerFalha, esquecerFalhas, provedoresDoHub, useCatalogoMudou } from './catalogo'
import { corpoTroca, engineDaSessao, erroDaTroca, LeituraContas, precisaConfirmar, rotuloChip, type ContaMotor, type Escolha } from './regras'
import { modeloDe } from '../contexto/BarraContexto'
import { ConfirmarTroca } from './ConfirmarTroca'
import { usoDaConversa } from './uso'
import { SeletorModelo } from './SeletorModelo'
import { ancoraNoCampo, posicaoLista } from './posicao'
import { noBody, usePresaAoChip } from './presa'
import { ChipModeloOpen } from './fork-open-chip'

export function ChipModelo({ bot, sessionId }: { bot: Bot; sessionId: string | null }) {
  return window.codync.appScheme === 'mexerica-open' ? <ChipModeloOpen bot={bot} /> : <ChipModeloSessao bot={bot} sessionId={sessionId} />
}

function ChipModeloSessao({ bot, sessionId }: { bot: Bot; sessionId: string | null }) {
  const store = useStoreRef()
  // Re-render só quando muda o que o chip usa: conexão, época e computador (não a cada evento do Hub).
  useSelector(store, (s) => s.client)
  useSelector(store, (s) => s.principalEpoch)
  useSelector(store, (s) => s.computer.id)
  const t = useT()
  const sessions = useModel(store.sessions)
  const openSettings = useOpenBotSettings()
  useCatalogoMudou()
  const [favoritos, setFavoritos] = usePref(seletorPrefs.favoritos)
  const backends = store.hello?.backends
  useEffect(() => {
    if (!backends?.length) return
    const resto = semDesinstalados(favoritos, backends)
    if (resto !== favoritos) setFavoritos(resto)
  }, [backends, favoritos, setFavoritos])
  const [aberto, setAberto] = useState(false)
  const [confirmar, setConfirmar] = useState<Escolha | null>(null)
  const [erro, setErro] = useState<ReturnType<typeof erroDaTroca> | null>(null)
  const botao = useRef<HTMLButtonElement>(null)
  const painel = useRef<HTMLDivElement>(null)

  const session = sessions.of(bot.id)?.find((s) => (sessionId ? s.id === sessionId : s.principal)) ?? null
  const engine = engineDaSessao(session)
  const valor: Escolha = engine ?? { accountId: null, backend: bot.backend, model: bot.model ?? null, modelOptions: bot.modelOptions, fastMode: bot.fastMode }
  // R9: a capacidade é a do catálogo de sessões (anúncio do link e da época atuais), sem uma segunda fonte.
  const editavel = !!engine && sessions.can(bot.id, 'engine')
  const provedores = provedoresDoHub(store)
  const rotulo = rotuloChip(valor, provedores)
  const modelo = rotulo.modelo ?? t('conversa.seletor.chip.padraoDe', { motor: rotulo.motor })
  const nome = rotulo.esforco ? t('conversa.seletor.chip.nomeEsforco', { modelo, esforco: rotulo.esforco }) : t('conversa.seletor.chip.nome', { modelo })

  // O nome e o esforço do chip fechado vêm do catálogo do motor atual: lido uma vez por computador e motor.
  // Outra conexão ou reconexão (o `hello` muda a época): uma falha anterior deste motor é tentada de novo, sem reler
  // o que deu certo (R2-1, R3-1).
  useEffect(() => { esquecerFalha(store, valor.backend); carregarModelos(store, valor.backend) }, [store, store.client, store.principalEpoch, store.computer.id, valor.backend])

  // Perdeu a capacidade, ou outro link/época: o painel e a confirmação pendente fecham; nada da época velha continua.
  useEffect(() => { setAberto(false); setConfirmar(null); setErro(null) }, [store.client, store.principalEpoch])
  useEffect(() => { if (!editavel) { setAberto(false); setConfirmar(null) } }, [editavel])

  // R8: o cadastro de contas do Hub, lido ao abrir e preso ao link/época; só a lista fica guardada. Uma falha
  // mostra o próprio aviso e bloqueia a troca entre motores (sem chute); fechar e reabrir o seletor tenta de novo.
  const [, setLidas] = useState(0)
  const [leitura] = useState(() => new LeituraContas<HostClient>(
    () => ({ client: store.client, epoch: store.principalEpoch }),
    (client) => engineSetup.accounts(client).then((r) => r.accounts as ContaMotor[]),
    () => setLidas((n) => n + 1),
  ))
  useEffect(() => { if (aberto) leitura.abrir(); else leitura.fechar() }, [aberto, store.client, store.principalEpoch]) // eslint-disable-line react-hooks/exhaustive-deps
  const contasAtuais = leitura.estado()
  // R5: com a conta da conversa na lista, o aviso diz de qual conta é a falha (sem ela, o texto de sempre).
  const contaLabel = Array.isArray(contasAtuais) && valor.accountId ? contasAtuais.find((c) => c.id === valor.accountId)?.label ?? null : null

  const fechar = () => { setAberto(false); setErro(null); botao.current?.focus() }
  useEffect(() => {
    if (!aberto) return
    const fora = (e: MouseEvent) => {
      if (!painel.current?.contains(e.target as Node) && !botao.current?.contains(e.target as Node) && !document.querySelector('.dialog.shown')) fechar()
    }
    document.addEventListener('mousedown', fora)
    return () => document.removeEventListener('mousedown', fora)
  }, [aberto]) // eslint-disable-line react-hooks/exhaustive-deps

  const gravar = async (escolha: Escolha) => {
    // Revalida antes do pedido: capacidade e sessão do link/época atuais; resposta de outra época é ignorada.
    const client = store.client
    const epoch = store.principalEpoch
    if (!engine || !store.sessions.can(bot.id, 'engine')) return
    setErro(null)
    try {
      if (!client) throw new Error('offline')
      await client.call('sessionEngineSet', corpoTroca(bot.id, sessionId, engine.revision, escolha))
    } catch (error) {
      if (store.client === client && store.principalEpoch === epoch) setErro(erroDaTroca(error))
    }
  }
  const escolher = (escolha: Escolha) => {
    if (precisaConfirmar(valor, escolha, usoDaConversa(modeloDe(store), bot.id, sessionId))) setConfirmar(escolha)
    else void gravar(escolha)
  }

  // D473: a lista vai para o <body> (noBody) e abre presa ao chip, acima da caixa inteira do campo, dentro da janela.
  // R1 (#548): medida depois do layout e refeita enquanto aberta (barra lateral, janela, scroll), em presa.ts.
  const pos = usePresaAoChip(aberto, botao, (el) => posicaoLista(ancoraNoCampo(el), { largura: window.innerWidth, altura: window.innerHeight }))
  const ancora: CSSProperties | null = pos ? ('top' in pos ? { left: pos.left, top: pos.top } : { left: pos.left, bottom: pos.bottom }) : null
  const estilo = pos && ancora ? { ...ancora, '--seletor-largura': `${pos.width}px`, '--seletor-altura': `${pos.maxHeight}px` } as CSSProperties : undefined
  return (
    <>
      <button ref={botao} className="press chip-modelo" aria-label={editavel ? t('conversa.seletor.chip.trocar', { nome }) : nome} title={editavel ? t('conversa.seletor.chip.dica') : nome}
        aria-haspopup={editavel ? 'dialog' : undefined} aria-expanded={editavel ? aberto : undefined} aria-disabled={!editavel || undefined}
        onClick={() => { if (!editavel) return; if (!aberto) esquecerFalhas(); setAberto((a) => !a) }}>
        <span className="chip-modelo-nome">{modelo}</span>
        {rotulo.esforco ? <span className="chip-modelo-esforco">· {rotulo.esforco}</span> : null}
      </button>
      {estilo ? noBody(
        <div ref={painel} className="chip-modelo-painel" style={estilo}>
          <SeletorModelo modo="conversa" provedores={provedores} valor={valor} carregar={(b) => carregarModelos(store, b)} onEscolher={escolher} onFechar={fechar} contas={contasAtuais} tentarDeNovo={(b) => { esquecerFalha(store, b); carregarModelos(store, b) }}
            acesso={openSettings ? { nivel: bot.permission, mudar: () => { fechar(); openSettings(store, bot.id, 'behaviour') } } : null} aviso={erro ? (erro === 'conversa.seletor.erro.indisponivel' && contaLabel ? t('conversa.seletor.erro.indisponivelConta', { conta: contaLabel }) : t(erro)) : null} />
        </div>,
      ) : null}
      <ConfirmarTroca aberto={confirmar !== null} onCancelar={() => setConfirmar(null)}
        onTrocar={() => { const e = confirmar; setConfirmar(null); if (e) void gravar(e) }} />
    </>
  )
}
