import { test } from 'node:test'
import assert from 'node:assert/strict'
import { stepText } from './cards/step-text.ts'
import { CATALOGOS, tradutor } from '../../lib/fork-i18n/index.ts'
import { APP_LANGUAGES } from '../../../shared/fork-idioma.ts'

// MX-141: o título cru do motor (inglês) vira linguagem simples; o objeto (arquivo/comando) fica como veio.
const pt = tradutor('pt-BR')
const texto = (title: string, kind = 'other', t = pt) => stepText(t, { title, kind })

test('verbos conhecidos no início do título (sem diferenciar maiúsculas) viram português simples', () => {
  assert.equal(texto('Read src/pace.js', 'read'), 'Lendo src/pace.js')
  assert.equal(texto('read src/pace.js', 'read'), 'Lendo src/pace.js')
  assert.equal(texto('Edit src/app.ts', 'edit'), 'Alterando src/app.ts')
  assert.equal(texto('MultiEdit src/app.ts', 'edit'), 'Alterando src/app.ts')
  assert.equal(texto('Write src/novo.ts', 'edit'), 'Escrevendo src/novo.ts')
  assert.equal(texto('Delete src/velho.ts', 'delete'), 'Apagando src/velho.ts')
  assert.equal(texto('Move a.ts to b.ts', 'move'), 'Movendo a.ts to b.ts')
  assert.equal(texto('Run npm test', 'execute'), 'Rodando npm test')
  assert.equal(texto('Bash ls -la', 'execute'), 'Rodando ls -la')
  assert.equal(texto('grep pace src', 'search'), 'Procurando pace src')
  assert.equal(texto('Glob **/*.ts', 'search'), 'Procurando **/*.ts')
  assert.equal(texto('Find "pace" in src', 'search'), 'Procurando "pace" in src')
  assert.equal(texto('List src', 'search'), 'Listando src')
  assert.equal(texto('Fetch https://exemplo.com', 'fetch'), 'Buscando na web https://exemplo.com')
  assert.equal(texto('WebSearch pace clock', 'fetch'), 'Buscando na web pace clock')
  assert.equal(texto('Web Search pace clock', 'fetch'), 'Buscando na web pace clock')
  assert.equal(texto('Think', 'think'), 'Pensando')
  assert.equal(texto('Update todos', 'other'), 'Atualizando tarefas')
  assert.equal(texto('Update TODOs', 'other'), 'Atualizando tarefas')
})

test('verbo só reconhecido como palavra inteira; "Reader" e "Edition" não são verbos', () => {
  assert.equal(texto('Reader of things'), 'Reader of things')
  assert.equal(texto('Editor aberto'), 'Editor aberto')
  assert.equal(texto('Readme.md', 'read'), 'Lendo Readme.md')
})

test('crases em volta do objeto saem; o resto do título segue como veio', () => {
  assert.equal(texto('Edit `src/app.ts`', 'edit'), 'Alterando src/app.ts')
  assert.equal(texto('Read File', 'read'), 'Lendo')
  assert.equal(texto('Read', 'read'), 'Lendo')
})

test('só comando entre crases ou caminho: o verbo vem do tipo do passo', () => {
  assert.equal(texto('`npm test`', 'execute'), 'Rodando npm test')
  assert.equal(texto('`cat a.txt`', 'read'), 'Lendo cat a.txt')
  assert.equal(texto('`ls`', 'other'), 'Trabalhando ls')
  // MX-253 (revisão do #757): crases não provam que é comando; sem tipo, o verbo é o neutro.
  assert.equal(texto('`src/app.ts`', 'other'), 'Trabalhando src/app.ts')
  assert.equal(texto('`npm test`', 'other'), 'Trabalhando npm test')
  assert.equal(texto('npm test', 'execute'), 'Rodando npm test')
  assert.equal(texto('`npm test`', 'execute'), 'Rodando npm test')
  assert.equal(texto('Run npm test', 'other'), 'Rodando npm test')
  assert.equal(texto('Bash npm test', 'other'), 'Rodando npm test')
  assert.equal(texto('src/pace.js', 'read'), 'Lendo src/pace.js')
  assert.equal(texto('src/app.ts', 'edit'), 'Alterando src/app.ts')
  assert.equal(texto('docs/a.md', 'other'), 'Trabalhando docs/a.md')
  assert.equal(texto('npm test', 'execute'), 'Rodando npm test')
  assert.equal(texto('pace', 'search'), 'Procurando pace')
})

