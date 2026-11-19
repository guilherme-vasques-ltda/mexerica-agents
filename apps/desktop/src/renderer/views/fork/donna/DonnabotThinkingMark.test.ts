import assert from 'node:assert/strict'
import test from 'node:test'
import { activityIn } from './DonnabotThinkingMark.ts'

test('a frase fixa do Hub traduz só quando é a string inteira da atividade', () => {
  const esperado = {
    'pt-BR': ['Pensando…', 'Escrevendo…', 'Iniciando…'],
    en: ['Thinking…', 'Writing…', 'Starting…'],
    es: ['Pensando…', 'Escribiendo…', 'Iniciando…'],
    fr: ['Réflexion…', 'Rédaction…', 'Démarrage…'],
    'zh-CN': ['思考中…', '正在撰写…', '正在启动…'],
    'ar-AE': ['يفكر…', 'يكتب…', 'جارٍ البدء…'],
  } as const
  for (const [idioma, [pensando, escrevendo, iniciando]] of Object.entries(esperado) as [keyof typeof esperado, readonly string[]][]) {
    assert.equal(activityIn(idioma, 'Thinking…'), pensando)
    assert.equal(activityIn(idioma, 'Writing a reply…'), escrevendo)
    assert.equal(activityIn(idioma, 'Typing…'), escrevendo)
    assert.equal(activityIn(idioma, 'Starting agent…'), iniciando)
    // Com prefixo (membro de canal ou qualquer outro), nada é classificado: fica como veio.
    assert.equal(activityIn(idioma, 'Berga: Thinking…'), 'Berga: Thinking…')
    assert.equal(activityIn(idioma, 'Berga: Typing…'), 'Berga: Typing…')
    assert.equal(activityIn(idioma, 'Needs approval: Edit src/pace.js'), 'Needs approval: Edit src/pace.js')
    assert.equal(activityIn(idioma, 'Equipe: Azul: Needs approval: Run migration…'), 'Equipe: Azul: Needs approval: Run migration…')
    assert.equal(activityIn(idioma, 'Thinking… about it'), 'Thinking… about it')
    assert.equal(activityIn(idioma, 'Read src/pace.js'), 'Read src/pace.js')
  }
  assert.equal(activityIn('pt-BR', ''), 'Trabalhando…')
})
