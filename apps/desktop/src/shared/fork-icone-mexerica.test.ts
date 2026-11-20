// D451/D464 (PR f): os ícones que o build e o pacote oficial referenciam são os arquivos do pacote oficial do dono (#542,
// brand/assets/logo), byte a byte, com os tamanhos esperados.
import { createHash } from 'node:crypto'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
const require = createRequire(import.meta.url)
const { parse } = require('yaml')
const desktop = new URL('../../', import.meta.url)
const repo = new URL('../../../../', import.meta.url)
const file = (base: URL, path: string) => readFileSync(new URL(path, base))

/** Largura e altura do IHDR de um PNG. */
function png(data: Buffer): [number, number] {
  assert.equal(data.subarray(0, 8).toString('hex'), '89504e470d0a1a0a')
  return [data.readUInt32BE(16), data.readUInt32BE(20)]
}

test('o .icns do build local é o mexerica.icns do pacote, com os tamanhos de 32 a 1024 em PNG', () => {
  const local = require('../../tools/local-builder.cjs')
  assert.equal(local.mac.icon, 'resources/brand/Donnabot.icns')
  const icns = file(desktop, local.mac.icon)
  assert.equal(icns.subarray(0, 4).toString('latin1'), 'icns')
  assert.equal(icns.readUInt32BE(4), icns.length)
  const found: Record<string, number> = {}
  for (let at = 8; at < icns.length;) {
    const size = icns.readUInt32BE(at + 4)
    const type = icns.subarray(at, at + 4).toString('latin1')
    if (type !== 'TOC ') found[type] = png(icns.subarray(at + 8, at + size))[0]
    at += size
  }
  assert.deepEqual(found, { ic07: 128, ic08: 256, ic09: 512, ic10: 1024, ic11: 32, ic12: 64, ic13: 256, ic14: 512 })
})

test('o AppIcon.icon e o icon.png do builder existem em 1024 px', () => {
  const prod = parse(readFileSync(new URL('electron-builder.yml', desktop), 'utf8'))
  assert.equal(prod.mac.icon, 'resources/AppIcon.icon')
  const composer = JSON.parse(file(desktop, 'resources/AppIcon.icon/icon.json').toString('utf8'))
  for (const layer of composer.groups.flatMap((g: { layers: { 'image-name': string }[] }) => g.layers)) {
    assert.deepEqual(png(file(desktop, `resources/AppIcon.icon/Assets/${layer['image-name']}`)), [1024, 1024])
  }
  assert.deepEqual(png(file(desktop, prod.linux.icon)), [1024, 1024])
})

test('os favicons do renderer e do pacote oficial existem nos tamanhos declarados', () => {
  const html = file(desktop, 'src/renderer/index.html').toString('utf8')
  for (const [, sizes, href] of html.matchAll(/<link rel="icon" type="image\/png" sizes="(\d+)x\d+" href="\.\/([^"]+)"/g)) {
    assert.deepEqual(png(file(new URL('src/renderer/', desktop), href)), [Number(sizes), Number(sizes)], href)
  }
  assert.deepEqual(png(file(desktop, 'src/renderer/assets/brand/favicons/favicon-180.png')), [180, 180])
  assert.deepEqual(png(file(repo, 'brand/assets/logo/06-web-e-redes/apple-touch-icon-180.png')), [180, 180])
  assert.deepEqual(png(file(repo, 'brand/assets/logo/06-web-e-redes/icone-app-web-512.png')), [512, 512])
  for (const ico of ['src/renderer/assets/brand/favicons/favicon.ico', '../../brand/assets/logo/06-web-e-redes/favicon.ico']) {
    const data = file(desktop, ico)
    assert.equal(data.readUInt16LE(2), 1, ico) // ICO
    assert.equal(data.readUInt16LE(4), 4, ico) // 16, 32, 48 e 64
  }
})

