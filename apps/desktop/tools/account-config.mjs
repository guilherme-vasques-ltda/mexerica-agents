// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
// Writes resources/account-config.json from apps/desktop/config/<env>.plist: the Clerk publishable
// key and the cloud URL the build talks to. Usage: node tools/account-config.mjs dev|main
// (the `npm run dev` / `dist:*:<env>` scripts run it).
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const env = process.argv[2]
if (!['dev', 'main'].includes(env)) throw new Error(`unknown environment ${env}`)
const root = join(dirname(fileURLToPath(import.meta.url)), '..')
if (process.argv[3] === '--official') {
  if (env !== 'main' || process.argv.length !== 4) throw new Error('Official account configuration requires main.')
  const { officialAccountPlist } = await import('../src/shared/fork-official-account.ts')
  const output = join(root, 'build/official/account-config.json')
  rmSync(output, { force: true })
  let config
  try { config = officialAccountPlist(readFileSync(join(root, 'config/main.plist'), 'utf8')) } catch {}
  if (!config) throw new Error('Official account configuration is missing or invalid (config/main.plist).')
  mkdirSync(dirname(output), { recursive: true })
  writeFileSync(output, `${JSON.stringify(config, null, 2)}\n`, { flag: 'wx' })
  process.exit(0)
}
if (process.argv.length !== 3) throw new Error('Unexpected account configuration arguments.')
const plist = readFileSync(join(root, 'config', `${env}.plist`), 'utf8')
// An empty value (Donnabot: no cloud or sign-in yet) stays null, so the app fails closed.
const value = (key) => new RegExp(`<key>${key}</key>\\s*<string>([^<]*)</string>`).exec(plist)?.[1]?.trim() || null
// Public, release-qualified M only. Missing M leaves ACS4 unavailable; no env/health fallback.
let deploymentManifest = null
try { deploymentManifest = JSON.parse(readFileSync(join(root, 'config', `${env}.deployment.json`), 'utf8')) }
catch (error) { if (error.code !== 'ENOENT') throw error }
writeFileSync(
  join(root, 'resources/account-config.json'),
  `${JSON.stringify({ environment: env, clerkPublishableKey: value('clerkPublishableKey'), cloudURL: value('cloudURL'), clerkFrontendOrigin: value('clerkFrontendOrigin'), clerkProxyOrigin: value('clerkProxyOrigin'), clerkTokenOrigin: value('clerkTokenOrigin'), clerkIssuer: value('clerkIssuer'), deploymentId: value('deploymentId'), deploymentEnv: value('deploymentEnv'), manifestHash: value('manifestHash'), deploymentManifest }, null, 2)}\n`,
)
console.log(`account config: ${env}`)
