// D443 (antes D348): o menu do app no macOS diz "Mexerica" nos itens que mostram o nome ("Sobre a Mexerica", D445a). Sem rótulo, o Electron monta "About
// Donnabot", "Hide Donnabot" e "Quit Donnabot" com o nome técnico (`app.name`, "Donnabot", definido em main/environment.ts). Os papéis
// (`role`) continuam, e com eles o comportamento do sistema. Desde o PR (d), `productName`/`CFBundleName` (Dock, Finder
// e o título em negrito da barra) são "Mexerica"; o `app.name` técnico não muda aqui.
import { nomeVisivel } from './fork-nome-visivel.ts'
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
import { FALLBACK_LANGUAGE, type AppLanguage } from './fork-idioma.ts'

type Papel = 'about' | 'services' | 'hide' | 'hideOthers' | 'unhide' | 'quit'
export type ItemMenuApp =
  | { role: Papel; label?: string }
  | { type: 'separator' }
  | { label: string; accelerator: string; click: () => void }

/**
 * Os itens do menu do app no Mac, com "Ajustes…" (⌘,) no lugar de sempre (ajustes-tela-01), no idioma efetivo do app
 * (D454/D495: sem escolha, inglês; `main/fork-idioma.ts`).
 */
export function itensMenuApp(appName: string, abrirAjustes: () => void, idioma: AppLanguage = FALLBACK_LANGUAGE): ItemMenuApp[] {
  const nome = nomeVisivel(appName)
  return [
    { role: 'about', label: traduzir(idioma, 'marca.menu.sobre', { nome }) },
    { type: 'separator' },
    { label: traduzir(idioma, 'marca.menu.ajustes'), accelerator: 'CmdOrCtrl+,', click: abrirAjustes },
    { type: 'separator' },
    { role: 'services' },
    { type: 'separator' },
    { role: 'hide', label: traduzir(idioma, 'marca.menu.ocultar', { nome }) },
    { role: 'hideOthers' },
    { role: 'unhide' },
    { type: 'separator' },
    { role: 'quit', label: traduzir(idioma, 'marca.menu.encerrar', { nome }) },
  ]
}

/**
 * D445c: o painel "Sobre" do Mac mostra "Mexerica" com a linha pequena "baseado no Codync". Sem isto, o painel padrão
 * usa o nome do bundle ("Donnabot", técnico até o PR d). Versão e ícone continuam os do bundle.
 */
export function painelSobre(appName: string, idioma: AppLanguage = FALLBACK_LANGUAGE): { applicationName: string; credits: string } {
  return { applicationName: nomeVisivel(appName), credits: traduzir(idioma, 'marca.sobre.baseado') }
}
