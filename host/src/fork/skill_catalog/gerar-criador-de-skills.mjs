#!/usr/bin/env node
// Geração de texto passivo em tempo de desenvolvimento; não é executado pelo Hub.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const root = new URL('../../../../', import.meta.url)
const output = new URL('./criador-de-skills.json', import.meta.url)
const sources = [
  ['SKILL.md', 'docs/skills/criador-de-skills/SKILL.md'],
  ...['README', 'skills', 'mcp', 'plugins', 'seguranca', 'fontes'].map(name =>
    [`references/${name}.md`, `docs/guides/extensibilidade/${name}.md`]),
  ['ADAPTATION.md', 'docs/skills/criador-de-skills/ADAPTATION.md'],
  ['LICENSE', 'LICENSE'],
]
assert.ok(process.argv.slice(2).every(arg => arg === '--check'), 'Use somente --check ou nenhum argumento')
const assets = sources.map(([path, source]) => {
  const bytes = readFileSync(new URL(source, root))
  const content = bytes.toString('utf8')
  assert.deepEqual(Buffer.from(content, 'utf8'), bytes, `UTF-8 inválido: ${source}`)
  return { path, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'), mode: '100644', content }
})
const generated = JSON.stringify(assets, null, 2) + '\n'
if (process.argv.includes('--check')) {
  assert.ok(readFileSync(output, 'utf8') === generated,
    'Bundle desatualizado: rode node host/src/fork/skill_catalog/gerar-criador-de-skills.mjs e inclua o JSON no mesmo PR')
} else {
  writeFileSync(output, generated)
}
console.log(`criador-de-skills: ${assets.length} arquivos ${process.argv.includes('--check') ? 'conferidos' : 'gerados'} em ${fileURLToPath(output)}`)
