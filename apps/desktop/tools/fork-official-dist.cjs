const { execFileSync } = require('node:child_process')
const { rmSync } = require('node:fs')
const { dirname, join } = require('node:path')

const rc = process.argv[3] === 'official-rc'
if (process.argv.length !== (rc ? 4 : 3) || process.argv[2] !== process.platform || (process.platform !== 'darwin' && process.arch !== 'x64')) {
  throw new Error('Build the official package on its target operating system.')
}
const platform = {
  darwin: ['--mac'],
  win32: ['--win', '--x64'],
  linux: ['--linux', '--x64'],
}[process.platform]
if (!platform) throw new Error('Unsupported official desktop platform.')

function node(entry, args, env = process.env) {
  execFileSync(process.execPath, [entry, ...args], { stdio: 'inherit', env })
}

node('tools/account-config.mjs', ['main', '--official'])
const projectDir = process.cwd()
if (rc) require('./fork-official-rc.cjs').stageManifest(projectDir)
rmSync('build/official/out', { recursive: true, force: true })
node(join(dirname(require.resolve('electron-vite/package.json')), 'bin/electron-vite.js'), ['build', '--mode', rc ? 'official-rc' : 'official', '--outDir', 'build/official/out'])
const builderPath = rc ? 'tools/fork-official-rc-builder.cjs' : 'tools/fork-official-builder.cjs'
const builder = rc ? require('./fork-official-rc-builder.cjs')({ projectDir }) : require('./fork-official-builder.cjs')
builder.beforePack({ packager: { projectDir }, electronPlatformName: process.platform }).then(() => {
  node(require.resolve('electron-builder/out/cli/cli.js'), [...platform, '--config', builderPath, '--publish', 'never'], { ...process.env, CSC_IDENTITY_AUTO_DISCOVERY: 'false' })
}).catch(error => { console.error(error.message); process.exitCode = 1 })
