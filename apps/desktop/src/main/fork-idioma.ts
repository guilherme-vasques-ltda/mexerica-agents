import { app } from 'electron'
import { FALLBACK_LANGUAGE, isAppLanguage, sugerirIdioma, type AppLanguage } from '../shared/fork-idioma'

/**
 * DON-903: lê só os idiomas preferidos do sistema (local, sem rede, sem permissão) e devolve ao renderer
 * apenas o idioma do app sugerido, nunca a lista inteira.
 */
export function suggestedLanguage(): AppLanguage | null {
  try {
    return sugerirIdioma(app.getPreferredSystemLanguages())
  } catch {
    return null
  }
}

/**
 * D454/D495 (R1 do #563): o idioma efetivo do app, para o que o processo principal desenha (menu do Mac, painel Sobre,
 * barra de menus). A escolha fica no renderer (`appLanguage`); ele avisa aqui ao abrir e a cada troca
 * (`app:setLanguage`). Até o primeiro aviso, e sem escolha salva, vale o padrão inglês.
 */
let idiomaApp: AppLanguage = FALLBACK_LANGUAGE
const ouvintes = new Set<() => void>()

export function idiomaDoApp(): AppLanguage {
  return idiomaApp
}

/** Grava o idioma vindo do renderer. Valor fora dos 6 é ignorado; só uma troca de verdade avisa. */
export function definirIdiomaDoApp(valor: unknown): boolean {
  if (!isAppLanguage(valor) || valor === idiomaApp) return false
  idiomaApp = valor
  for (const ouvinte of ouvintes) ouvinte()
  return true
}

/** Quem redesenha com o idioma (menu, barra) se registra aqui. Devolve como sair. */
export function aoMudarIdiomaDoApp(ouvinte: () => void): () => void {
  ouvintes.add(ouvinte)
  return () => void ouvintes.delete(ouvinte)
}
