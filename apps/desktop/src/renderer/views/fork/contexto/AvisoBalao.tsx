import { Icon } from '../../../components/Icon'
import type { Tradutor } from '../../../lib/fork-i18n/estado'
import type { Aviso } from './estado'

const livre = (t: Tradutor) => t as unknown as (chave: string, vars?: Record<string, string>) => string

/** A faixa de aviso do balão: ícone + título + frase. `role="status"` e é o `aria-describedby` do botão. */
export function AvisoBalao({ id, aviso, t }: { id?: string; aviso: Aviso; t: Tradutor }) {
  const nome = { relogio: 'clock', ok: 'checkmark', info: 'info.circle', alerta: 'exclamationmark.triangle' }[aviso.icone]
  return (
    <div id={id} className={`ctx-aviso t-${aviso.tom}`} role="status">
      <span className="ctx-aviso-ic"><Icon name={nome} size={12} weight="semibold" /></span>
      <div>
        <div className="ctx-aviso-t">{livre(t)(`contexto.aviso.${aviso.id}.titulo`, aviso.vars)}</div>
        <div className="ctx-aviso-d">{livre(t)(`contexto.aviso.${aviso.id}.detalhe`, aviso.vars)}</div>
      </div>
    </div>
  )
}
