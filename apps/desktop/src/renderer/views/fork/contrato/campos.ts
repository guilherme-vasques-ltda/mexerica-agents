// T2a (DON-912): a ordem e os rótulos dos campos do contrato. Puro: sem I/O nem React.
import type { TaskContract } from '../../../../shared/models.ts'

const ROTULOS = {
  result: 'contrato.resultado',
  acceptance: 'contrato.aceite',
  scope: 'contrato.escopo',
  limits: 'contrato.limites',
  rule: 'contrato.regra',
} as const

export type RotuloContrato = (typeof ROTULOS)[keyof typeof ROTULOS]

/** Os campos com texto, na ordem do Hub; campo vazio ou fora do formato não aparece. */
export function camposDoContrato(c: TaskContract): { chave: keyof TaskContract; rotulo: RotuloContrato; texto: string }[] {
  return (Object.keys(ROTULOS) as (keyof TaskContract)[]).flatMap((chave) => {
    const texto = c[chave]
    return typeof texto === 'string' && texto.trim() ? [{ chave, rotulo: ROTULOS[chave], texto }] : []
  })
}
