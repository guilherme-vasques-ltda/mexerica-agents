import assert from 'node:assert/strict'
import test from 'node:test'
import { cofreCanais, codigoValido, ENTRY_LOGIN_NOVO, lerAbrir, lerAbrirNovo, lerEnvio, soDigitos } from './fork-cofre-seguro.ts'

const SENHA = 'senha-ficticia-do-teste'
const base = { computerId: 'pc-1', entryId: 'e-1' }

test('o que o chat manda ao main nunca leva valor nem texto do pedido: só a referência', () => {
  const lido = lerAbrir({ ...base, valor: SENHA, value: SENHA, password: SENHA,
    pedido: { tipo: 'login', titulo: 'site-falso.com', botNome: 'Outro', valor: SENHA }, spaceId: 'sp-1', conversationId: 'conv-1' })
  assert.ok(lido)
  assert.equal(JSON.stringify(lido).includes(SENHA), false)
  assert.equal(JSON.stringify(lido).includes('site-falso'), false)
  assert.deepEqual(Object.keys(lido).sort(), ['computerId', 'conversationId', 'entryId', 'spaceId'])
})

test('lerAbrir recusa o que não é uma referência bem formada', () => {
  for (const ruim of [null, 'x', 42, {}, { ...base, entryId: '' }, { ...base, computerId: 'a'.repeat(200) },
    { ...base, spaceId: 42 }, { ...base, hubId: {} }, { ...base, conversationId: null }]) {
    // conversationId: null é tratado como ausente; os demais inválidos recusam.
    if (ruim && typeof ruim === 'object' && 'conversationId' in ruim && ruim.conversationId === null) {
      assert.ok(lerAbrir(ruim))
      continue
    }
    assert.equal(lerAbrir(ruim), null)
  }
})

test('lerEnvio: só confere tamanho e tipo; recusa vazio, grande e não texto', () => {
  assert.deepEqual(lerEnvio({ valor: SENHA, usuario: 'ana@exemplo.com', rotulo: 'x' }, 'login'), { valor: SENHA, usuario: 'ana@exemplo.com', rotulo: 'x' })
  assert.equal(lerEnvio({ valor: '' }, 'login'), null)
  assert.equal(lerEnvio({ valor: 'x'.repeat(4097) }, 'login'), null)
  assert.notEqual(lerEnvio({ valor: 'x'.repeat(4097) }, 'secret'), null)
  assert.equal(lerEnvio({ valor: 42 }, 'secret'), null)
  assert.equal(lerEnvio('texto', 'secret'), null)
})

test('nenhum canal do chat carrega o valor: o único canal com valor é o da janela segura', () => {
  assert.deepEqual(Object.values(cofreCanais).sort(), [
    'fork-cofre-seguro:abrir', 'fork-cofre-seguro:abrir-novo', 'fork-cofre-seguro:cancelar', 'fork-cofre-seguro:dados', 'fork-cofre-seguro:estado', 'fork-cofre-seguro:salvar',
  ])
})

