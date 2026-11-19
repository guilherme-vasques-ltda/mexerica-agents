import { createElement } from 'react'
import type { Tradutor } from '../../lib/fork-i18n/index.ts'

/** Interpola só os marcadores do catálogo. Dados livres viram nós de texto isolados, nunca HTML. */
function partes(template: string, dados: Record<string, string>) {
  return template.split(/(\{[a-z]+\})/u).map((part, key) => {
    const field = part.slice(1, -1)
    return part.startsWith('{') && Object.hasOwn(dados, field)
      ? createElement('bdi', { key, dir: 'auto' }, dados[field]) : part
  })
}
export function ContaConfirmadaTexto({ t, conta, motor, hub }: { t: Tradutor; conta: string; motor: string; hub: string }) {
  return partes(t('botCreate.confirmarContaSpace', { conta: '{conta}', motor: '{motor}', hub: '{hub}' }), { conta, motor, hub })
}
export function BotCriadoNaOrigemTexto({ t, nome, space, hub }: { t: Tradutor; nome: string; space: string; hub: string }) {
  return partes(t('botCreate.criadoNaOrigem', { nome: '{nome}', space: '{space}', hub: '{hub}' }), { nome, space, hub })
}
