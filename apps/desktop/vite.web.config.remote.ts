import { resolve } from 'node:path'
import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'
import { version } from './package.json'

export default defineConfig(({ mode }) => {
  const cwd = process.cwd()
  const name = 'WEB_REMOTE_CLERK_TEST_PUBLISHABLE_KEY'
  const key = loadEnv(mode, cwd, name)[name] ?? ''
  const testKey = /^pk_test_([A-Za-z0-9+/]+={0,2})$/.exec(key)
  const host = testKey ? Buffer.from(testKey[1], 'base64').toString('ascii') : ''
  const frontend = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.clerk\.accounts\.dev\$$/.test(host)
    ? ` https://${host.slice(0, -1)}` : ''
  if (key && !frontend) throw new Error(`Invalid development publishable key in ${name}`)
  const bindingName = 'WEB_REMOTE_HUBS'
  const raw = loadEnv(mode, cwd, bindingName)[bindingName]
  const hubs: { workspaceId: string; hubId: string; origin: string; signKey: string }[] = raw ? JSON.parse(raw) : []
  if (!Array.isArray(hubs) || hubs.some((hub) => {
    if (!hub || typeof hub.workspaceId !== 'string' || !/^(?:org|personal):[\w-]+$/.test(hub.workspaceId)
      || typeof hub.hubId !== 'string' || !/^[\w-]{1,128}$/.test(hub.hubId)
      || typeof hub.signKey !== 'string' || !/^[A-Za-z0-9_-]{43}$/.test(hub.signKey)) return true
    const url = new URL(hub.origin)
    return url.protocol !== 'wss:' || url.href !== `${url.origin}/` || typeof hub.origin !== 'string'
  }) || new Set(hubs.map((hub) => hub.workspaceId)).size !== hubs.length) throw new Error(`Invalid ${bindingName}`)
  const configured = !!key && hubs.length > 0
  const origins = configured ? [...new Set(hubs.map(({ origin }) => new URL(origin).origin))] : []
  const csp = [
    "default-src 'self'", `connect-src 'self'${configured ? frontend : ''}${origins.map((origin) => ` ${origin}`).join('')}`,
    "img-src 'self' data: blob:",
    "object-src 'none'", "base-uri 'none'", "form-action 'none'",
  ].join('; ')
  return {
    root: resolve(cwd, 'src/web/remote'),
    publicDir: false,
    resolve: { alias: { '@shared': resolve(cwd, 'src/shared') } },
    define: {
      __REMOTE_CLERK_TEST_KEY__: JSON.stringify(configured ? key : ''),
      __REMOTE_HUBS__: JSON.stringify(configured ? hubs.map(({ workspaceId, hubId, origin, signKey }) =>
        ({ workspaceId, hubId, origin, signKey })) : []),
      __DONNA_DEV_CLOUD_ACCESS__: 'false',
      __APP_VERSION__: JSON.stringify(version),
    },
    plugins: [react(), {
      name: 'mexerica-web-remote-csp',
      transformIndexHtml: (html) => html.replace('__REMOTE_WEB_CSP__', csp),
    }],
    build: {
      outDir: resolve(cwd, 'dist/web-remote'), emptyOutDir: true, target: 'esnext',
      rollupOptions: { input: {
        index: resolve(cwd, 'src/web/remote/index.html'),
        ...(configured ? { renderer: resolve(cwd, 'src/web/remote/renderer.html') } : {}),
      } },
    },
  }
})
