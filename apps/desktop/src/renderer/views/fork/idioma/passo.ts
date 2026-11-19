// DON-903, passo 1 da entrada (docs/reference/redesenho/entrada.md#idioma): regras sem DOM.
// O passo aparece até a pessoa tocar Continuar uma vez neste computador; depois, o idioma muda em
// Ajustes → Geral. A marca é apagada com "Apagar todos os dados", como a da apresentação.
// O passo nunca libera o portão da conta: ele só existe dentro das boas-vindas, com a conta não confirmada.
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'

const KEY = 'donnabot.entrada.idioma'

type Armazenamento = Pick<Storage, 'getItem' | 'setItem'>

export function passoIdiomaPendente(storage: Armazenamento | null): boolean {
  if (!storage) return false
  try {
    return storage.getItem(KEY) === null
  } catch {
    return false
  }
}

export function marcarPassoIdioma(storage: Armazenamento | null): void {
  try {
    storage?.setItem(KEY, 'escolhido')
  } catch {
    // Sem gravar, o passo volta na próxima abertura; a conta não depende disso.
  }
}

/** A ordem da roda e de Ajustes → Geral (D454/D495): English, Português (Brasil), depois os outros na ordem da D363. */
export const ORDEM_RODA: readonly AppLanguage[] = ['en', 'pt-BR', 'es', 'fr', 'zh-CN', 'ar-AE']

export interface TeclaRoda {
  key: string
  altKey: boolean
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
}

/**
 * O que uma tecla faz na roda (listbox): ↑/↓ vão ao vizinho sem dar a volta, Home/End às pontas e
 * Enter continua. `null`: tecla que a roda não usa.
 */
export function teclaRoda(e: TeclaRoda, atual: AppLanguage, ordem: readonly AppLanguage[] = ORDEM_RODA): { escolher: AppLanguage } | { continuar: true } | null {
  if (e.altKey || e.ctrlKey || e.metaKey || ordem.length === 0) return null
  if (e.key === 'Enter' && !e.shiftKey) return { continuar: true }
  if (e.shiftKey) return null
  const i = Math.max(0, ordem.indexOf(atual))
  const alvo = e.key === 'ArrowDown' ? Math.min(i + 1, ordem.length - 1)
    : e.key === 'ArrowUp' ? Math.max(i - 1, 0)
    : e.key === 'Home' ? 0
    : e.key === 'End' ? ordem.length - 1
    : null
  return alvo === null ? null : { escolher: ordem[alvo]! }
}
