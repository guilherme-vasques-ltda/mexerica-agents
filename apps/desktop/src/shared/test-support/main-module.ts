import { readFileSync } from 'node:fs'
import { createContext, Script } from 'node:vm'
import ts from 'typescript'

/**
 * MEX-1431: a transpilação (TypeScript) era refeita a cada `loadMainModule`; com um rig por teste (fork-dock-ciclo,
 * ~17 rigs de `index.ts` + catálogos de texto) cada teste gastava ~13 s de CPU e o arquivo estourava o tempo do job.
 * O cache guarda só o JS transpilado, chaveado pelo endereço e pelo texto da fonte; cada carga ainda avalia o
 * módulo num contexto novo, então nenhum estado de módulo vaza entre testes.
 */
const transpiled = new Map<string, { source: string, output: string }>()
/** Quantas vezes o TypeScript transpilou de fato neste processo (o teste de regressão do cache lê isto). */
export const transpileStats = { calls: 0 }

function transpileCached(source: URL): string {
  const text = readFileSync(source, 'utf8')
  const hit = transpiled.get(source.href)
  if (hit && hit.source === text) return hit.output
  transpileStats.calls++
  const output = ts.transpileModule(text, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, esModuleInterop: true },
    fileName: source.pathname,
  }).outputText
  transpiled.set(source.href, { source: text, output })
  return output
}

/** Runs the actual main-process source with explicit, fail-closed OS boundaries. */
export function loadMainModule<T>(file: string, dependencies: Record<string, unknown>, globals: Record<string, unknown> = {}, required?: (name: string) => void): T {
  const url = new URL(`../../main/${file}.ts`, import.meta.url)
  const root = new URL('../../main/', import.meta.url)
  const shared = new URL('../', import.meta.url)
  // D443: os catálogos de texto (só dados) que o main e o shared leem, como o renderer.
  const catalogos = new URL('../../renderer/lib/fork-i18n/', import.meta.url)
  const sourceURL = (name: string, parent: URL) => new URL(name.endsWith('.ts') ? name : `${name}.ts`, parent)
  const mocks = new Map(Object.entries(dependencies).map(([name, value]) => [name.startsWith('.') ? sourceURL(name, url).href : name, value]))
  const modules = new Map<string, Record<string, unknown>>()
  // Um contexto V8 por carga (não por módulo): criar um contexto custava ~0,3 s e a árvore de catálogos tem dezenas de
  // arquivos por rig. Cada módulo roda embrulhado numa função CommonJS (`exports`, `require`), então não há
  // escopo de topo compartilhado, e cada `loadMainModule` continua com módulos, contexto e globais próprios.
  const context = createContext({ Error, Promise, console, AbortSignal, ...globals })
  const evaluate = (source: URL): Record<string, unknown> => {
    const cached = modules.get(source.href)
    if (cached) return cached
    const output = transpileCached(source)
    const exports = {}
    modules.set(source.href, exports)
    const require = (name: string) => {
      required?.(name)
      const local = name.startsWith('.') ? sourceURL(name, source) : null
      const key = local?.href ?? name
      if (mocks.has(key)) return mocks.get(key)
      // Follow the real fork hook; every external/OS dependency must still be explicit.
      if (local?.href.startsWith(root.href) || local?.href.startsWith(catalogos.href) || local?.href.startsWith(shared.href) && !local.href.includes('/test-support/')) return evaluate(local)
      throw new Error(`Unmocked main-process dependency: ${name}`)
    }
    // O prefixo fica na primeira linha: os números de linha dos erros seguem os da saída transpilada.
    const wrapper = new Script(`(function (exports, require) { ${output}\n})`, { filename: source.pathname }).runInContext(context) as (exports: object, require: (name: string) => unknown) => void
    wrapper(exports, require)
    return exports
  }
  return evaluate(url) as T
}

export function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

export const settle = () => new Promise<void>((resolve) => setImmediate(resolve))
