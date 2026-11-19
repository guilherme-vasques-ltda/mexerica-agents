// DON-908 (C8): o custo estimado desta conversa no balão do medidor de contexto (C2). Lê só com o balão aberto
// (`donnaCostReport {botId, sessionId?}`, todo o histórico da C8 da sessão, filhos inclusos). Hub sem o RPC ou
// conexão que não é do dono: o bloco some, sem aviso. A nota fixa aparece sempre com o bloco.
import { useModel } from '../../../lib/observable'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { useCusto } from './useCusto'
import { dinheiro } from './modelo'
import { parcialDe, tokensDe } from './textos'
import './custo.css'

export function CustoConversa({ botId, sessionId, aberto }: { botId: string; sessionId: string | null; aberto: boolean }) {
  const store = useModel(useStore())
  const idioma = useIdioma()
  const t = useT()
  const { estado } = useCusto(store, { botId, sessionId }, aberto)
  if (estado.kind === 'lendo' || (estado.kind === 'falha' && (estado.falha === 'antigo' || estado.falha === 'soDono'))) return null
  const c = estado.kind === 'pronto' ? estado.relatorio.conversation : null
  if (estado.kind === 'pronto' && !c) return null
  const p = 'contexto.custo'
  const aviso = c ? parcialDe(t, idioma, p, c.totals) : null
  const valor = c ? dinheiro(idioma, c.totals.costs) : null
  return (
    <div className="ctx-custo">
      {!c ? <div className="ctx-linha ctx-nota">{t('contexto.custo.falha')}</div> : c.totals.turns === 0 ? (
        <>
          <div className="ctx-linha ctx-titulo"><span>{t('contexto.custo.titulo')}</span></div>
          <div className="ctx-linha ctx-nota">{t('contexto.custo.vazio')}</div>
        </>
      ) : (
        <>
          {/* Com dinheiro do motor: título e valor. Sem preço: só as partes de tokens e a nota (sem linha de valor). */}
          {valor ? <div className="ctx-linha ctx-titulo"><span>{t('contexto.custo.titulo')}</span><span className="ctx-valor">{valor}</span></div> : null}
          <div className={`ctx-linha${valor ? ' ctx-nota' : ''}`}>{tokensDe(t, idioma, p, c.totals)}</div>
          {aviso ? <div className="ctx-linha ctx-nota">{aviso}</div> : null}
        </>
      )}
      <div className="ctx-linha ctx-nota ctx-custo-nota">{t('contexto.custo.nota')}</div>
    </div>
  )
}
