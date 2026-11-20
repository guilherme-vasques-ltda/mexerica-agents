import assert from 'node:assert/strict'
import test from 'node:test'
import { aceleradorDoEvento, criarGuardaPressao, decidirEntradaVoz, rotuloAtalho, validarAtalho, type ContextoEntradaVoz } from './fork-voz-entrada.ts'

const pronto: ContextoEntradaVoz = { conversaVisivel: true, telaGrande: false, grupo: false, sessaoAberta: false, disponivel: true, senhaBloqueada: false, emChamada: false }

test('o atalho inicia a chamada na conversa principal à vista', () => {
  assert.deepEqual(decidirEntradaVoz(pronto), { acao: 'iniciar' })
})

test('já em chamada o atalho encerra, e nunca abre uma segunda sessão', () => {
  assert.deepEqual(decidirEntradaVoz({ ...pronto, emChamada: true }), { acao: 'encerrar' })
})

test('encerrar nunca é recusado, nem com a conversa fora de vista', () => {
  assert.deepEqual(decidirEntradaVoz({ ...pronto, emChamada: true, conversaVisivel: false, disponivel: false }), { acao: 'encerrar' })
})

test('chat errado, conversa escondida (inclui a tela grande do WhatsApp), grupo, aba de sessão, senha colada e voz indisponível são recusados com motivo', () => {
  const caso = (parcial: Partial<ContextoEntradaVoz>) => decidirEntradaVoz({ ...pronto, ...parcial })
  assert.deepEqual(caso({ conversaVisivel: false }), { acao: 'recusar', motivo: 'oculta' })
  assert.deepEqual(caso({ telaGrande: true }), { acao: 'recusar', motivo: 'oculta' })
  assert.deepEqual(caso({ grupo: true }), { acao: 'recusar', motivo: 'grupo' })
  assert.deepEqual(caso({ sessaoAberta: true }), { acao: 'recusar', motivo: 'sessao' })
  assert.deepEqual(caso({ senhaBloqueada: true }), { acao: 'recusar', motivo: 'senha' })
  assert.deepEqual(caso({ disponivel: false }), { acao: 'recusar', motivo: 'indisponivel' })
})

test('atalho: normaliza, exige dois modificadores (ou um com F), recusa reservados e lixo', () => {
  assert.deepEqual(validarAtalho('cmdorctrl+shift+l', 'darwin'), { ok: true, acelerador: 'CommandOrControl+Shift+L' })
  assert.deepEqual(validarAtalho('Shift+Alt+V', 'linux'), { ok: true, acelerador: 'Alt+Shift+V' })
  assert.deepEqual(validarAtalho('Control+F9', 'win32'), { ok: true, acelerador: 'Control+F9' })
  for (const ruim of ['K', 'Shift+K', 'CommandOrControl+K', 'Alt+Y', 'Alt+K', 'Control+Space', 'CommandOrControl+Shift+Enter', 'CommandOrControl+Shift', 'Foo+K', 'CommandOrControl+CmdOrCtrl+K', 'Shift+F5', '', 'CommandOrControl+Shift+F25', 'Command+CommandOrControl+L', 'Shift+Shift+L', 42, null]) {
    assert.deepEqual(validarAtalho(ruim, 'darwin'), { ok: false, motivo: 'invalido' }, String(ruim))
  }
  assert.deepEqual(validarAtalho('CommandOrControl+Shift+Z', 'win32'), { ok: false, motivo: 'reservado' })
  assert.deepEqual(validarAtalho('Command+Shift+Z', 'darwin'), { ok: false, motivo: 'reservado' })
  assert.deepEqual(validarAtalho('Control+Command+S', 'darwin'), { ok: false, motivo: 'reservado' })
  assert.deepEqual(validarAtalho('Shift+Command+4', 'darwin'), { ok: false, motivo: 'reservado' })
  assert.deepEqual(validarAtalho('CommandOrControl+Shift+K', 'darwin'), { ok: false, motivo: 'reservado' }, 'seletor de Spaces')
  assert.deepEqual(validarAtalho('Control+Shift+K', 'linux'), { ok: false, motivo: 'reservado' }, 'seletor de Spaces')
  assert.deepEqual(validarAtalho('Command+Shift+Z', 'win32'), { ok: true, acelerador: 'Command+Shift+Z' })
})

test('pressões repetidas: segurar a tecla ou apertar duas vezes seguidas conta uma só', () => {
  let agora = 1000
  const aceita = criarGuardaPressao(() => agora, 1200)
  assert.equal(aceita(), true)
  for (let i = 0; i < 20; i++) { agora += 33; assert.equal(aceita(), false, 'repetição do sistema') }
  agora += 1100
  assert.equal(aceita(), false, 'a primeira repetição do sistema, após o atraso, também é recusada e renova a janela')
  agora += 1200
  assert.equal(aceita(), true, 'um novo toque depois da pausa vale')
})

test('gravar a combinação: usa a tecla física, ignora só-modificador e Super fora do Mac', () => {
  const tecla = (code: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }> = {}) => ({ code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods })
  assert.equal(aceleradorDoEvento(tecla('KeyL', { metaKey: true, shiftKey: true }), 'darwin'), 'Command+Shift+L')
  assert.equal(aceleradorDoEvento(tecla('Digit7', { ctrlKey: true, altKey: true }), 'win32'), 'Control+Alt+7')
  assert.equal(aceleradorDoEvento(tecla('F9', { ctrlKey: true }), 'linux'), 'Control+F9')
  assert.equal(aceleradorDoEvento(tecla('ShiftLeft', { shiftKey: true }), 'darwin'), null)
  assert.equal(aceleradorDoEvento(tecla('KeyK', { metaKey: true }), 'linux'), null)
  assert.deepEqual(validarAtalho(aceleradorDoEvento(tecla('KeyL', { metaKey: true, shiftKey: true }), 'darwin'), 'darwin'), { ok: true, acelerador: 'Command+Shift+L' })
})

test('gravar a combinação: a tecla impressa vale (AZERTY), e a posição física só quando a impressa não serve', () => {
  const ev = { code: 'KeyQ', key: 'a', metaKey: false, ctrlKey: true, altKey: false, shiftKey: true }
  assert.equal(aceleradorDoEvento(ev, 'win32'), 'Control+Shift+A')
  assert.equal(aceleradorDoEvento({ ...ev, key: 'Shift', code: 'ShiftLeft' }, 'win32'), null)
  assert.equal(aceleradorDoEvento({ ...ev, key: '˙', code: 'KeyH', metaKey: true, ctrlKey: false, altKey: true }, 'darwin'), 'Command+Alt+Shift+H')
})

test('nomes herdados de Object não valem como modificador', () => {
  assert.deepEqual(validarAtalho('constructor+Command+Shift+Z', 'darwin'), { ok: false, motivo: 'invalido' })
})

test('rótulo legível por plataforma', () => {
  assert.equal(rotuloAtalho('CommandOrControl+Shift+K', 'darwin'), '⇧⌘K')
  assert.equal(rotuloAtalho('Control+Alt+K', 'darwin'), '⌃⌥K')
  assert.equal(rotuloAtalho('CommandOrControl+Shift+K', 'win32'), 'Ctrl+Shift+K')
})
