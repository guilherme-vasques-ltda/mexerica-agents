// MX-264 (opção A do dono): o botão "Ligar em todos os bots" de uma skill ou de um conector, com a prévia aberta no mesmo
// lugar e UM OK, e o interruptor "Novos bots já vêm com esta skill" (só skills). A view só desenha o modelo (em-todos.ts).
// Sistema visual: docs/reference/redesenho/components/ExtensoesEmTodos/README.md.
import { useEffect, useRef, useState } from 'react'
import { isolado } from '@shared/bidi'
import { Button, Spinner, Switch } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useEscape } from '../../../components/Overlay'
import { useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { mesmaOrigem, origemDe } from '../meus-mcp/estado'
import {
  agruparExcluidos, alterarNovosBots, interruptorInicial, lerNovosBots, proximoInterruptor, SessaoEmTodos, temInterruptorNovosBots,
  type Alvo, type ErroEmTodos, type TipoExtensao,
} from './em-todos'
import './em-todos.css'

const CHAVE_ERRO = {
  permissao: 'extensoes.emTodos.erro.permissao',
  semFuncao: 'extensoes.emTodos.erro.semFuncao',
  indisponivel: 'extensoes.emTodos.erro.indisponivel',
  incerto: 'extensoes.emTodos.erro.incerto',
  generico: 'extensoes.emTodos.erro.generico',
} as const satisfies Record<ErroEmTodos, string>

/**
 * A faixa de uma extensão instalada: o botão, e (só skill) o interruptor dos bots novos. A prévia abre embaixo, no mesmo
 * lugar. `novosBots` é o que o Hub disse na lista de skills; `aoMudarNovosBots` pede que quem lista releia.
 */
export function LigarEmTodos({ tipo, id, nome, novosBots = false, aoMudarNovosBots }: {
  tipo: TipoExtensao
  id: string
  /** O nome que a pessoa vê (dado, nunca traduzido). */
  nome: string
  novosBots?: boolean
  aoMudarNovosBots?: () => void
}) {
  const t = useT()
  const store = useStore()
  const [sessao, setSessao] = useState<SessaoEmTodos | null>(null)
  const botao = useRef<HTMLButtonElement>(null)
  const aberta = sessao !== null && sessao.estado.fase !== 'fechado'
  useModel(sessao)

  const abrir = () => {
    // O gesto é desta conexão (Hub, link e geração); resposta de outra não vale (D166/D175).
    const origem = origemDe(store)
    if (!origem) return
    const alvo: Alvo = { tipo, id, nome }
    const nova = new SessaoEmTodos(origem.client, alvo, () => mesmaOrigem(store, origem))
    setSessao(nova)
    nova.abrir()
  }
  /** O foco volta ao botão só depois do render que o reabilita (com a prévia aberta ele fica desligado). */
  const devolverFoco = useRef(false)
  const fechar = () => {
    sessao?.fechar()
    devolverFoco.current = true
  }
  useEffect(() => {
    if (aberta || !devolverFoco.current) return
    devolverFoco.current = false
    botao.current?.focus({ preventScroll: true })
  }, [aberta])

  return (
    <div className="eet" data-tipo={tipo}>
      <div className="eet-barra">
        <button ref={botao} className="eet-botao press" aria-expanded={aberta} aria-label={t('extensoes.emTodos.botaoNome', { nome: isolado(nome) })} disabled={aberta || !origemDe(store)} onClick={abrir}>
          <Icon name="checkmark.circle" size={12} />
          {t('extensoes.emTodos.botao')}
        </button>
        {temInterruptorNovosBots(tipo) ? <NovosBots id={id} nome={nome} valor={novosBots} aoMudar={aoMudarNovosBots} /> : null}
      </div>
      {sessao && aberta ? <Previa sessao={sessao} onFechar={fechar} /> : null}
    </div>
  )
}

/** A prévia no mesmo lugar: quem recebe, quem fica de fora com o motivo, e um OK só. */
function Previa({ sessao, onFechar }: { sessao: SessaoEmTodos; onFechar: () => void }) {
  const t = useT()
  const store = useStore()
  useModel(sessao)
  const { estado, alvo } = sessao
  const nome = isolado(alvo.nome)
  const painel = useRef<HTMLDivElement>(null)
  const fase = estado.fase
  // Esc fecha só a prévia: a âncora é o próprio bloco (ativo), não o botão que fica desligado enquanto ele está aberto.
  // Enquanto aplica, Esc não faz nada (o pedido já foi ao Hub).
  useEscape(true, () => { if (sessao.estado.fase !== 'aplicando') onFechar() }, painel)
  // Hub, link ou geração mudou: a prévia é velha e o bloco fecha antes de qualquer escrita.
  useModel(store)
  useEffect(() => { sessao.reconferir() })
  // O foco entra no OK quando a prévia chega (e no bloco enquanto lê); sem OK (ninguém recebe ou erro), no Fechar.
  useEffect(() => {
    const primeiro = painel.current?.querySelector<HTMLElement>('.eet-acoes button:not(:disabled)')
    // O bloco abre embaixo da linha: se passa da borda da página, a página rola até ele inteiro (o OK fica à vista).
    if (fase === 'previa' || fase === 'feito' || fase === 'erro') {
      ;(primeiro ?? painel.current)?.focus({ preventScroll: true })
      painel.current?.scrollIntoView({ block: 'nearest' })
    } else if (fase === 'lendo') {
      painel.current?.focus({ preventScroll: true })
      painel.current?.scrollIntoView({ block: 'nearest' })
    }
  }, [fase])

  const nomeDe = (botId: string) => store.bots.get(botId)?.name ?? null
  return (
    <div ref={painel} tabIndex={-1} className="eet-painel" role="group" aria-label={t('extensoes.emTodos.botaoNome', { nome })} aria-busy={fase === 'lendo' || fase === 'aplicando'}>
      {fase === 'lendo' ? (
        <>
          {estado.mudou ? <p className="eet-aviso" role="status">{t('extensoes.emTodos.mudou')}</p> : null}
          <p className="eet-estado" role="status"><Spinner />{t('extensoes.emTodos.lendo')}</p>
        </>
      ) : fase === 'previa' || fase === 'aplicando' ? (() => {
        const previa = estado.previa
        const aplicando = fase === 'aplicando'
        const grupos = agruparExcluidos(previa.excluidos, nomeDe)
        const quem = previa.recebedores.length
        return (
          <>
            {fase === 'previa' && estado.mudou ? <p className="eet-aviso" role="status">{t('extensoes.emTodos.mudou')}</p> : null}
            <h4 className="eet-titulo" role="status">{quem > 0 ? t.plural('extensoes.emTodos.recebem', quem, { nome }) : t('extensoes.emTodos.nenhum', { nome })}</h4>
            {quem > 0 ? (
              <ul className="eet-lista" aria-label={t.plural('extensoes.emTodos.recebem', quem, { nome })}>
                {previa.recebedores.map((r) => <li key={r.botId} className="eet-linha"><bdi dir="auto">{r.nome}</bdi></li>)}
              </ul>
            ) : null}
            {grupos.length > 0 ? (
              <div className="eet-fora">
                <h4 className="eet-titulo eet-titulo-fora">{t('extensoes.emTodos.fora', { n: previa.excluidos.length })}</h4>
                <dl className="eet-motivos">
                  {grupos.map((g) => (
                    <div key={g.motivo} className="eet-motivo">
                      <dt>{t(`extensoes.emTodos.motivo.${g.motivo}`)}</dt>
                      <dd>
                        {g.nomes.map((n, i) => <span key={i}>{i > 0 ? ', ' : ''}<bdi dir="auto">{n}</bdi></span>)}
                        {g.semNome > 0 ? <span>{g.nomes.length > 0 ? ', ' : ''}{t.plural('extensoes.emTodos.semNome', g.semNome)}</span> : null}
                      </dd>
                    </div>
                  ))}
                </dl>
              </div>
            ) : null}
            <div className="eet-acoes">
              {quem > 0 ? (
                <Button className="eet-ok" disabled={aplicando} onClick={() => void sessao.confirmar()}>
                  {aplicando ? <><Spinner />{t('extensoes.emTodos.ligando')}</> : t.plural('extensoes.emTodos.ligar', quem)}
                </Button>
              ) : null}
              <Button kind="secondary" disabled={aplicando} onClick={onFechar}>{quem > 0 ? t('extensoes.emTodos.cancelar') : t('extensoes.emTodos.fechar')}</Button>
            </div>
          </>
        )
      })() : fase === 'feito' ? (
        <>
          <p className="eet-feito" role="status"><Icon name="checkmark" size={12} weight="bold" />{t.plural('extensoes.emTodos.feito', estado.quantos)}</p>
          <div className="eet-acoes"><Button kind="secondary" onClick={onFechar}>{t('extensoes.emTodos.fechar')}</Button></div>
        </>
      ) : fase === 'erro' ? (
        <>
          <p className="eet-erro" role="alert">{t(CHAVE_ERRO[estado.erro])}</p>
          <div className="eet-acoes">
            {estado.erro === 'generico' ? <Button onClick={() => sessao.abrir()}>{t('extensoes.emTodos.tentar')}</Button> : null}
            <Button kind="secondary" onClick={onFechar}>{t('extensoes.emTodos.fechar')}</Button>
          </div>
        </>
      ) : null}
    </div>
  )
}

/** "Novos bots já vêm com esta skill": só skills (D91 mantida: conector e conta nascem desligados). Efeito imediato. */
function NovosBots({ id, nome, valor, aoMudar }: { id: string; nome: string; valor: boolean; aoMudar?: () => void }) {
  const t = useT()
  const store = useStore()
  // A transição do valor mostrado é pura (em-todos.ts): a lista velha da página nunca sobrescreve o que o Hub disse por último.
  const [interruptor, setInterruptor] = useState(() => interruptorInicial(valor))
  const [ocupado, setOcupado] = useState(false)
  const [erro, setErro] = useState<'permissao' | 'semFuncao' | 'generico' | null>(null)
  const { ligado, incerto, conferido } = interruptor
  // O valor da lista no momento em que a resposta chega (lido depois do await, nunca o capturado antes do pedido): uma lista
  // que confirmou o resultado durante o pedido não pode deixar o interruptor esperando por ela.
  const listaAgora = useRef(valor)
  listaAgora.current = valor
  useEffect(() => setInterruptor((e) => proximoInterruptor(e, { tipo: 'lista', valor })), [valor])
  const mudar = async (novo: boolean) => {
    const origem = origemDe(store)
    if (!origem) return
    setOcupado(true)
    setErro(null)
    try {
      const resultado = await alterarNovosBots(origem.client, { tipo: 'skill', id }, novo)
      if (!mesmaOrigem(store, origem)) return
      if (resultado.tipo === 'erro') return setErro(resultado.erro)
      setInterruptor((e) => proximoInterruptor(e, { tipo: 'resultado', resultado, lista: listaAgora.current }))
      if (resultado.tipo !== 'incerto') aoMudar?.()
    } finally {
      setOcupado(false)
    }
  }
  /** Relê o Hub, sem repetir a gravação. */
  const conferir = async () => {
    const origem = origemDe(store)
    if (!origem) return
    setOcupado(true)
    try {
      const lido = await lerNovosBots(origem.client, id)
      if (!mesmaOrigem(store, origem)) return
      setInterruptor((e) => proximoInterruptor(e, { tipo: 'conferiu', lido, lista: listaAgora.current }))
      if (lido !== null) aoMudar?.()
    } finally {
      setOcupado(false)
    }
  }
  return (
    <div className="eet-novos">
      <Switch on={ligado} disabled={ocupado || incerto} onChange={(v) => void mudar(v)} label={t('extensoes.emTodos.novosBots.nome', { nome: isolado(nome) })} className="eet-interruptor">{t('extensoes.emTodos.novosBots.rotulo')}</Switch>
      <p className="eet-nota">{t('extensoes.emTodos.novosBots.nota')}</p>
      {erro ? <p className="eet-erro" role="alert">{t(CHAVE_ERRO[erro])}</p> : null}
      {conferido ? <p className="eet-nota" role="status">{t('extensoes.emTodos.novosBots.conferido')}</p> : null}
      {incerto ? (
        <>
          <p className="eet-erro" role="alert">{t('extensoes.emTodos.novosBots.incerto')}</p>
          <div className="eet-acoes"><Button kind="secondary" disabled={ocupado} onClick={() => void conferir()}>{t('extensoes.emTodos.novosBots.conferir')}</Button></div>
        </>
      ) : null}
    </div>
  )
}
