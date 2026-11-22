const { execFileSync } = require('node:child_process')
const { rmSync } = require('node:fs')
const { dirname, join } = require('node:path')

if (process.argv.length !== 3 || process.argv[2] !== process.platform || (process.platform !== 'darwin' && process.arch !== 'x64')) {
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
rmSync('build/official/out', { recursive: true, force: true })
node(join(dirname(require.resolve('electron-vite/package.json')), 'bin/electron-vite.js'), ['build', '--mode', 'official', '--outDir', 'build/official/out'])
require('./fork-official-builder.cjs').beforePack({ packager: { projectDir: process.cwd() }, electronPlatformName: process.platform }).then(() => {
  node(require.resolve('electron-builder/out/cli/cli.js'), [...platform, '--config', 'tools/fork-official-builder.cjs', '--publish', 'never'], { ...process.env, CSC_IDENTITY_AUTO_DISCOVERY: 'false' })
}).catch(error => { console.error(error.message); process.exitCode = 1 })
