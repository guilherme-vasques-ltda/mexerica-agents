import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, statSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { arquivoEscolhaEntrada, gravarEscolhaEntrada, lerEscolhaEntrada } from './fork-modo-entrada.ts'

test('a escolha da entrada é gravada uma vez em userData, só do dono, e só nos formatos aceitos', () => {
  const pasta = mkdtempSync(join(tmpdir(), 'escolha-entrada-'))
  try {
    const arquivo = arquivoEscolhaEntrada(join(pasta, 'userData'))
    assert.equal(lerEscolhaEntrada(arquivo), null)
    assert.equal(gravarEscolhaEntrada(arquivo, { modo: 'conta', infraestrutura: null }), false)
    assert.equal(gravarEscolhaEntrada(arquivo, { modo: 'conta', infraestrutura: 'nuvem' }), false)
    assert.equal(gravarEscolhaEntrada(arquivo, { modo: 'local', infraestrutura: null }), true)
    assert.deepEqual(lerEscolhaEntrada(arquivo), { modo: 'local', infraestrutura: null })
    if (process.platform !== 'win32') assert.equal(statSync(arquivo).mode & 0o777, 0o600)
    assert.equal(gravarEscolhaEntrada(arquivo, { modo: 'conta', infraestrutura: 'propria' }), true)
    assert.deepEqual(JSON.parse(readFileSync(arquivo, 'utf8')), { modo: 'conta', infraestrutura: 'propria' })

    writeFileSync(arquivo, '{"modo":"nuvem"}')
    assert.equal(lerEscolhaEntrada(arquivo), null)

    if (process.platform !== 'win32') {
      const alvo = join(pasta, 'alvo.json')
      writeFileSync(alvo, '{"modo":"local","infraestrutura":null}')
      const link = join(pasta, 'link.json')
      symlinkSync(alvo, link)
      assert.equal(lerEscolhaEntrada(link), null)
      assert.equal(gravarEscolhaEntrada(link, { modo: 'local', infraestrutura: null }), false)
    }
  } finally {
    rmSync(pasta, { recursive: true, force: true })
  }
})
