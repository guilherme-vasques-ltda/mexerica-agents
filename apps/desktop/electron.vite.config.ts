import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'electron-vite'
import { admitsCloudBuild } from './src/shared/fork-cloud-build.ts'

let accountConfig: unknown = null
try { accountConfig = JSON.parse(readFileSync(resolve('resources/account-config.json'), 'utf8')) }
catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
const releaseDefines = { __DONNA_DEV_CLOUD_ACCESS__: JSON.stringify(admitsCloudBuild(accountConfig)) }

// Fresh macOS checkouts must include SF Symbols without a separate manual icons step.
if (process.platform === 'darwin') {
  execFileSync('swift', [resolve('tools/export-symbols.swift')], { stdio: 'inherit' })
}

export default defineConfig({
  main: { define: releaseDefines },
  // Fork D277: preload próprio da caixa protegida, separado do geral.
  preload: { define: releaseDefines, build: { rollupOptions: { input: { index: resolve('src/preload/index.ts'), 'fork-secret-box': resolve('src/preload/fork-secret-box.ts'), 'fork-cofre-seguro': resolve('src/preload/fork-cofre-seguro.ts') } } } },
  renderer: {
    define: releaseDefines,
    resolve: { alias: { '@shared': resolve('src/shared') } },
    plugins: [react(), {
      // MX-93: main.tsx carrega o app (inicio.tsx) só depois de a abertura pintar. Este preload no HTML baixa e
      // compila o pedaço em paralelo, para a espera ser só a do quadro, nunca a do download. (Pré-carregar também a
      // janela-app foi medido e piorou: compilar 2 MB a mais disputa a thread com o app.)
      name: 'mexerica-preload-inicio',
      transformIndexHtml: {
        order: 'post',
        handler(_html, ctx) {
          const inicio = Object.values(ctx.bundle ?? {}).find((c) => c.type === 'chunk' && c.isDynamicEntry && c.moduleIds.some((id) => id.endsWith('/src/renderer/inicio.tsx')))
          return inicio ? [{ tag: 'link', attrs: { rel: 'modulepreload', crossorigin: true, href: `./${inicio.fileName}` }, injectTo: 'head' }] : []
        },
      },
    }],
    // Readable function names in error reports; the files are local, so size doesn't matter.
    // Auditoria D249 (E2): o CSS das partes sob demanda continua num arquivo só, na ordem de sempre.
    build: { minify: false, cssCodeSplit: false },
  },
})
