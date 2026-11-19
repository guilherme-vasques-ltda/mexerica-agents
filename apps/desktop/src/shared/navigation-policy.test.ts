import assert from 'node:assert/strict'
import test from 'node:test'
import { decideNavigation, navigationLogProtocol } from './navigation-policy.ts'

const packaged = 'file:///Applications/Donnabot.app/Contents/Resources/app.asar/out/renderer/index.html?window=chat'
const dev = 'http://localhost:5173/?window=chat'

test('só a entrada do app mantém navegação interna, em pacote e em desenvolvimento', () => {
  assert.equal(decideNavigation(packaged.replace('chat', 'pairing') + '#section', packaged), 'allow')
  assert.equal(decideNavigation(dev.replace('chat', 'island') + '#section', dev), 'allow')
  assert.equal(decideNavigation('file:///tmp/index.html', packaged), 'block')
  assert.equal(decideNavigation(packaged.replace('/renderer/', '/other/'), packaged), 'block')
  assert.equal(decideNavigation('file://other/Applications/Donnabot.app/Contents/Resources/app.asar/out/renderer/index.html', packaged), 'block')
  assert.equal(decideNavigation('http://localhost:5174/', dev), 'external')
  assert.equal(decideNavigation('http://localhost:5173/other.html', dev), 'external')
  assert.equal(decideNavigation('http://localhost.evil.test:5173/', dev), 'external')
  assert.equal(decideNavigation('http://user:password@localhost:5173/', dev), 'external')
  assert.equal(decideNavigation(dev, packaged), 'external')
})

test('web e e-mail abrem fora; esquemas de arquivos, apps e URLs inválidas são bloqueados', () => {
  for (const url of ['http://example.test/', 'HTTPS://example.test/path?q=value', 'mailto:person@example.test']) {
    assert.equal(decideNavigation(url, packaged), 'external', url)
  }
  for (const url of ['file:///etc/passwd', 'smb://example.test/share', 'javascript:alert(1)', 'data:text/html,test',
    'about:blank', 'donnabot://oauth', 'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
    'https://', '/relative', '', null, { toString: () => 'https://example.test/' }]) {
    assert.equal(decideNavigation(url, packaged), 'block')
  }
})

test('o log retém só o esquema, sem credenciais, caminho, query, fragmento ou entrada inválida', () => {
  assert.equal(navigationLogProtocol('smb://user:secret@example.test/private?token=secret#secret'), 'smb:')
  assert.equal(navigationLogProtocol('mailto:private@example.test?body=secret'), 'mailto:')
  assert.equal(navigationLogProtocol('invalid secret?token=secret'), 'invalid')
  assert.equal(navigationLogProtocol(null), 'invalid')
})