// D464: cada arquivo do app e do pacote oficial é o do pacote do dono, byte a byte (o SVG do app só embute o PNG de 128).
// SHA-256 dos arquivos de brand/assets/logo na main (#542, 77939a31); conferidos na cópia com `git show`.
const PACOTE: Record<string, string> = {
  '05-icone-app/macos/mexerica.icns': 'c931ee20b83fbff302b4993d4877dcdd4391d584132dc9fac0d88fcb266f20b3',
  '05-icone-app/macos/icone-macos-1024.png': '008dde7f90e7912c0c3f7e36ee42d23dc57c974a82781364a35b7b9bbfeaddd2',
  '05-icone-app/icone-mestre-1024.png': 'a8bc2e3c047ccb3fa68bbdca43da1742c77f807873247a936dd5fbfddc8cade1',
  '06-web-e-redes/favicon.ico': 'ce5b34e7bb65325f32ae04b3b5940c7bfae7dc117fa9f47d24f0fb2e6f5e01ff',
  '06-web-e-redes/favicon-16.png': '3aa50101b0d1c2f3f533ace227501dc1df51c54f04aaba9003cf4d79abedbcf8',
  '06-web-e-redes/favicon-32.png': '8002bf845d8d05979928c72a8b37b41aa9a50b95e02c2677f5c6eec64bb01c91',
  '06-web-e-redes/apple-touch-icon-180.png': '436a8c838c42f63dca7d86295a493a303fc7eae144ef07e9067a3b87d1a7f249',
  '06-web-e-redes/icone-app-web-192.png': '362e93b13271a7d3eace38458af274fe3e8af6b3c80cda3005d3c45552307075',
  '06-web-e-redes/favicon.svg': '5451337fd0904ca2d97102ccae47428db3e4cfc6f0162af5467850d77f662355',
  '06-web-e-redes/icone-app-web-512.png': 'a3a8e77bb01af8120a32498dd355b0107e5953a7d13e6d199ccfd8b9f9faa83f',
  '05-icone-app/macos/icone-macos-128.png': 'f000a79e557e5b5721a1ef6140ba81e0c11be7c73f5aca6f35e9812c670e3bfc',
}
test('D464: ícones do app e do pacote oficial têm os mesmos bytes do pacote oficial (brand/assets/logo)', () => {
  const sha = (data: Buffer) => createHash('sha256').update(data).digest('hex')
  const pares: [string, string][] = [
    ['05-icone-app/macos/mexerica.icns', 'apps/desktop/resources/brand/Donnabot.icns'],
    ['05-icone-app/macos/icone-macos-1024.png', 'apps/desktop/resources/icon.png'],
    ['05-icone-app/icone-mestre-1024.png', 'apps/desktop/resources/AppIcon.icon/Assets/image 9.png'],
    ['06-web-e-redes/favicon.ico', 'apps/desktop/src/renderer/assets/brand/favicons/favicon.ico'],
    ['06-web-e-redes/favicon-16.png', 'apps/desktop/src/renderer/assets/brand/favicons/favicon-16.png'],
    ['06-web-e-redes/favicon-32.png', 'apps/desktop/src/renderer/assets/brand/favicons/favicon-32.png'],
    ['06-web-e-redes/apple-touch-icon-180.png', 'apps/desktop/src/renderer/assets/brand/favicons/favicon-180.png'],
    ['06-web-e-redes/icone-app-web-192.png', 'apps/desktop/src/renderer/assets/brand/favicons/favicon-192.png'],
    ['06-web-e-redes/favicon.ico', 'brand/assets/logo/06-web-e-redes/favicon.ico'],
    ['06-web-e-redes/favicon.svg', 'brand/assets/logo/06-web-e-redes/favicon.svg'],
    ['06-web-e-redes/icone-app-web-512.png', 'brand/assets/logo/06-web-e-redes/icone-app-web-512.png'],
    ['06-web-e-redes/apple-touch-icon-180.png', 'brand/assets/logo/06-web-e-redes/apple-touch-icon-180.png'],
  ]
  for (const [origem, destino] of pares) assert.equal(sha(file(repo, destino)), PACOTE[origem], `${destino} ← brand/assets/logo/${origem}`)
  const svg = file(desktop, 'resources/icon.svg').toString('utf8')
  const embutido = Buffer.from(/href="data:image\/png;base64,([^"]+)"/.exec(svg)![1]!, 'base64')
  assert.equal(sha(embutido), PACOTE['05-icone-app/macos/icone-macos-128.png'])
})