test('título desconhecido que já é frase (do Hub, no idioma) fica como está; nada é inventado', () => {
  assert.equal(texto('Leu 38 erros', 'read'), 'Leu 38 erros')
  assert.equal(texto('Perguntou à Sprint', 'other'), 'Perguntou à Sprint')
  assert.equal(texto('WhatsApp: ler conversa', 'other'), 'WhatsApp: ler conversa')
  assert.equal(texto('mcp__connectors__whatsapp_read_chat', 'other'), 'mcp__connectors__whatsapp_read_chat')
})

test('frase já no idioma nunca ganha o verbo de novo, nem em passo de comando (revisão do #747)', () => {
  assert.equal(texto('Rodando npm test', 'execute'), 'Rodando npm test')
  assert.equal(texto('Executando testes', 'execute'), 'Executando testes')
  assert.equal(texto('运行测试', 'execute'), '运行测试')
  assert.equal(texto('src/a.ts atualizado', 'read'), 'src/a.ts atualizado')
  assert.equal(texto('docs/a.md revisado', 'other'), 'docs/a.md revisado')
  assert.equal(texto('./run.sh --rapido', 'execute'), 'Rodando ./run.sh --rapido')
})

test('frase já no idioma, mesmo sem complemento ou em minúsculas, fica como veio (2ª revisão do #747)', () => {
  assert.equal(texto('Lendo', 'read'), 'Lendo')
  assert.equal(texto('Pensando', 'think'), 'Pensando')
  assert.equal(texto('rodando npm test', 'execute'), 'rodando npm test')
  assert.equal(texto('LENDO src/a.ts', 'read'), 'LENDO src/a.ts')
  assert.equal(texto('正在读取', 'read', tradutor('zh-CN')), '正在读取')
  assert.equal(texto('Reading', 'read', tradutor('en')), 'Reading')
  assert.equal(texto('Lendorama', 'read'), 'Lendo Lendorama')
})

test('"File/Files" sem caminho só some na leitura; nas outras ações o objeto fica', () => {
  assert.equal(texto('Read Files', 'read'), 'Lendo')
  assert.equal(texto('Run file', 'execute'), 'Rodando file')
  assert.equal(texto('Delete files', 'delete'), 'Apagando files')
})

test('nomes herdados de objeto ("constructor", "__proto__") não viram ação', () => {
  assert.equal(texto('constructor of things'), 'constructor of things')
  assert.equal(texto('toString src/a.ts'), 'toString src/a.ts')
  assert.equal(texto('x', 'constructor'), 'x')
  assert.equal(texto('__proto__', '__proto__'), '__proto__')
})

test('sem título (ou só "Tool"/"Terminal"): o nome do tipo, como antes', () => {
  assert.equal(texto('', 'execute'), 'Comando')
  assert.equal(texto('   ', 'edit'), 'Edição')
  assert.equal(texto('Tool', 'other'), 'Ferramenta')
  assert.equal(texto('Terminal', 'execute'), 'Comando')
})

test('em inglês, os mesmos passos leem inglês simples', () => {
  const en = tradutor('en')
  assert.equal(texto('Read src/pace.js', 'read', en), 'Reading src/pace.js')
  assert.equal(texto('Edit src/app.ts', 'edit', en), 'Changing src/app.ts')
  assert.equal(texto('`npm test`', 'execute', en), 'Running npm test')
})

test('em árabe, o caminho fica isolado (LTR) dentro do texto da direita para a esquerda', () => {
  const ar = tradutor('ar-AE')
  assert.equal(texto('Read src/pace.js', 'read', ar), '⁨جارٍ القراءة⁩: ⁨src/pace.js⁩')
})

test('os 6 idiomas têm todas as chaves dos passos em linguagem simples', () => {
  const chaves = Object.keys(CATALOGOS['pt-BR']).filter((k) => k.startsWith('conversa.cartoes.passos.acao.'))
  assert.equal(chaves.length, 14)
  for (const idioma of APP_LANGUAGES) {
    const catalogo = CATALOGOS[idioma] as Record<string, string>
    for (const chave of chaves) assert.ok(catalogo[chave], `${idioma} sem ${chave}`)
    assert.ok(catalogo['conversa.cartoes.passos.acao.comObjeto']!.includes('{acao}') && catalogo['conversa.cartoes.passos.acao.comObjeto']!.includes('{objeto}'), `${idioma}: modelo sem {acao}/{objeto}`)
    assert.equal(texto('Read src/pace.js', 'read', tradutor(idioma)).includes('src/pace.js'), true, `${idioma}: objeto some`)
  }
})
