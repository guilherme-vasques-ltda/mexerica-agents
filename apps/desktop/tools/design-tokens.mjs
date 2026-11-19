// D165: writes the design system's CSS from apps/desktop/design/tokens.json, the one source:
// the design dir's tokens.css and the app's src/renderer/styles/tokens.css. `--check` only compares.
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { designTokensCSS } from '../src/renderer/lib/design-tokens.ts'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..')
const source = resolve(root, 'apps/desktop/design/tokens.json')
const targets = [resolve(root, 'apps/desktop/design/tokens.css'), resolve(root, 'apps/desktop/src/renderer/styles/tokens.css')]
const css = designTokensCSS(JSON.parse(readFileSync(source, 'utf8')))
const check = process.argv.includes('--check')
let stale = false
for (const target of targets) {
  let current = null
  try { current = readFileSync(target, 'utf8') } catch {}
  if (current === css) continue
  if (check) { stale = true; console.error(`desatualizado: ${target}`) } else { writeFileSync(target, css); console.log(`escrito: ${target}`) }
}
if (stale) process.exit(1)
