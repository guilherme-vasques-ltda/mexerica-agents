import test from 'node:test'
import assert from 'node:assert/strict'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import { ContaConfirmadaTexto, BotCriadoNaOrigemTexto } from '../../criar-bot/ContaConfirmada.ts'

for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
  test(`D543: nomes de conta, motor e Hub são bdi em ${idioma}`, () => {
    const html = renderToStaticMarkup(createElement(ContaConfirmadaTexto, {
      t: tradutor(idioma), conta: 'حساب <nome>', motor: 'Codex', hub: 'Hub עברית',
    }))
    assert.equal((html.match(/<bdi dir="auto">/g) ?? []).length, 3)
    assert.ok(html.includes('<bdi dir="auto">حساب &lt;nome&gt;</bdi>'))
    assert.ok(html.includes('<bdi dir="auto">Hub עברית</bdi>'))
    assert.ok(html.includes('<bdi dir="auto">Codex</bdi>'))
    const receipt = renderToStaticMarkup(createElement(BotCriadoNaOrigemTexto, {
      t: tradutor(idioma), nome: 'بوت', space: 'Space עברית', hub: 'Hub A',
    }))
    assert.equal((receipt.match(/<bdi dir="auto">/g) ?? []).length, 3)
    assert.ok(receipt.includes('<bdi dir="auto">Space עברית</bdi>'))
  })
}
