// Bot de fundo (desenho v2, 16/11): o campo "Conta e modelo" de uma função. Fechado, é um campo de formulário com o motor, o modelo e a
// "Conta: …"; aberto, é o seletor do chat (`SeletorModelo`, modo `funcaoDeFundo`), preso ao campo como o chip do compositor.
// Os dados vêm de `seletor.ts`; aqui só o desenho, a posição e o fechar.
import { useEffect, useRef, useState, type CSSProperties } from 'react'
import { AgentIcon } from '../../../components/AgentIcon'
import { Icon } from '../../../components/Icon'
import { useT } from '../../../lib/fork-i18n/estado'
import type { BotStore } from '../../../store/bot-store'
import { settingsScreen } from '../ajustes/tela'
import { carregarModelos, esquecerFalha, esquecerFalhas, provedoresDoHub, useCatalogoMudou } from '../seletor-modelo/catalogo'
import { posicaoLista } from '../seletor-modelo/posicao'
import { noBody, usePresaAoChip } from '../seletor-modelo/presa'
import type { Escolha } from '../seletor-modelo/regras'
import { SeletorModelo } from '../seletor-modelo/SeletorModelo'
import { resumoDaEscolha, seletorDaFuncao, type TextosSeletor } from './seletor'
import type { Draft, FunctionSnapshot } from './model'

export function EscolhaDaFuncao({ host, data, functionId, draft, desconectada, dirty, disabled, textos, onEscolher }: {
  host: BotStore
  data: FunctionSnapshot
  functionId: string
  draft: Draft
  /** A conta salva caiu: o campo abre sem nada escolhido. */
  desconectada: boolean
  dirty: boolean
  disabled: boolean
  textos: TextosSeletor
  onEscolher: (escolha: Escolha) => void
}) {
  const t = useT()
  useCatalogoMudou()
  const [aberto, setAberto] = useState(false)
  const botao = useRef<HTMLButtonElement>(null)
  const painel = useRef<HTMLDivElement>(null)
  const s = seletorDaFuncao({ data, functionId, draft, catalogo: provedoresDoHub(host), desconectada, textos, abrirContas: () => { setAberto(false); settingsScreen.show('ai-accounts', 'contas') } })
  const resumo = resumoDaEscolha(s, textos)
  // O nome do modelo no campo fechado vem do catálogo do motor da escolha: lido uma vez por computador e motor.
  useEffect(() => { if (s.valor.backend) carregarModelos(host, s.valor.backend) }, [host, host.client, host.principalEpoch, s.valor.backend]) // eslint-disable-line react-hooks/exhaustive-deps

  const fechar = () => { setAberto(false); botao.current?.focus() }
  // Outra conexão ou outro computador: o painel não sobrevive à época antiga.
  useEffect(() => setAberto(false), [host.client, host.principalEpoch])
  useEffect(() => { if (disabled) setAberto(false) }, [disabled])
  useEffect(() => {
    if (!aberto) return
    const fora = (e: MouseEvent) => {
      if (!painel.current?.contains(e.target as Node) && !botao.current?.contains(e.target as Node) && !document.querySelector('.dialog.shown')) fechar()
    }
    document.addEventListener('mousedown', fora)
    return () => document.removeEventListener('mousedown', fora)
  }, [aberto]) // eslint-disable-line react-hooks/exhaustive-deps

  const pos = usePresaAoChip(aberto, botao, (el) => { const r = el.getBoundingClientRect(); return posicaoLista({ top: r.top, bottom: r.bottom, left: r.left }, { largura: window.innerWidth, altura: window.innerHeight }) })
  const estilo = pos ? { ...('top' in pos ? { left: pos.left, top: pos.top } : { left: pos.left, bottom: pos.bottom }), '--seletor-largura': `${pos.width}px`, '--seletor-altura': `${pos.maxHeight}px` } as unknown as CSSProperties : undefined
  const escolher = (escolha: Escolha) => {
    onEscolher(escolha)
    // Trocar de conta ou de modelo encerra a escolha; só o esforço mantém o painel aberto.
    if (escolha.accountId !== s.valor.accountId || escolha.model !== s.valor.model) fechar()
  }
  return (
    <div className="background-escolha">
      <button ref={botao} type="button" className="background-gatilho" disabled={disabled} aria-haspopup="dialog" aria-expanded={aberto}
        onClick={() => { if (!aberto) esquecerFalhas(); setAberto(!aberto) }}>
        {resumo ? <AgentIcon registry={s.valor.backend} size={20} /> : null}
        <span className="background-gatilho-texto">
          {resumo ? <>
            <b>{resumo.modelo}{dirty ? <span className="background-novo">{t('fundo.seletor.novaEscolha')}</span> : null}</b>
            <span><bdi dir="auto">{t('fundo.seletor.contaDe', { motor: resumo.motor, conta: resumo.conta })}</bdi></span>
          </> : <span className="background-gatilho-vazio">{t('fundo.seletor.escolha')}</span>}
        </span>
        <Icon name="chevron.down" size={11} weight="semibold" />
      </button>
      {estilo ? noBody(
        <div ref={painel} className="chip-modelo-painel" style={estilo}>
          <SeletorModelo modo="funcaoDeFundo" provedores={s.provedores} valor={s.valor} carregar={(b) => carregarModelos(host, b)} onEscolher={escolher} onFechar={fechar}
            contas={s.contas} funcao={s.funcao} tentarDeNovo={(b) => { esquecerFalha(host, b); carregarModelos(host, b) }} />
        </div>,
      ) : null}
    </div>
  )
}
