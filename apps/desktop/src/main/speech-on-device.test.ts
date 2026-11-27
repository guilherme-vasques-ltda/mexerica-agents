import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const helper = new URL('../../native/speech-macos/Sources/', import.meta.url)

test('o helper recusa fala indisponível antes de pedir o microfone e revalida antes da captura', () => {
  const main = readFileSync(new URL('main.swift', helper), 'utf8')
  const start = main.slice(main.indexOf('func start()'), main.indexOf('func stop()'))
  assert.match(start, /guard let recognizer, localSpeech\.canStart else \{\s*return fail\(Self\.localSpeechUnavailable\)\s*\}[\s\S]*?await AVCaptureDevice\.requestAccess/)
  assert.match(start, /await Self\.speechAuthorized[\s\S]*?guard let request = localSpeech\.makeRequest\(\) else \{\s*return fail\(Self\.localSpeechUnavailable\)\s*\}[\s\S]*?input\.installTap[\s\S]*?engine\.start\(\)[\s\S]*?recognitionTask\(with: request,/)
  const policy = readFileSync(new URL('OnDeviceSpeech.swift', helper), 'utf8')
  assert.match(policy, /var canStart: Bool \{ supportsOnDeviceRecognition && isAvailable \}/)
  assert.match(policy, /guard canStart else \{ return nil \}\s*let request = SFSpeechAudioBufferRecognitionRequest\(\)\s*request\.requiresOnDeviceRecognition = true\s*request\.shouldReportPartialResults = true\s*return request/)
  assert.equal((main + policy).match(/SFSpeechAudioBufferRecognitionRequest\(\)/g)?.length, 1)
})

test('a lista inclui só idiomas com suporte local', () => {
  const main = readFileSync(new URL('main.swift', helper), 'utf8')
  assert.match(main, /SFSpeechRecognizer\.supportedLocales\(\)\s*\.filter \{ SFSpeechRecognizer\(locale: \$0\)\?\.supportsOnDeviceRecognition == true \}/)
})

test('a Open registra a fala nativa só para a janela local e não libera permissões de mídia ao renderer', () => {
  const open = readFileSync(new URL('./fork-open/main.ts', import.meta.url), 'utf8')
  assert.match(open, /isWindowSender\(event, window, chatURL\) && event\.senderFrame\?\.url === chatURL/)
  assert.match(open, /registerSpeech\(daJanela\)/)
  assert.match(open, /setPermissionRequestHandler\(\(_contents, _permission, callback\) => callback\(false\)\)/)
})
