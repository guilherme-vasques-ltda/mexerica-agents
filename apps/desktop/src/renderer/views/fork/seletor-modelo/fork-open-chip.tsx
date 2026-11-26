// Na Open, o chip do modelo embaixo do compositor lista os modelos que as contas conectadas comprovaram e troca só
// este bot por `donnaOpenModelSet`. Sem outra opção para escolher, é só leitura (sem botão que não abre).
import { useEffect, useRef, useState, type CSSProperties } from 'react'
import type { Bot } from '@shared/models'
import { useT } from '../../../lib/fork-i18n/estado'
import { useSelector } from '../../../lib/observable'
import { useStoreRef } from '../../../store/context'
import { engineSetup } from '../../../store/fork-engine-setup'
import type { ManagedAccount } from '../../../../shared/fork-engine-accounts'
import { clientePessoal } from '../contas-ia/fork-open-cliente'
import { aoMudarContaOpen, aplicarModeloOpen, contasEscolhiveis, modelosDaConta, padraoDaConta } from '../contas-ia/fork-open-modelo'
import { ancoraNoCampo, posicaoLista } from './posicao'
import { noBody, usePresaAoChip } from './presa'

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
    const ler = () => { void engineSetup.accounts(client).then((r) => { if (vivo) setContas(contasEscolhiveis(r.accounts)) }, () => { if (vivo) setContas([]) }) }
    ler()
    const parar = aoMudarContaOpen(ler)
    return () => { vivo = false; parar() }
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

  const escolher = async (accountId: string, model: string) => {
    const client = store.client
    if (!client) { setErro(true); return }
    setGravando(true)
    setErro(false)
    try {
      await aplicarModeloOpen(client, { accountId, model, botId: bot.id })
      fechar()
    } catch {
      setErro(true)
    } finally {
      setGravando(false)
    }
  }

  const pos = usePresaAoChip(aberto, botao, (el) => posicaoLista(ancoraNoCampo(el), { largura: window.innerWidth, altura: window.innerHeight }))
  const estilo: CSSProperties | undefined = pos ? ('top' in pos ? { left: pos.left, top: pos.top, maxHeight: pos.maxHeight } : { left: pos.left, bottom: pos.bottom, maxHeight: pos.maxHeight }) : undefined
  if (!editavel) return <span className="chip-modelo" title={modelo}><span className="chip-modelo-nome">{modelo}</span></span>
  return (
    <>
      <button ref={botao} className="press chip-modelo" aria-label={t('contas.openModelo.chipTrocar', { modelo })} title={t('contas.openModelo.chipTrocar', { modelo })}
        aria-haspopup="listbox" aria-expanded={aberto} onClick={() => { setErro(false); setAberto((a) => !a) }}>
        <span className="chip-modelo-nome">{modelo}</span>
      </button>
      {aberto && estilo ? noBody(
        <div ref={painel} className="chip-modelo-painel chip-modelo-open" style={estilo} role="listbox" aria-label={t('contas.openModelo.chipLista')} aria-busy={gravando || undefined}>
          {contas.map((conta) => (
            <div key={conta.id} className="chip-modelo-open-grupo" role="group" aria-label={conta.label}>
              {contas.length > 1 ? <div className="chip-modelo-open-conta">{conta.label}</div> : null}
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
          {erro ? <div className="chip-modelo-open-erro" role="alert">{t('contas.openModelo.chipErro')}</div> : null}
        </div>,
      ) : null}
    </>
  )
}
