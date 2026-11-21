import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import type { Backend } from '../../../../shared/models.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'

// D224: aviso, sem bloqueio, do que a trava de segurança do Hub não cobre em cada motor.
// Um motor novo entra aqui e no catálogo (conversa.avisos.motor.<motor>); o componente e os pontos de exibição não mudam.
const avisos = { opencode: 'conversa.avisos.motor.opencode' } as const
export type ChaveAvisoMotor = (typeof avisos)[keyof typeof avisos]

/** A chave do aviso do motor escolhido, ou `null` quando não há o que avisar. */
export function avisoDoMotor(backend: string): ChaveAvisoMotor | null {
  return Object.hasOwn(avisos, backend) ? avisos[backend as keyof typeof avisos] : null
}

/** O Hub marcou o motor como "age sem pedir aprovação" (`actsWithoutApproval`). Sem o campo, não há o que avisar. */
export const ageSemAprovacao = (b: Pick<Backend, 'actsWithoutApproval'> | undefined): boolean => b?.actsWithoutApproval === true

/** O selo curto e a explicação do motor que age sem pedir, no idioma pedido. O aviso só informa: o motor segue escolhível. */
export const textosSemAprovacao = (idioma: AppLanguage) => ({
  selo: traduzir(idioma, 'conversa.avisos.motor.semAprovacaoSelo'),
  texto: traduzir(idioma, 'conversa.avisos.motor.semAprovacao'),
})

/** O rótulo de um motor numa lista de texto simples: o nome, com "não instalado" ou o selo de "age sem pedir". */
export function rotuloDeMotor(b: Pick<Backend, 'name' | 'available' | 'actsWithoutApproval'>, idioma: AppLanguage): string {
  const notas = [
    ...(b.available ? [] : [traduzir(idioma, 'botCreate.naoInstalado')]),
    ...(ageSemAprovacao(b) ? [textosSemAprovacao(idioma).selo] : []),
  ]
  return notas.length ? `${b.name} (${notas.join(', ')})` : b.name
}
