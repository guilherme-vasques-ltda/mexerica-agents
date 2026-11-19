import { useEffect, useState } from 'react'
import type { Entry } from '@shared/models'
import { BotAvatar } from '../../../components/Avatar'
import { Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack } from '../../../components/Stack'
import { useReduceMotion } from '../../../lib/theme'
import { useStoreRef } from '../../../store/context'
import { HoldButton } from '../../thread/cards/HoldButton'
import '../../thread/cards/approval-card.css'
import './conector-card.css'
import { IdentidadeConta } from '../whatsapp/contato'
import { textoDe, cru, ErroFrase, frase, type Frase } from '../../../lib/fork-i18n/frase.ts'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { PROTOCOLO, decidir, editar, lerPedido, tela, type Chamar, type LeituraConector, type ReciboConector, type Tom } from './modelo'

type Acao = 'aprovar' | 'recusar' | 'editar'

const ICONE: Record<Tom, string> = { espera: 'clock', ok: 'checkmark', neutro: 'xmark', alerta: 'exclamationmark.triangle' }

/**
 * R6a (components/ApprovalCard): o cartão comum de uma ação de conector. A entrada pública só traz a
 * referência; o conteúdo vem do RPC Local `donnaConectorTentativa`. Segure aprova uma vez, Editar pede ao
 * Hub um cartão novo, Recusar é um clique. O Hub confere cartão, revisão, prazo e quem pede.
 */
