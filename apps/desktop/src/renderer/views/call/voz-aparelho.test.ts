import { test } from 'node:test'
import assert from 'node:assert/strict'
import { chaveSemVozNoAparelho } from './voz-aparelho.ts'
import { textos as ptBR } from '../../lib/fork-i18n/pt-BR/voz/chamada.ts'
import { textos as en } from '../../lib/fork-i18n/en/voz/chamada.ts'
import { textos as fr } from '../../lib/fork-i18n/fr/voz/chamada.ts'
import { textos as arAE } from '../../lib/fork-i18n/ar-AE/voz/chamada.ts'
import { textos as zhCN } from '../../lib/fork-i18n/zh-CN/voz/chamada.ts'
import { textos as es } from '../../lib/fork-i18n/es/voz/chamada.ts'

// Na Web o stub de plataforma diz `linux`: a Web vem antes, então nunca sai o texto do Linux.
test('na Web a voz no computador é a chave da Web, mesmo com o stub de plataforma em linux', () => {
  assert.equal(chaveSemVozNoAparelho('linux', true), 'voz.chamada.indisponivel.web')
  assert.equal(chaveSemVozNoAparelho('darwin', true), 'voz.chamada.indisponivel.web')
})

test('no app desktop o texto de hoje continua: Mac, Windows e Linux pelo sistema', () => {
  assert.equal(chaveSemVozNoAparelho('darwin', false), 'voz.chamada.indisponivel.mac')
  assert.equal(chaveSemVozNoAparelho('win32', false), 'voz.chamada.indisponivel.windows')
  assert.equal(chaveSemVozNoAparelho('linux', false), 'voz.chamada.indisponivel.linux')
})

// Texto travado nos seis idiomas: o da Web diz "neste dispositivo" e "neste navegador" (não "no próprio computador"),
// e nunca cita o Linux. Mudar um deles exige atualizar este teste.
test('o texto da Web nos seis idiomas: dispositivo e navegador, sem Linux nem "computador"', () => {
  assert.equal(ptBR['voz.chamada.indisponivel.web'], 'A voz neste dispositivo não está disponível neste navegador.')
  assert.equal(es['voz.chamada.indisponivel.web'], 'La voz en este dispositivo no está disponible en este navegador.')
  assert.equal(fr['voz.chamada.indisponivel.web'], 'La voix sur cet appareil n’est pas disponible dans ce navigateur.')
  assert.equal(arAE['voz.chamada.indisponivel.web'], 'الصوت على هذا الجهاز غير متاح في هذا المتصفح.')
  assert.equal(zhCN['voz.chamada.indisponivel.web'], '此浏览器不支持在本机处理语音。')
  assert.equal(en['voz.chamada.indisponivel.web'], 'On-device voice isn’t available in this browser.')
  for (const t of [ptBR, en, fr, arAE, zhCN, es]) assert.doesNotMatch(t['voz.chamada.indisponivel.web'], /linux|computador|ordenador|ordinateur/i)
})
