const { lstatSync, readFileSync, rmSync, writeFileSync } = require('node:fs')
const { join } = require('node:path')

const manifestPath = 'build/official/rc-manifest.json'

function rcManifest(projectDir) {
  const { version } = JSON.parse(readFileSync(join(projectDir, 'package.json'), 'utf8'))
  if (typeof version !== 'string' || !/^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)-rc\.[1-9]\d*(?![\s\S])/.test(version)) {
    throw new Error('Official RC requires a package version of the form X.Y.Z-rc.N (N >= 1).')
  }
  return { profile: 'rc', version, appId: 'app.donnabot.mac.rc', name: 'Mexerica RC', scheme: 'mexerica-rc', packageName: 'mexerica-rc' }
}

const manifestBytes = manifest => Buffer.from(`${JSON.stringify(manifest)}\n`)

function stageManifest(projectDir) {
  const bytes = manifestBytes(rcManifest(projectDir))
  const file = join(projectDir, manifestPath)
  rmSync(file, { force: true })
  writeFileSync(file, bytes, { flag: 'wx' })
}

function readRegularFile(file) {
  if (!lstatSync(file).isFile()) throw new Error('Not a regular file.')
  return readFileSync(file)
}

module.exports = { manifestPath, rcManifest, manifestBytes, stageManifest, readRegularFile }