test('preload geral: canais do chat escritos à mão e iguais aos do contrato; sem import do shared (evita chunk que quebra o preload com sandbox)', async () => {
  const { readFileSync } = await import('node:fs')
  const fonte = readFileSync(new URL('../preload/index.ts', import.meta.url), 'utf8')
  assert.equal(/from ['"]\.\.\/shared\/fork-cofre-seguro/.test(fonte), false)
  assert.ok(fonte.includes(`ipcRenderer.invoke('${cofreCanais.abrir}'`))
  assert.ok(fonte.includes(`ipcRenderer.invoke('${cofreCanais.abrirNovo}'`))
  assert.ok(fonte.includes(`listen('${cofreCanais.estado}'`))
  // O preload da janela segura também não pode importar o módulo em tempo de execução (só tipo, que some no build).
  const janela = readFileSync(new URL('../preload/fork-cofre-seguro.ts', import.meta.url), 'utf8')
  assert.equal(/import\s*\{[^}]*\}\s*from ['"]\.\.\/shared\/fork-cofre-seguro/.test(janela), false)
  for (const canal of [cofreCanais.dados, cofreCanais.salvar, cofreCanais.cancelar]) {
    assert.ok(janela.includes(`'${canal}'`), canal)
  }
})

test('MX-277: abrir a janela do login novo leva SÓ o computador; qualquer outro campo (valor, site, texto) recusa a chamada', () => {
  assert.deepEqual(lerAbrirNovo({ computerId: 'pc-1' }), { computerId: 'pc-1' })
  for (const ruim of [null, 'x', 42, [], {}, { computerId: '' }, { computerId: 'a'.repeat(200) }, { computerId: 7 },
    { computerId: 'pc-1', valor: SENHA }, { computerId: 'pc-1', password: SENHA }, { computerId: 'pc-1', site: 'exemplo.com' },
    { computerId: 'pc-1', entryId: 'e-1' }, { valor: SENHA }]) {
    assert.equal(lerAbrirNovo(ruim), null, JSON.stringify(ruim))
  }
  assert.equal(ENTRY_LOGIN_NOVO, 'login-novo')
})

test('MX-277: o envio do login novo exige site, usuário opcional, senha dentro do limite; as outras janelas não pedem site', () => {
  assert.deepEqual(lerEnvio({ valor: SENHA, usuario: 'ana@exemplo.com', rotulo: 'x', site: ' exemplo.com ' }, 'loginNovo'),
    { valor: SENHA, usuario: 'ana@exemplo.com', rotulo: 'x', site: 'exemplo.com' })
  for (const ruim of [{ valor: SENHA }, { valor: SENHA, site: '' }, { valor: SENHA, site: '   ' }, { valor: SENHA, site: 42 },
    { valor: SENHA, site: 'a'.repeat(254) }, { valor: '', site: 'exemplo.com' }, { valor: 'x'.repeat(4097), site: 'exemplo.com' }]) {
    assert.equal(lerEnvio(ruim, 'loginNovo'), null, JSON.stringify(ruim))
  }
  // Login e chave continuam como antes: nenhum `site` é devolvido.
  assert.deepEqual(lerEnvio({ valor: SENHA, site: 'exemplo.com' }, 'login'), { valor: SENHA, usuario: '', rotulo: '' })
})

test('MX-286: código de login — só dígitos, de 4 a 10, como o Hub; o resto nem chega ao Hub', () => {
  for (const bom of ['1234', '123456', '1234567890', ' 123456 ']) {
    assert.deepEqual(lerEnvio({ valor: bom, usuario: 'x', rotulo: 'y', site: 'z.com' }, 'code'), { valor: bom.trim(), usuario: '', rotulo: '' }, bom)
  }
  for (const ruim of ['', '   ', '123', '12345678901', '12 34', '12-34', 'abcdef', '12a456', '１２３４５６', '12.456', '1e5555', '+123456']) {
    assert.equal(lerEnvio({ valor: ruim }, 'code'), null, JSON.stringify(ruim))
  }
  for (const ruim of [null, 'x', 42, {}, { valor: 123456 }, { valor: ['123456'] }, { valor: null }]) assert.equal(lerEnvio(ruim, 'code'), null)
  assert.equal(codigoValido('123456'), true)
  assert.equal(codigoValido('123'), false)
  assert.equal(codigoValido('12345678901'), false)
  assert.equal(codigoValido('12 3456'), false)
})

test('MX-286: soDigitos — colar o código inteiro (com espaço, traço ou texto) vira só os dígitos, no máximo 10', () => {
  assert.equal(soDigitos('123 456'), '123456')
  assert.equal(soDigitos('123-456'), '123456')
  assert.equal(soDigitos('Seu código: 482913.'), '482913')
  assert.equal(soDigitos('abc'), '')
  assert.equal(soDigitos('12345678901234'), '1234567890')
  assert.equal(soDigitos('１２３'), '', 'dígitos de largura total não valem')
})
