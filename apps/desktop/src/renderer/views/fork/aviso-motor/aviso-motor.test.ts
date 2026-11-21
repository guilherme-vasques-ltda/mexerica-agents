import assert from 'node:assert/strict'
import test from 'node:test'
import { CATALOGOS, tradutor } from '../../../lib/fork-i18n/index.ts'
import { APP_LANGUAGES } from '../../../../shared/fork-idioma.ts'
import { ageSemAprovacao, avisoDoMotor, rotuloDeMotor, textosSemAprovacao } from './aviso-motor.ts'

test('a OpenCode mostra o aviso da D224', () => {
  const chave = avisoDoMotor('opencode')
  assert.ok(chave)
  assert.match(tradutor('pt-BR')(chave), /não cobre tudo/)
  assert.match(tradutor('en')(chave), /doesn't cover everything/)
})

test('os demais motores seguem sem aviso', () => {
  for (const backend of ['codex', 'claude', 'custom', '', 'toString', '__proto__']) assert.equal(avisoDoMotor(backend), null)
})

test('só o motor que o Hub marcou com actsWithoutApproval verdadeiro mostra o aviso de "age sem pedir"', () => {
  assert.equal(ageSemAprovacao({ actsWithoutApproval: true }), true)
  assert.equal(ageSemAprovacao({ actsWithoutApproval: false }), false)
  assert.equal(ageSemAprovacao({}), false, 'sem o campo (Hub antigo), nada a avisar')
  assert.equal(ageSemAprovacao(undefined), false)
  assert.equal(ageSemAprovacao({ actsWithoutApproval: 'true' as unknown as boolean }), false, 'só o booleano verdadeiro vale')
})

test('o selo e a explicação existem nos seis idiomas e dizem o que o motor faz', () => {
  const pt = textosSemAprovacao('pt-BR')
  assert.equal(pt.selo, 'Age sem pedir')
  assert.match(pt.texto, /sem pedir sua aprovação/)
  for (const idioma of APP_LANGUAGES) {
    const catalogo = CATALOGOS[idioma] as Record<string, string | undefined>
    assert.ok(catalogo['conversa.avisos.motor.semAprovacaoSelo'], `selo em ${idioma}`)
    assert.ok(catalogo['conversa.avisos.motor.semAprovacao'], `explicação em ${idioma}`)
  }
  assert.notEqual(textosSemAprovacao('en').texto, pt.texto)
})

test('o rótulo do motor na lista de opções soma "não instalado" e o selo, sem esconder o nome', () => {
  assert.equal(rotuloDeMotor({ name: 'Codex', available: true }, 'pt-BR'), 'Codex')
  assert.equal(rotuloDeMotor({ name: 'Codex', available: false }, 'pt-BR'), 'Codex (não instalado)')
  assert.equal(rotuloDeMotor({ name: 'OpenCode', available: true, actsWithoutApproval: true }, 'pt-BR'), 'OpenCode (Age sem pedir)')
  assert.equal(rotuloDeMotor({ name: 'OpenCode', available: false, actsWithoutApproval: true }, 'en'), 'OpenCode (not installed, Acts without asking)')
})
