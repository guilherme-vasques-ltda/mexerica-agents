// "Compactando…" no chat (desenho aprovado pelo dono, 16/11, opção A): a marca no fluxo das mensagens e o gancho que o
// campo de mensagem usa para esperar. Só desenha; a regra (quando bloqueia, quando libera) está em `estado.ts`.
import { useEffect, useRef } from 'react'
import { isWorkingIn, type Bot } from '@shared/models'
import { Button, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useModel } from '../../../lib/observable'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useStoreRef } from '../../../store/context'
import { modeloDe } from './BarraContexto'
import { medidaDaMarca, type Compactacao } from './estado'
import './contexto.css'

/**
 * A compactação desta conversa, ou null. Enquanto a atividade do bot diz "resumindo" na conversa principal (extra não
 * tem atividade própria no Hub, só o pedido manual vale), a tela observa no modelo: o campo e o fluxo dividem um
 * registro e um relógio, o limite de espera vale mesmo depois de trocar de conexão, e desmontar a tela solta o envio.
 */
export function useCompactacao(bot: Bot | null, botId: string, sessionId: string | null): Compactacao | null {
  const modelo = useModel(modeloDe(useStoreRef()))
  const ativa = sessionId === null && !!bot && isWorkingIn(bot, botId) && bot.activityCode?.kind === 'summarizing'
  // Só a transição "resumindo → não resume" diz que a compactação acabou; montar a tela com a atividade ainda ausente
  // (ou desmontá-la) não diz, e não pode apagar o primeiro sinal do prazo de 90 s.
  const estavaAtiva = useRef(false)
  useEffect(() => {
    const acabou = estavaAtiva.current && !ativa
    estavaAtiva.current = ativa
    if (ativa) return modelo.observarAtividade(botId, sessionId)
    if (acabou) modelo.atividadeAcabou(botId, sessionId)
    return undefined
  }, [modelo, botId, sessionId, ativa])
  // O registro nasce no efeito; no primeiro quadro a atividade já vale, para o campo nunca deixar um envio passar.
  return modelo.compactacao(botId, sessionId, ativa) ?? (ativa ? { fase: 'andamento' } : null)
}

/** A marca no fluxo: fio, pílula, fio. Concluída e erro calmo ficam no mesmo lugar, na mesma região `role="status"`. */
export function MarcaCompactando({ compactacao, botId, sessionId }: { compactacao: Compactacao; botId: string; sessionId: string | null }) {
  const store = useStoreRef()
  const modelo = useModel(modeloDe(store))
  const idioma = useIdioma()
  const t = useT()
  const superficie = superficieIdioma(idioma)
  const medida = medidaDaMarca(idioma, compactacao)
  const leitura = modelo.of(botId, sessionId)
  const atencao = compactacao.fase === 'incerto' || compactacao.fase === 'falha'
  if (atencao) {
    return (
      <div className="chat-row notice idioma-superficie" {...superficie}>
        <div className="compactando-erro" role="status">
          <Icon name="exclamationmark.triangle" size={16} />
          <div className="compactando-erro-texto">
            <div className="compactando-erro-t">{t(compactacao.fase === 'falha' ? 'contexto.compactando.falha.titulo' : 'contexto.compactando.incerto.titulo')}</div>
            <div className="compactando-erro-d">{t('contexto.compactando.liberado')}</div>
          </div>
          {compactacao.fase === 'falha' && leitura?.canCompact
            ? <Button kind="secondary" onClick={() => void modelo.comprimir(botId, sessionId, leitura.contextId)}>{t('contexto.ajustes.tentarDeNovo')}</Button> : null}
        </div>
      </div>
    )
  }
  const andando = compactacao.fase === 'andamento'
  return (
    <div className="chat-row notice idioma-superficie" {...superficie}>
      <div className="compactando-marca" role="status" aria-live="polite">
        <span className="compactando-fio" aria-hidden="true" />
        <span className="chat-pill glass-raised compactando-pilula">
          {andando ? <span className="compactando-giro" aria-hidden="true"><Spinner size={13} /></span>
            : <span className="compactando-ok" aria-hidden="true"><Icon name="checkmark" size={11} weight="bold" /></span>}
          <b>{t(andando ? 'contexto.compactando.titulo' : 'contexto.compactando.concluida')}</b>
          {andando ? <span className="compactando-detalhe">{t('contexto.compactando.detalhe')}</span>
            : medida ? <span className="compactando-detalhe">{t('contexto.compactando.medida', medida)}</span> : null}
        </span>
        <span className="compactando-fio" aria-hidden="true" />
      </div>
      {andando ? <div className="compactando-traco" aria-hidden="true" /> : null}
    </div>
  )
}

/** O motivo acima do campo, em uma linha; `destaque` acende por instantes quando a pessoa tenta enviar. */
export function MotivoCompactando({ id, destaque }: { id: string; destaque: boolean }) {
  const t = useT()
  return (
    <div className={`compactando-motivo${destaque ? ' is-destaque' : ''}`}>
      <Icon name="arrow.down.right.and.arrow.up.left" size={14} />
      <span id={id} role="status">{t('contexto.compactando.motivo')}</span>
      {/* O anúncio de quem tentou enviar: lido uma vez por tentativa. */}
      <span className="compactando-leitor" role="status" aria-live="assertive">{destaque ? t('contexto.compactando.tentou') : ''}</span>
    </div>
  )
}
