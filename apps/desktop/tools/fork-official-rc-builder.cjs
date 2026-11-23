const { createHash } = require('node:crypto')
const { join } = require('node:path')
const official = require('./fork-official-builder.cjs')
const { manifestPath, rcManifest, manifestBytes, readRegularFile } = require('./fork-official-rc.cjs')

const sidecarPath = 'build/official/out/main/rc-manifest.sha256'
const resources = { darwin: ['codync-speech'], win32: [], linux: [] }

module.exports = ({ projectDir }) => {
  const manifest = rcManifest(projectDir)
  const { profile, version, appId, name, scheme, packageName } = manifest
  const [macVersion, rcNumber] = version.split('-rc.')
  return {
    ...official,
    appId,
    productName: name,
    buildNumber: rcNumber,
    directories: { ...official.directories, output: 'dist/official-rc' },
    extraMetadata: { name: packageName, productName: name, version, mexericaProfile: profile, mexericaAppId: appId },
    extraResources: [...official.extraResources, { from: manifestPath, to: 'rc-manifest.json' }],
    protocols: [{ name, schemes: [scheme] }],
    publish: null,
    mac: {
      ...official.mac,
      bundleShortVersion: macVersion,
      bundleVersion: rcNumber,
      target: [{ target: 'dmg', arch: 'universal' }],
      extraResources: official.mac.extraResources.filter(({ to }) => resources.darwin.includes(to)),
    },
    dmg: { ...official.dmg, artifactName: 'mexerica-rc-${version}-macos.${ext}' },
    win: {
      ...official.win,
      executableName: packageName,
      artifactName: 'mexerica-rc-${version}-windows-x64.${ext}',
      extraResources: official.win.extraResources.filter(({ to }) => to === 'icon-tray.png'),
    },
    nsis: { ...official.nsis, include: null, differentialPackage: false },
    linux: {
      ...official.linux,
      executableName: packageName,
      artifactName: 'mexerica-rc-${version}-linux-${arch}.${ext}',
      desktop: { entry: { StartupWMClass: name } },
      extraResources: [],
    },
    deb: { ...official.deb, packageName },
    async beforePack(context) {
      const project = context.packager.projectDir
      try {
        const bytes = readRegularFile(join(project, manifestPath))
        const pin = readRegularFile(join(project, sidecarPath))
        if (!bytes.equals(manifestBytes(manifest)) || !pin.equals(Buffer.from(createHash('sha256').update(bytes).digest('hex')))) throw new Error()
      } catch { throw new Error('Official RC manifest does not match the compiled desktop.') }
      await official.beforePack(context, resources)
    },
  }
}
