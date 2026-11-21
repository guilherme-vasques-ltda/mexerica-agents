// Build da web local do Mexerica Open: o mesmo renderer do Electron (src/renderer), com
// entrada própria em src/web que instala a ponte e o portão de acesso local. O Hub serve a
// pasta de saída (dist/web) na mesma origem (127.0.0.1), então não há Clerk, nuvem nem
// relays: __DONNA_DEV_CLOUD_ACCESS__ nasce desligado (fail-closed) e o guardião reprova
// qualquer URL externa de autenticação ou script externo que aparecer no bundle.
import { copyFileSync, mkdirSync } from 'node:fs'
import { resolve } from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig, type Plugin } from 'vite'
import { version } from './package.json'

// A CSP do index aponta ./assets/brand/favicons/...; os PNGs moram em src/renderer/assets.
// No Electron eles chegam por outro caminho; na web copiamos para dist/web/assets/...
const brandAssets: Plugin = {
  name: 'mexerica-web-brand-assets',
  apply: 'build',
  closeBundle() {
    const from = resolve('src/renderer/assets/brand/favicons')
    const to = resolve('dist/web/assets/brand/favicons')
    mkdirSync(to, { recursive: true })
    for (const file of ['favicon.ico', 'favicon-16.png', 'favicon-32.png', 'favicon-192.png', 'favicon-180.png']) {
      copyFileSync(resolve(from, file), resolve(to, file))
    }
  },
}

// Ícones oficiais do PWA (brand/assets/logo/05-icone-app/web-pwa), copiados sem retoque para
// dist/web/icones, como o manifest.webmanifest espera.
const pwaIcones: Plugin = {
  name: 'mexerica-web-pwa-icones',
  apply: 'build',
  closeBundle() {
    const from = resolve('../..', 'brand/assets/logo/05-icone-app/web-pwa')
    const to = resolve('dist/web/icones')
    mkdirSync(to, { recursive: true })
    for (const file of ['icone-web-192.png', 'icone-web-512.png', 'icone-maskable-512.png']) {
      copyFileSync(resolve(from, file), resolve(to, file))
    }
  },
}

// Guardião de segurança do build: reprova se qualquer chunk ou asset levar URL externa do
// fornecedor de autenticação (o app web local não tem conta em nuvem) ou se o HTML carregar
// script de fora (a CSP nega mesmo, mas o bundle nem deve conter).
const semExternosOuClerk: Plugin = {
  name: 'mexerica-web-sem-externos',
  renderChunk(code) {
    if (/https?:\/\/[A-Za-z0-9.-]*clerk[A-Za-z0-9.-]*/i.test(code)) throw new Error('Chunk contém URL externa de autenticação')
    return null
  },
  generateBundle(_options, bundle) {
    for (const asset of Object.values(bundle)) {
      if (asset.type !== 'asset') continue
      const fonte = typeof asset.source === 'string' ? asset.source : null
      if (fonte === null) continue
      if (/https?:\/\/[A-Za-z0-9.-]*clerk[A-Za-z0-9.-]*/i.test(fonte)) throw new Error(`Asset ${asset.fileName} contém URL externa de autenticação`)
      if (asset.fileName.endsWith('.html') && /<script\b[^>]*\bsrc\s*=\s*['"](?:https?:)?\/\//i.test(fonte)) throw new Error('HTML contém script externo')
    }
  },
}

export default defineConfig({
  root: resolve('src/web'),
  // src/web/public leva o manifest.webmanifest para a raiz da saída.
  publicDir: resolve('src/web/public'),
  base: '/',
  resolve: { alias: { '@shared': resolve('src/shared') } },
  plugins: [react(), brandAssets, pwaIcones, semExternosOuClerk],
  define: {
    __DONNA_DEV_CLOUD_ACCESS__: 'false',
    __APP_VERSION__: JSON.stringify(version),
  },
  // Igual ao Electron (electron.vite.config.ts): CSS num arquivo só. Com a divisão por chunk, o CSS dos hosts que
  // embrulham o shell (.rc-host, .times-host, .torre-host: height 100%) só chegava ao abrir Rotinas, Equipes ou o
  // Board, e o app inteiro ficava com a altura do conteúdo (~210 px).
  build: {
    outDir: resolve('dist/web'),
    emptyOutDir: true,
    cssCodeSplit: false,
    chunkSizeWarningLimit: 1500,
    rollupOptions: { input: { index: resolve('src/web/index.html') } },
  },
})
