import { resolve } from 'node:path'
import { defineConfig, loadEnv } from 'vite'

export default defineConfig(({ mode }) => {
  const cwd = process.cwd()
  const name = 'WEB_REMOTE_CLERK_TEST_PUBLISHABLE_KEY'
  const key = loadEnv(mode, cwd, name)[name] ?? ''
  const testKey = /^pk_test_([A-Za-z0-9+/]+={0,2})$/.exec(key)
  const host = testKey ? Buffer.from(testKey[1], 'base64').toString('ascii') : ''
  const frontend = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.clerk\.accounts\.dev\$$/.test(host)
    ? ` https://${host.slice(0, -1)}` : ''
  if (key && !frontend) throw new Error(`Invalid development publishable key in ${name}`)
  const csp = [
    "default-src 'self'", `connect-src 'self'${frontend}`, "img-src 'self' data:",
    "object-src 'none'", "base-uri 'none'", "form-action 'none'",
  ].join('; ')
  return {
    root: resolve(cwd, 'src/web/remote'),
    publicDir: false,
    resolve: { alias: { '@shared': resolve(cwd, 'src/shared') } },
    define: { __REMOTE_CLERK_TEST_KEY__: JSON.stringify(key) },
    plugins: [{
      name: 'mexerica-web-remote-csp',
      transformIndexHtml: (html) => html.replace('__REMOTE_WEB_CSP__', csp),
    }],
    build: { outDir: resolve(cwd, 'dist/web-remote'), emptyOutDir: true, target: 'esnext' },
  }
})
