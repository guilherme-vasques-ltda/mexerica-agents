import assert from 'node:assert/strict'
import { test } from 'node:test'
import { composerEnter, isSendMode, sendShortcuts } from './composer-keyboard.ts'
const enter = { key: 'Enter', shiftKey: false, altKey: false, repeat: false, isComposing: false, keyCode: 13 }

test('Enter sends; Shift/Option-Enter preserve multiline input', () => {
  assert.equal(composerEnter(enter), 'send')
  assert.equal(composerEnter({ ...enter, shiftKey: true }), 'newline')
  assert.equal(composerEnter({ ...enter, altKey: true }), 'newline')
  assert.equal(composerEnter(enter, 'enter', true), 'send', 'the default ignores how many lines there are')
})

test('CJK candidate confirmation and a held Enter never send', () => {
  for (const mode of ['enter', 'mod-multiline', 'mod'] as const) {
    assert.equal(composerEnter({ ...enter, isComposing: true }, mode), 'ignore')
    assert.equal(composerEnter({ ...enter, keyCode: 229, metaKey: true }, mode), 'ignore')
    assert.equal(composerEnter({ ...enter, repeat: true, metaKey: true }, mode), 'ignore')
  }
})

test('⌘/Ctrl+Enter sends in every mode', () => {
  for (const mode of ['enter', 'mod-multiline', 'mod'] as const) {
    assert.equal(composerEnter({ ...enter, metaKey: true }, mode, true), 'send')
    assert.equal(composerEnter({ ...enter, ctrlKey: true }, mode, false), 'send')
    assert.equal(composerEnter({ ...enter, shiftKey: true }, mode), 'newline')
  }
})

test('⌘+Enter para várias linhas: Enter sends one line, breaks once the draft has a line break', () => {
  assert.equal(composerEnter(enter, 'mod-multiline', false), 'send')
  assert.equal(composerEnter(enter, 'mod-multiline', true), 'newline')
})

test('⌘+Enter sempre: Enter always breaks the line', () => {
  assert.equal(composerEnter(enter, 'mod', false), 'newline')
  assert.equal(composerEnter(enter, 'mod', true), 'newline')
})

test('the field announces the chosen shortcut; a bad saved value is not a mode', () => {
  assert.equal(sendShortcuts('enter', true), 'Enter Meta+Enter Shift+Enter')
  assert.equal(sendShortcuts('mod', false), 'Control+Enter Enter Shift+Enter')
  assert.equal(isSendMode('mod-multiline'), true)
  assert.equal(isSendMode('ctrl'), false)
  assert.equal(isSendMode(1), false)
})