export function ConectorCard({ entry }: { entry: Entry }) {
  const store = useStoreRef()
  const op = entry.data.conectorOperation!
  const reducedMotion = useReduceMotion()
  const [leitura, setLeitura] = useState<LeituraConector | null>(null)
  // Lição da R1 do #572: erros como frase (o que vem do Hub, como dado), traduzidos ao desenhar.
  const [erroLeitura, setErroLeitura] = useState<Frase | null>(null)
  const [acao, setAcao] = useState<Acao | null>(null)
  const [erro, setErro] = useState<Frase | null>(null)
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const comoFrase = (e: unknown) => (e instanceof ErroFrase ? e.frase : cru(e instanceof Error ? e.message : String(e)))
  const [editando, setEditando] = useState<Record<string, string> | null>(null)
  const [agora, setAgora] = useState(Date.now)
  const suportado = op.decisionProtocol === PROTOCOLO
  const chamar: Chamar = async (metodo, corpo) => (await store.ready()).call<unknown>(metodo, corpo)

  // Relê a cada revisão da entrada: o Hub muda estado, revisão e cartão por ela.
  useEffect(() => {
    if (!suportado) return
    let vivo = true
    setErroLeitura(null)
    void (async () => {
      try {
        const lida = await lerPedido(chamar, op.tentativaId)
        if (vivo) setLeitura(lida)
      } catch (e) {
        if (vivo) setErroLeitura(comoFrase(e))
      }
    })()
    return () => { vivo = false }
  }, [store, op.tentativaId, entry.rev, suportado])

  // O selo vira "Venceu" no prazo do Hub, sem clique. O Hub também recusa depois dele.
  const prazo = leitura?.recibo.cardDeadline
  useEffect(() => {
    if (!prazo) return
    const t = setTimeout(() => setAgora(Date.now()), Math.max(0, prazo - Date.now() + 50))
    return () => clearTimeout(t)
  }, [prazo])

  const recibo = leitura?.recibo
  const estado = recibo?.state ?? op.state
  const apresentacao = leitura?.apresentacao ?? null
  const t = tela(estado, {
    suportado,
    apresentacao: !!apresentacao,
    conta: !!apresentacao?.conta,
    vencido: !!recibo && recibo.cardDeadline <= agora,
    substituido: !!recibo && recibo.cardId !== op.cardId,
    recusaLeitura: erroLeitura,
  })
  const bot = entry.data.author ? store.bots.get(entry.data.author) : undefined
  const nome = store.authorName(entry.data.author)
  const carregando = suportado && !leitura && !erroLeitura

  const pedir = async (qual: Acao, fazer: () => Promise<ReciboConector | null>) => {
    setAcao(qual)
    setErro(null)
    try {
      const novo = await fazer()
      if (novo) setLeitura((antes) => ({ recibo: novo, apresentacao: antes?.apresentacao ?? null }))
      setEditando(null)
    } catch (e) {
      setErro(comoFrase(e))
    } finally {
      setAcao(null)
    }
  }
  const decisao = (d: 'aprovar_uma_vez' | 'recusar') => recibo && void pedir(d === 'recusar' ? 'recusar' : 'aprovar', () => decidir(chamar, recibo, d))
  const salvar = () => recibo && apresentacao && editando && void pedir('editar', () => editar(chamar, recibo, apresentacao.argumentos, editando))

  // Nome do bot e efeito vêm do Hub (dados): isolados na frase.
  const titulo = tx(apresentacao ? frase('conectores.cartao.quer', { nome, efeito: apresentacao.efeito }) : frase('conectores.cartao.pediu', { nome }))
  return (
    <div className="agent-inset">
      {/* Cartão de aprovação (R1 do #565): idioma e direção na raiz; o que vai sair (conta, campos) na direção do próprio dado, isolado. */}
      <section className="approval-card conector-card idioma-superficie" {...superficieIdioma(idioma)} aria-label={titulo}>
        <HStack gap={8} className="conector-card-topo">
          <span className={`approval-badge conector-tom-${t.tom}`}>
            {carregando ? <Spinner size={12} /> : <Icon name={ICONE[t.tom]} size={12} weight="semibold" />}{carregando ? tx(frase('conectores.cartao.abrindo')) : tx(t.selo)}
          </span>
          <span className="conector-card-quem">{tx(frase('conectores.cartao.pedidoDe'))}{bot ? <BotAvatar bot={bot} size={18} animated={false} /> : null}<strong dir="auto">{nome}</strong></span>
        </HStack>
        <h3 className="approval-title">{titulo}</h3>
        {apresentacao ? (
          <dl className="approval-facts conector-card-fatos">
            <dt>{tx(frase('conectores.cartao.contaDeSaida'))}</dt>
            <dd>
              {apresentacao.conta ? (
                <><bdi className="selectable conector-card-conta" dir="ltr"><IdentidadeConta identidade={apresentacao.conta.identidade} /></bdi><bdi className="conector-card-conta-rotulo" dir="auto">{apresentacao.conta.rotulo}</bdi></>
              ) : (
                <span className="conector-card-conta-ausente"><Icon name="exclamationmark.triangle" size={12} />{tx(frase('conectores.cartao.naoIdentificada'))}</span>
              )}
            </dd>
            {Object.entries(apresentacao.argumentos).map(([campo, valor]) => (
              <Fato key={campo} campo={campo} rotulo={apresentacao.rotulos[campo]!} valor={valor} editando={editando} mudar={(v) => setEditando((e) => e && { ...e, [campo]: v })} />
            ))}
          </dl>
        ) : null}
        {t.nota ? <p className={`approval-note conector-card-nota${t.tom === 'alerta' ? ' alerta' : ''}`}>{tx(t.nota)}</p> : null}
        {erro ? <p className="approval-note conector-card-erro" role="alert"><Icon name="exclamationmark.circle" size={13} />{tx(frase('conectores.cartao.naoAceitou', { motivo: erro }))}</p> : null}
        {t.decide && recibo && apresentacao ? (
          editando ? (
            <>
              <div className="approval-actions">
                <button className="hold-button primary" disabled={acao !== null} onClick={salvar}>
                  <span className="hold-button-label"><HStack as="span" gap={6}>{tx(frase('conectores.cartao.salvarNovo'))}{acao === 'editar' ? <Spinner size={12} /> : null}</HStack></span>
                </button>
                <button className="approval-deny" disabled={acao !== null} onClick={() => setEditando(null)}>{tx(frase('conectores.cartao.desfazer'))}</button>
              </div>
              <p className="approval-note"><Icon name="pencil" size={13} />{tx(frase('conectores.cartao.notaEdicao'))}</p>
            </>
          ) : (
            <>
              <div className="approval-actions">
                <HoldButton primary label={tx(frase('conectores.cartao.aprovarUmaVez'))} disabled={acao !== null} onConfirm={() => decisao('aprovar_uma_vez')}>
                  <HStack as="span" gap={6}><Icon name="checkmark" size={13} weight="semibold" />{tx(frase(reducedMotion ? 'conectores.cartao.aprovar' : 'conectores.cartao.segure'))}{acao === 'aprovar' ? <Spinner size={12} /> : null}</HStack>
                </HoldButton>
                <button className="conector-card-editar" disabled={acao !== null} onClick={() => setEditando({ ...apresentacao.argumentos })}>
                  <HStack as="span" gap={6}><Icon name="square.and.pencil" size={13} />{tx(frase('conectores.cartao.editar'))}</HStack>
                </button>
                <span className="conector-card-espaco" />
                <button className="approval-deny" disabled={acao !== null} onClick={() => decisao('recusar')}>
                  <HStack as="span" gap={6}>{tx(frase('conectores.cartao.recusar'))}{acao === 'recusar' ? <Spinner size={12} /> : null}</HStack>
                </button>
              </div>
              <p className="approval-note"><Icon name="checkmark.shield" size={13} />{tx(frase('conectores.cartao.notaDecisao'))}</p>
            </>
          )
        ) : null}
      </section>
    </div>
  )
}

function Fato({ campo, rotulo, valor, editando, mudar }: { campo: string; rotulo: string; valor: string; editando: Record<string, string> | null; mudar: (v: string) => void }) {
  return (
    <>
      <dt dir="auto">{rotulo}</dt>
      <dd>
        {editando ? (
          <textarea className="conector-card-campo" dir="auto" aria-label={rotulo} value={editando[campo] ?? valor} rows={Math.min(6, Math.max(1, Math.ceil(valor.length / 60)))} onChange={(e) => mudar(e.target.value)} />
        ) : (
          <bdi className="selectable conector-card-valor" dir="auto">{valor}</bdi>
        )}
      </dd>
    </>
  )
}
