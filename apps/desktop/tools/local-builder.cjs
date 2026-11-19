// O Mexerica.app deste Mac (`npm run dist:mac:local`; docs/guides/app-mac-local.md). D443 (PR d): só o nome do
// pacote mudou; appId, esquemas, pasta de dados e o item do Chaveiro continuam os do Donnabot.
// Sem Apple Developer (D48/D73): nada de notarização, DMG, zip de atualização nem identidade do
// upstream. Assina ad-hoc, ou com uma identidade local explícita em DONNABOT_MAC_IDENTITY.
// Só o app, cliente do Hub (D76): sem o donnabot-host e sem o helper de tela (DonnabotScreen).
const { readFileSync } = require('node:fs')
const { join } = require('node:path')
const { parse } = require('yaml')
const macArch = require('./mac-arch.cjs')
const base = parse(readFileSync(join(__dirname, '..', 'electron-builder.yml'), 'utf8'))
const { extraFiles, signIgnore, ...mac } = base.mac
module.exports = {
  ...base,
  mac: {
    ...mac,
    // dist/mac-arm64/Mexerica.app no Apple Silicon, dist/mac/Mexerica.app no Intel.
    target: [{ target: 'dir', arch: macArch() }],
    // Ícone do pacote oficial da Mexerica (D464, #523; o nome técnico do arquivo fica; resources/brand/README.md).
    icon: 'resources/brand/Donnabot.icns',
    identity: process.env.DONNABOT_MAC_IDENTITY || '-',
    // O hardened runtime só serve à notarização; com assinatura ad-hoc ele barraria o Electron Framework.
    hardenedRuntime: false,
    notarize: false,
    timestamp: 'none',
    extraResources: [{ from: 'build/native/codync-speech', to: 'codync-speech' }],
  },
}
