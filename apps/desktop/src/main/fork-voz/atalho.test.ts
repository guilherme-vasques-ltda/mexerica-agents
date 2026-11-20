import assert from 'node:assert/strict'
import test from 'node:test'
import { criarAtalhoVoz, type AtalhoGlobal } from './atalho.ts'

// globalShortcut FALSO: só o registro em memória. Não prova o comportamento do sistema (macOS, Windows, X11, Wayland).
function montar(ocupados: string[] = []) {
  const registrados = new Map<string, () => void>()
  const global: AtalhoGlobal = {
    register: (a, cb) => { if (ocupados.includes(a)) return false; registrados.set(a, cb); return true },
    unregister: (a) => void registrados.delete(a),
    isRegistered: (a) => registrados.has(a) || ocupados.includes(a),
  }
  const enviados: unknown[] = []
  let agora = 10_000
  const atalho = criarAtalhoVoz({ global, plataforma: 'darwin', enviar: (c) => enviados.push(c), agora: () => agora })
  return { atalho, registrados, enviados, passa: (ms: number) => { agora += ms } }
}

test('ligar registra a combinação normalizada e apertar manda um só comando voiceToggle ao chat', () => {
  const t = montar()
  assert.deepEqual(t.atalho.definir({ ativo: true, acelerador: 'cmdorctrl+shift+l' }), { ok: true, acelerador: 'CommandOrControl+Shift+L' })
  t.registrados.get('CommandOrControl+Shift+L')!()
  assert.deepEqual(t.enviados, [{ kind: 'voiceToggle' }])
})

test('repetição do sistema e toques seguidos viram um só comando; depois da pausa vale de novo', () => {
  const t = montar()
  t.atalho.definir({ ativo: true, acelerador: 'CommandOrControl+Shift+L' })
  const apertar = t.registrados.get('CommandOrControl+Shift+L')!
  apertar()
  for (let i = 0; i < 30; i++) { t.passa(30); apertar() }
  assert.equal(t.enviados.length, 1)
  t.passa(1300)
  apertar()
  assert.equal(t.enviados.length, 2)
})

test('desligar ou trocar a combinação solta a anterior: nada fica registrado escutando teclas', () => {
  const t = montar()
  t.atalho.definir({ ativo: true, acelerador: 'CommandOrControl+Shift+L' })
  t.atalho.definir({ ativo: true, acelerador: 'CommandOrControl+Shift+J' })
  assert.deepEqual([...t.registrados.keys()], ['CommandOrControl+Shift+J'])
  assert.deepEqual(t.atalho.definir({ ativo: false, acelerador: 'CommandOrControl+Shift+J' }), { ok: true, acelerador: null })
  assert.equal(t.registrados.size, 0)
  t.atalho.definir({ ativo: true, acelerador: 'CommandOrControl+Shift+L' })
  t.atalho.liberar()
  assert.equal(t.registrados.size, 0)
})

test('conflitos e entradas ruins: em uso por outro app, reservada, inválida e pedido malformado; nada fica registrado', () => {
  const t = montar(['CommandOrControl+Shift+L'])
  assert.deepEqual(t.atalho.definir({ ativo: true, acelerador: 'CommandOrControl+Shift+L' }), { ok: false, motivo: 'em-uso' })
  assert.deepEqual(t.atalho.definir({ ativo: true, acelerador: 'CommandOrControl+Shift+Z' }), { ok: false, motivo: 'reservado' })
  assert.deepEqual(t.atalho.definir({ ativo: true, acelerador: 'K' }), { ok: false, motivo: 'invalido' })
  assert.deepEqual(t.atalho.definir({ ativo: true }), { ok: false, motivo: 'invalido' })
  assert.deepEqual(t.atalho.definir(null), { ok: false, motivo: 'invalido' })
  assert.equal(t.registrados.size, 0)
})

test('o sistema que não consegue registrar (erro) é indisponível, sem derrubar o app', () => {
  const global: AtalhoGlobal = { register: () => { throw new Error('sem suporte') }, unregister() {}, isRegistered: () => false }
  const atalho = criarAtalhoVoz({ global, plataforma: 'linux', enviar() {} })
  assert.deepEqual(atalho.definir({ ativo: true, acelerador: 'Control+Shift+L' }), { ok: false, motivo: 'indisponivel' })
})
