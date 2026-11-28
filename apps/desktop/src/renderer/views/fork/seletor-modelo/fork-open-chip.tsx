// Na Open, o chip do modelo embaixo do compositor lista os modelos que as contas conectadas comprovaram e troca só
// este bot por `donnaOpenModelSet`. Sem outra opção para escolher, é só leitura (sem botão que não abre).
import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent as TeclaReact } from 'react'
import type { Bot } from '@shared/models'
import { useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { useSelector } from '../../../lib/observable'
import { useStoreRef } from '../../../store/context'
import { engineSetup } from '../../../store/fork-engine-setup'
import type { ManagedAccount } from '../../../../shared/fork-engine-accounts'
import { clientePessoal } from '../contas-ia/fork-open-cliente'
import { aoMudarContaOpen, aplicarModeloOpen, contasEscolhiveis, modelosDaConta, opcoesDoModelo, padraoDaConta, valorDaOpcao } from '../contas-ia/fork-open-modelo'
import { ancoraNoCampo, posicaoLista } from './posicao'
import { noBody, usePresaAoChip } from './presa'

const ROTULO_ESFORCO: Record<string, (t: Tradutor) => string> = {
  none: (t) => t('contas.openModelo.esforco.none'),
  minimal: (t) => t('contas.openModelo.esforco.minimal'),
  low: (t) => t('contas.openModelo.esforco.low'),
  medium: (t) => t('contas.openModelo.esforco.medium'),
  high: (t) => t('contas.openModelo.esforco.high'),
  xhigh: (t) => t('contas.openModelo.esforco.xhigh'),
  max: (t) => t('contas.openModelo.esforco.max'),
  ultra: (t) => t('contas.openModelo.esforco.ultra'),
}
const ROTULO_VELOCIDADE: Record<string, (t: Tradutor) => string> = {
  fast: (t) => t('contas.openModelo.velocidadeRapida'),
}

export function ChipModeloOpen({ bot }: { bot: Bot }) {
  const store = useStoreRef()
  useSelector(store, (s) => s.client)
  const t = useT()
  const [contas, setContas] = useState<ManagedAccount[]>([])
  const [aberto, setAberto] = useState(false)
  const [gravando, setGravando] = useState(false)
  const [erro, setErro] = useState(false)
  const botao = useRef<HTMLButtonElement>(null)
  const painel = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const client = store.client ? clientePessoal(store.client) : null
    if (!client) { setContas([]); return }
    let vivo = true
    let repetir: ReturnType<typeof setTimeout> | undefined
    const ler = () => {
      clearTimeout(repetir)
      void engineSetup.accounts(client).then((r) => { if (vivo) setContas(contasEscolhiveis(r.accounts)) }, () => { if (vivo) repetir = setTimeout(ler, 5000) })
    }
    ler()
    const parar = aoMudarContaOpen(ler)
    return () => { vivo = false; clearTimeout(repetir); parar() }
  }, [store, store.client])

  const contaId = bot.forkEditor?.engineAccountId ?? null
  const opcoes = contas.flatMap((conta) => modelosDaConta(conta).map((model) => ({ conta, model })))
  const atual = opcoes.find((o) => o.conta.id === contaId && o.model === bot.model) ?? null
  const modelo = bot.model ?? t('contas.openModelo.chipPadrao')
  const editavel = opcoes.some((o) => o !== atual)

  useEffect(() => { if (!editavel) setAberto(false) }, [editavel])
  const fechar = () => { setAberto(false); setErro(false); botao.current?.focus() }
  useEffect(() => {
    if (!aberto) return
    const fora = (e: MouseEvent) => { if (!painel.current?.contains(e.target as Node) && !botao.current?.contains(e.target as Node)) fechar() }
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') fechar() }
    document.addEventListener('mousedown', fora)
    document.addEventListener('keydown', esc)
    return () => { document.removeEventListener('mousedown', fora); document.removeEventListener('keydown', esc) }
  }, [aberto]) // eslint-disable-line react-hooks/exhaustive-deps

  const navegar = (e: TeclaReact<HTMLDivElement>) => {
    const passo = ({ ArrowDown: 1, ArrowUp: -1 } as Record<string, number>)[e.key]
    const ponta = ({ Home: 0, End: -1 } as Record<string, number>)[e.key]
    if (passo === undefined && ponta === undefined) return
    const lista = Array.from(painel.current?.querySelectorAll<HTMLButtonElement>('[role="option"]:not(:disabled)') ?? [])
    if (!lista.length) return
    e.preventDefault()
    const agora = lista.indexOf(document.activeElement as HTMLButtonElement)
    const alvo = passo !== undefined ? (agora + passo + lista.length) % lista.length : (ponta! + lista.length) % lista.length
    lista[alvo]?.focus()
  }

  const opcoesAtuais = opcoesDoModelo(atual?.conta, atual?.model)
  const escolher = async (accountId: string, model: string, options?: Record<string, string>) => {
    const client = store.client
    if (!client) { setErro(true); return }
    setGravando(true)
    setErro(false)
    try {
      await aplicarModeloOpen(client, { accountId, model, botId: bot.id, ...(options ? { options } : {}) })
      fechar()
    } catch {
      setErro(true)
    } finally {
      setGravando(false)
    }
  }

  const pos = usePresaAoChip(aberto, botao, (el) => posicaoLista(ancoraNoCampo(el), { largura: window.innerWidth, altura: window.innerHeight }))
  const estilo: CSSProperties | undefined = pos ? ('top' in pos ? { left: pos.left, top: pos.top, maxHeight: pos.maxHeight } : { left: pos.left, bottom: pos.bottom, maxHeight: pos.maxHeight }) : undefined
  const posicionado = estilo !== undefined
  useEffect(() => {
    if (!aberto || !posicionado) return
    const opcao = painel.current?.querySelector<HTMLButtonElement>('[role="option"][aria-selected="true"]') ?? painel.current?.querySelector<HTMLButtonElement>('[role="option"]')
    opcao?.focus()
  }, [aberto, posicionado])
  if (!editavel) return <span className="chip-modelo" title={modelo}><span className="chip-modelo-nome">{modelo}</span></span>
  return (
    <>
      <button ref={botao} className="press chip-modelo" aria-label={t('contas.openModelo.chipTrocar', { modelo })} title={t('contas.openModelo.chipTrocar', { modelo })}
        aria-haspopup="listbox" aria-expanded={aberto} onClick={() => { setErro(false); setAberto((a) => !a) }}>
        <span className="chip-modelo-nome">{modelo}</span>
      </button>
      {aberto && estilo ? noBody(
        <div ref={painel} className="chip-modelo-painel chip-modelo-open" style={estilo} role="listbox" aria-label={t('contas.openModelo.chipLista')} aria-busy={gravando || undefined} onKeyDown={navegar}>
          {contas.map((conta) => (
            <div key={conta.id} className="chip-modelo-open-grupo" role="group" aria-label={conta.label}>
              <div className="chip-modelo-open-conta">{contas.length > 1 ? `${conta.brand} · ${conta.label}` : conta.brand}</div>
              {modelosDaConta(conta).map((model) => {
                const marcado = conta.id === contaId && model === bot.model
                const nomeModelo = model === padraoDaConta(conta) ? t('contas.openModelo.padrao', { modelo: model }) : model
                return (
                  <button key={model} type="button" role="option" aria-selected={marcado} className="press chip-modelo-open-opcao" disabled={gravando}
                    onClick={() => { if (!marcado) void escolher(conta.id, model) }}>
                    <span dir="ltr">{nomeModelo}</span>
                  </button>
                )
              })}
            </div>
          ))}
          {atual && opcoesAtuais.map((opcao) => {
            const atualValor = valorDaOpcao(opcao, bot.modelOptions)
            const rotulo = (valor: string) => (opcao.id === 'reasoning_effort' ? ROTULO_ESFORCO[valor] : ROTULO_VELOCIDADE[valor])?.(t) ?? valor
            const gravar = (valor: string | null) => {
              const { [opcao.id]: _antes, ...resto } = bot.modelOptions ?? {}
              void escolher(atual.conta.id, atual.model, valor === null ? resto : { ...resto, [opcao.id]: valor })
            }
            return (
              <div key={opcao.id} className="chip-modelo-open-grupo" role="group" aria-label={opcao.id === 'reasoning_effort' ? t('contas.openModelo.raciocinio') : t('contas.openModelo.velocidade')}>
                <div className="chip-modelo-open-conta">{opcao.id === 'reasoning_effort' ? t('contas.openModelo.raciocinio') : t('contas.openModelo.velocidade')}</div>
                {opcao.default === null ? (
                  <button type="button" className="press chip-modelo-open-opcao" aria-pressed={atualValor === null} disabled={gravando} onClick={() => { if (atualValor !== null) gravar(null) }}>{t('contas.openModelo.padraoDoMotor')}</button>
                ) : null}
                {opcao.values.map((valor) => (
                  <button key={valor} type="button" className="press chip-modelo-open-opcao" aria-pressed={atualValor === valor} disabled={gravando} onClick={() => {
                    if (valor === opcao.default) { if (bot.modelOptions?.[opcao.id] !== undefined) gravar(null) } else if (atualValor !== valor) gravar(valor)
                  }}>
                    <span dir="ltr">{valor === opcao.default ? t('contas.openModelo.padrao', { modelo: rotulo(valor) }) : rotulo(valor)}</span>
                  </button>
                ))}
              </div>
            )
          })}
          {erro ? <div className="chip-modelo-open-erro" role="alert">{t('contas.openModelo.chipErro')}</div> : null}
        </div>,
      ) : null}
    </>
  )
}
