// MX-245 (achado da revisão): o chip de reação de bots mostra no máximo 3 avatares; o `title` é a única forma de ver
// quem reagiu além desses 3 (hover do mouse). Uma correção anterior tirou o `title` para não duplicar a leitura do
// leitor de tela — e tirou junto a única pista visual de quem reagiu. Fonte, não render: só confere que a linha
// continua com os dois atributos.
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const fonte = readFileSync(fileURLToPath(new URL('./ChatRows.tsx', import.meta.url)), 'utf8')

test('chip de reação de bots: title (hover, quem reagiu) e aria-label convivem; a duplicidade de leitura fica de acompanhamento', () => {
  assert.match(fonte, /className="reaction-chip" title=\{label\} aria-label=\{label\} role="img"/)
})
