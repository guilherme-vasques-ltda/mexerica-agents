// DON-917 (fatia 9b, D70): o nome do Hub é texto livre e a hora é dado; em árabe, nenhum dos dois troca a ordem da frase.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { connectionLine, deadlineText, statusLine } from './cloud-copy.ts'

const FSI = '⁨'
const LRI = '⁦'
const PDI = '⁩'

test('o nome do Hub entra isolado e a frase segue o idioma e o aparelho', () => {
  const ar = statusLine('accountMismatch', 'خادم', 'connect', 'mac', 'ar-AE')!
  assert.ok(ar.text.includes(`${FSI}خادم${PDI}`))
  const en = statusLine('handshakeRefused', 'Studio', 'connect', 'pc', 'en')!
  assert.match(en.text, /refused this computer/)
  assert.ok(en.text.includes(`${FSI}Studio${PDI}`))
  assert.ok(connectionLine('hubOffline', 'Studio', false, false, 'fr').text.includes(`${FSI}Studio${PDI}`))
})

test('o prazo é a hora isolada da esquerda para a direita', () => {
  const texto = deadlineText(Date.UTC(2026, 10, 8, 14, 5), 'ar-AE')!
  assert.match(texto, new RegExp(`${LRI}[^${PDI}]+${PDI}`))
  assert.equal(deadlineText(undefined, 'en'), null)
})

test('R1-01 do #596: a tela só passa a connect o nome real (diretório ou nameOf), nunca um texto do catálogo', async () => {
  const { readFileSync } = await import('node:fs')
  const fonte = readFileSync(new URL('./CloudLinkSection.tsx', import.meta.url), 'utf8')
  const chamadas = [...fonte.matchAll(/cloud\.connect\(([^,]+),\s*([^)]+\)?)\)/g)].map((m) => m[2]!.trim())
  assert.ok(chamadas.length >= 3)
  for (const nome of chamadas) assert.match(nome, /^(computer\.name|cloud\.nameOf\([^)]*\))$/, nome)
})
