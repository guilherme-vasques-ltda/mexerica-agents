import { test } from 'node:test'
import { readFileSync } from 'node:fs'
import assert from 'node:assert/strict'
import { frasesAparelho } from './frases.ts'
import { textos as pt } from '../../../lib/fork-i18n/pt-BR/voz/ajustes.ts'
import { textos as en } from '../../../lib/fork-i18n/en/voz/ajustes.ts'
import { textos as es } from '../../../lib/fork-i18n/es/voz/ajustes.ts'
import { textos as fr } from '../../../lib/fork-i18n/fr/voz/ajustes.ts'
import { textos as zh } from '../../../lib/fork-i18n/zh-CN/voz/ajustes.ts'
import { textos as ar } from '../../../lib/fork-i18n/ar-AE/voz/ajustes.ts'

// A tela de Voz diz o aparelho em três lugares: o subtítulo ("neste ..."), o Pill ao lado dele (o nome) e o aviso
// "sem cofre". Na Web o aparelho é o navegador, um dispositivo; no app, o de sempre. Os catálogos são lidos direto.
const tradutor = (catalogo: Record<string, string>) => ((chave: string) => catalogo[chave] ?? chave) as never
const idiomas: Array<[string, Record<string, string>]> = [['pt', pt], ['en', en], ['es', es], ['fr', fr], ['zh', zh], ['ar', ar]]

test('na Web, o Pill diz o dispositivo nos seis idiomas (não "Este computador")', () => {
  assert.equal(frasesAparelho(tradutor(pt), false, true).aparelho, 'Este dispositivo')
  assert.equal(frasesAparelho(tradutor(en), false, true).aparelho, 'This device')
  assert.equal(frasesAparelho(tradutor(es), false, true).aparelho, 'Este dispositivo')
  assert.equal(frasesAparelho(tradutor(fr), false, true).aparelho, 'Cet appareil')
  assert.equal(frasesAparelho(tradutor(zh), false, true).aparelho, '这台设备')
  assert.equal(frasesAparelho(tradutor(ar), false, true).aparelho, 'هذا الجهاز')
})

test('na Web, o subtítulo e o aviso "sem cofre" usam o dispositivo; em Mac-Web também', () => {
  assert.equal(frasesAparelho(tradutor(pt), false, true).neste, 'neste dispositivo')
  assert.equal(frasesAparelho(tradutor(pt), false, true).deste, 'deste dispositivo')
  assert.equal(frasesAparelho(tradutor(pt), true, true).aparelho, 'Este dispositivo', 'navegador num Mac: ainda dispositivo')
})

test('no app desktop, o Pill e o subtítulo são os de sempre: "Este computador" fora do Mac, "Este Mac" no Mac', () => {
  assert.equal(frasesAparelho(tradutor(pt), false, false).aparelho, 'Este computador')
  assert.equal(frasesAparelho(tradutor(pt), true, false).aparelho, 'Este Mac')
  assert.equal(frasesAparelho(tradutor(en), false, false).aparelho, 'This computer')
})

test('Web e app nunca dizem o mesmo aparelho, nos seis idiomas', () => {
  for (const [nome, catalogo] of idiomas) {
    const naWeb = frasesAparelho(tradutor(catalogo), false, true)
    const noApp = frasesAparelho(tradutor(catalogo), false, false)
    assert.notEqual(naWeb.aparelho, noApp.aparelho, `${nome}: Pill`)
    assert.notEqual(naWeb.neste, noApp.neste, `${nome}: subtítulo`)
  }
})

test('os textos montados em pt: subtítulo e aviso "sem cofre" na Web e no app', () => {
  const topo = pt['voz.ajustes.topo']
  assert.equal(topo.replace('{neste}', frasesAparelho(tradutor(pt), false, true).neste), 'Como os bots ouvem você e com que voz respondem neste dispositivo. Cada aparelho tem as suas escolhas.')
  assert.equal(topo.replace('{neste}', frasesAparelho(tradutor(pt), false, false).neste), 'Como os bots ouvem você e com que voz respondem neste computador. Cada aparelho tem as suas escolhas.')
  const semCofre = pt['voz.ajustes.semCofre']
  assert.equal(semCofre.replace('{aparelho}', frasesAparelho(tradutor(pt), false, true).aparelho), 'Este dispositivo não tem um cofre seguro para guardar chaves; a voz usa a chave do computador principal.')
  assert.equal(semCofre.replace('{aparelho}', frasesAparelho(tradutor(pt), false, false).aparelho), 'Este computador não tem um cofre seguro para guardar chaves; a voz usa a chave do computador principal.')
})

// VozPage importa CSS, então o teste lê o fonte: o Pill e o aviso "sem cofre" têm de usar o aparelho daquela página.
test('a tela de Voz desenha o Pill e o aviso "sem cofre" com o aparelho de frasesAparelho (Web x app)', () => {
  const fonte = readFileSync(new URL('./VozPage.tsx', import.meta.url), 'utf8')
  assert.match(fonte, /<Icon name="desktopcomputer" size=\{11\} \/>\{f\.aparelho\}/)
  assert.match(fonte, /t\(TEXTO_SEM_COFRE, \{ aparelho: f\.aparelho \}\)/)
  assert.match(fonte, /const f = frasesAparelho\(t, mac\)/)
})

// O caminho real: a tela chama `frasesAparelho(t, mac)` sem dizer se é Web; a decisão vem da ponte da janela.
test('sem dizer nada, frasesAparelho segue a ponte da janela: Web diz dispositivo, app diz computador', () => {
  const janela = globalThis as { window?: unknown }
  const antes = janela.window
  try {
    janela.window = { codync: { web: true, platform: 'linux' } }
    assert.equal(frasesAparelho(tradutor(pt), false).aparelho, 'Este dispositivo')
    janela.window = { codync: { platform: 'win32' } }
    assert.equal(frasesAparelho(tradutor(pt), false).aparelho, 'Este computador')
  } finally {
    janela.window = antes
  }
})
