// DON-903: o idioma escolhido, guardado como as outras preferências da janela (lib/prefs.ts), e o
// gancho que re-renderiza tudo na hora em que ele muda, sem reiniciar. Sincroniza `lang` e `dir` do <html>.
import { useSyncExternalStore } from 'react'
import { direcaoDe, FALLBACK_LANGUAGE, isAppLanguage, type AppLanguage } from '../../../shared/fork-idioma.ts'
import { pref } from '../prefs.ts'
import { tradutor, type Tradutor } from './index.ts'

/** `null`: nunca escolhido. D454/D495: sem escolha salva, o app usa o inglês; com escolha salva, ela vale. */
export const idiomaPref = pref<AppLanguage | null>('appLanguage', null, (v): v is AppLanguage | null => v === null || isAppLanguage(v))

/**
 * MX-1358: o idioma que vale enquanto ninguém escolheu. O desktop não usa (continua no inglês até o passo de idioma);
 * a web instala aqui a sugestão do navegador, a mesma que a tela de login já usava, para o shell falar o idioma do login.
 * Nunca é gravado como escolha: só a pessoa grava (`idiomaPref.set`).
 */
let idiomaDePartida: () => AppLanguage | null = () => null

export function definirIdiomaDePartida(fonte: () => AppLanguage | null): void {
  idiomaDePartida = fonte
  if (typeof document !== 'undefined') aplicarDocumento()
}

export function idiomaAtual(): AppLanguage {
  const bridge = typeof window === 'undefined' ? undefined : window.codync
  const open = bridge?.appScheme === 'mexerica-open' ? bridge.suggestedLanguage : null
  return idiomaPref.get() ?? idiomaDePartida() ?? (isAppLanguage(open) ? open : FALLBACK_LANGUAGE)
}

/**
 * `lang` e `dir` do documento seguem o idioma (leitores de tela, fontes por escrita, RTL). Transição (D363):
 * o `<body>` é o limite do legado ainda em português, `lang="pt-BR" dir="ltr"`, para que telas não migradas
 * (e os diálogos que elas abrem no body) não espelhem pela metade. Cada superfície migrada declara o próprio
 * `lang`/`dir` (`superficieIdioma`). O limite sai quando o inventário do legado zerar.
 */
export const LIMITE_LEGADO = { lang: 'pt-BR', dir: 'ltr' } as const

function aplicarDocumento() {
  const idioma = idiomaAtual()
  document.documentElement.lang = idioma
  document.documentElement.dir = direcaoDe(idioma)
  document.body?.setAttribute('lang', LIMITE_LEGADO.lang)
  document.body?.setAttribute('dir', LIMITE_LEGADO.dir)
}

// `superficieIdioma` mora em index.ts (puro); reexportada aqui como antes.
export { superficieIdioma } from './index.ts'
// Sem DOM (render no Node, testes de views que importam uma superfície migrada), não há documento a sincronizar.
if (typeof document !== 'undefined') {
  aplicarDocumento()
  idiomaPref.subscribe(aplicarDocumento)
}

export function useIdioma(): AppLanguage {
  // O terceiro argumento é o mesmo valor na renderização em servidor (testes SSR das telas traduzidas).
  return useSyncExternalStore(idiomaPref.subscribe, idiomaAtual, idiomaAtual)
}

// `Tradutor` e `tradutor` moram em index.ts (puro, usável nos testes do Node); reexportados aqui como antes.
export { tradutor, type Tradutor } from './index.ts'

/** `t` no idioma atual; a view re-renderiza quando o idioma muda. */
export function useT(): Tradutor {
  return tradutor(useIdioma())
}
