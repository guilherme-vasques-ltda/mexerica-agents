import assert from 'node:assert/strict'
import test from 'node:test'
import { origemDoChat, permiteNoChat } from './seguranca-renderer.ts'

const renderer = 'file:///app/renderer/index.html?window=chat'
const chat = { janelaPrincipal: true, framePrincipal: true, url: renderer }

test('aceita o documento principal do chat', () => {
  assert.equal(origemDoChat(renderer, chat), true)
})

test('recusa outra janela, um frame filho e um frame ausente', () => {
  assert.equal(origemDoChat(renderer, { ...chat, janelaPrincipal: false }), false)
  assert.equal(origemDoChat(renderer, { ...chat, framePrincipal: false }), false)
  assert.equal(origemDoChat(renderer, { ...chat, url: undefined }), false)
})

test('recusa origem externa e outras páginas da mesma origem', () => {
  for (const url of ['https://example.test/', 'file:///app/renderer/outro.html?window=chat', 'file:///app/renderer/index.html?window=pairing']) {
    assert.equal(origemDoChat(renderer, { ...chat, url }), false)
  }
  const externa = 'https://example.test/?window=chat'
  assert.equal(origemDoChat(externa, { ...chat, url: externa }), false)
})

test('só permite microfone e escrita segura na área de transferência do chat', () => {
  assert.equal(permiteNoChat(renderer, chat, 'media', ['audio'], 'file://'), true)
  assert.equal(permiteNoChat(renderer, chat, 'clipboard-sanitized-write'), true)
  for (const tipos of [['video'], ['audio', 'video'], ['unknown'], []]) {
    assert.equal(permiteNoChat(renderer, chat, 'media', tipos, 'file://'), false)
  }
  assert.equal(permiteNoChat(renderer, chat, 'display-capture', ['audio'], 'file://'), false)
  assert.equal(permiteNoChat(renderer, chat, 'clipboard-read'), false)
})

test('nega mídia se a janela, o frame, o documento ou a origem não corresponder', () => {
  assert.equal(permiteNoChat(renderer, { ...chat, janelaPrincipal: false }, 'media', ['audio'], 'file://'), false)
  assert.equal(permiteNoChat(renderer, { ...chat, framePrincipal: false }, 'media', ['audio'], 'file://'), false)
  assert.equal(permiteNoChat(renderer, { ...chat, url: undefined }, 'media', ['audio'], 'file://'), false)
  assert.equal(permiteNoChat(renderer, chat, 'media', ['audio'], 'https://example.test'), false)
  assert.equal(permiteNoChat(renderer, chat, 'media', ['audio']), false)
})

test('aceita microfone no renderer de desenvolvimento configurado', () => {
  const desenvolvimento = 'http://localhost:5173/?window=chat'
  assert.equal(permiteNoChat(desenvolvimento, { ...chat, url: desenvolvimento }, 'media', ['audio'], 'http://localhost:5173/'), true)
})
