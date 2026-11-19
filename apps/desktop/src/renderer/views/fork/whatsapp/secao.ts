// D350 (dono, 07/11/2026): "so deve aparecer essa tela, depois que a pessoa vai em plugins e ativa o whatsapp".
// Com o plugin WhatsApp desligado (ou ainda sem a leitura do Hub), o item "WhatsApp" some do menu das
// Configurações do bot e a seção não aparece. Puro, para testar sem DOM.

import type { LeituraWhatsapp } from './regras.ts'

/** O plugin está ligado, pela leitura do Hub; sem leitura (carregando, erro), conta como desligado. */
export const pluginLigado = (leitura: LeituraWhatsapp | null | undefined) => leitura?.plugin.enabled === true

/** As seções das Configurações do bot que aparecem: sem o plugin, sem a do WhatsApp. */
export function secoesVisiveis<T extends readonly [string, string]>(secoes: readonly T[], ligado: boolean): T[] {
  return secoes.filter(([id]) => id !== 'whatsapp' || ligado)
}

/** A seção atual, se ainda aparece; senão a primeira (a do WhatsApp que sumiu volta para Identidade). */
export function secaoValida<S extends string>(atual: S, visiveis: readonly (readonly [S, string])[]): S {
  return visiveis.some(([id]) => id === atual) ? atual : (visiveis[0]?.[0] ?? atual)
}
