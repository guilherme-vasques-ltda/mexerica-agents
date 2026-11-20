const { accessSync } = require('node:fs')
const { join } = require('node:path')
const macArch = require('./mac-arch.cjs')
const open = require('../config/open.json')

module.exports = {
  appId: open.appId,
  productName: open.name,
  directories: { output: 'dist/open', buildResources: 'resources' },
  extraMetadata: { name: open.packageName, productName: open.name, main: 'out-open/main/open.js' },
  files: ['out-open/**', 'package.json', '!node_modules/**'],
  extraResources: [{ from: 'config/open.json', to: 'open-profile.json' }],
  protocols: [{ name: open.name, schemes: [open.scheme] }],
  publish: null,
  electronFuses: {
    runAsNode: false,
    enableCookieEncryption: false,
    enableNodeOptionsEnvironmentVariable: false,
    enableNodeCliInspectArguments: false,
    enableEmbeddedAsarIntegrityValidation: true,
    onlyLoadAppFromAsar: true,
  },
  beforePack: (context) => {
    const project = context.packager.projectDir
    if (context.electronPlatformName !== 'darwin' || context.packager.appInfo.id !== open.appId) {
      throw new Error('A variante Open só pode ser empacotada para macOS com sua própria identidade.')
    }
    accessSync(join(project, 'out-open/main/open.js'))
    accessSync(join(project, 'out-open/renderer/index.html'))
    accessSync(join(project, 'build/native/codync-speech'))
  },
  mac: {
    category: 'public.app-category.developer-tools',
    icon: 'resources/brand/Donnabot.icns',
    target: [{ target: 'dir', arch: process.platform === 'darwin' ? macArch() : process.arch }],
    identity: null,
    hardenedRuntime: false,
    notarize: false,
    extendInfo: { CFBundleName: open.name },
    extraResources: [{ from: 'build/native/codync-speech', to: 'codync-speech' }],
  },
}
