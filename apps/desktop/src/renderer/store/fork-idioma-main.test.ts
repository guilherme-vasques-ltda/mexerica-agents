import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { registerHooks } from 'node:module'
import test from 'node:test'

// D454/D495 (R1 do #563), D70: o idioma efetivo vai ao processo principal (menu do Mac e barra) e, pela D395, ao Hub.
// Sem escolha salva o efetivo é o inglês: o main recebe 'en' e o Hub recebe 'en' (o Berga passa a responder em inglês,
// efeito pretendido da D454 com a D395 e a D495). Com a escolha pt-BR, os dois recebem 'pt-BR'.
// O estado real importa módulos sem extensão (como no build): resolve `.ts`/`.tsx`.
registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
      for (const extension of ['.ts', '.tsx', '/index.ts']) {
        const candidate = new URL(`${specifier}${extension}`, context.parentURL)
        if (existsSync(candidate)) return next(candidate.href, context)
      }
    }
    return next(specifier, context)
  },
})
const salvo = new Map<string, string>()
Object.assign(globalThis, {
  window: { addEventListener() {} },
  localStorage: { getItem: (k: string) => salvo.get(k) ?? null, setItem: (k: string, v: string) => void salvo.set(k, v), removeItem: (k: string) => void salvo.delete(k) },
})
const { idiomaAtual, idiomaPref } = await import('../lib/fork-i18n/estado.ts')
const { registrarIdiomaNoMain } = await import('./fork-idioma-main.ts')
const { acompanharIdioma, enviarIdioma, registrarIdioma } = await import('./fork-idioma-hub.ts')

test('D454/D495: sem escolha, o main e o Hub recebem "en"; com a escolha pt-BR, "pt-BR"', async () => {
  assert.equal(idiomaPref.get(), null)
  const main: string[] = []
  const pararMain = registrarIdiomaNoMain(idiomaAtual, idiomaPref.subscribe, (idioma) => main.push(idioma))
  const hub: unknown[] = []
  const cliente = { call: async (method: string, body: Record<string, unknown>) => { hub.push([method, body]); return {} as never } }
  const pararHub = registrarIdioma(idiomaAtual, idiomaPref.subscribe)
  const sair = acompanharIdioma(() => cliente)
  enviarIdioma(cliente) // ao conectar
  assert.deepEqual(main, ['en'])
  assert.deepEqual(hub, [['donnaAppLanguageSave', { language: 'en' }]])
  idiomaPref.set('pt-BR')
  assert.deepEqual(main, ['en', 'pt-BR'])
  assert.deepEqual(hub.at(-1), ['donnaAppLanguageSave', { language: 'pt-BR' }])
  sair(); pararHub(); pararMain()
  idiomaPref.set('fr')
  assert.deepEqual(main, ['en', 'pt-BR'], 'depois de parar, nada mais vai')
})
