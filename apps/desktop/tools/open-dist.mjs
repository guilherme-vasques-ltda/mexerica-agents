import { execFileSync } from 'node:child_process'
import { join } from 'node:path'
import macArch from './mac-arch.cjs'
import open from '../config/open.json' with { type: 'json' }

if (process.platform !== 'darwin') throw new Error('O pacote Open exige macOS.')

execFileSync(process.execPath, ['tools/native-speech.mjs'], { stdio: 'inherit' })
execFileSync('node_modules/.bin/electron-vite', ['build', '--mode', 'open', '--outDir', 'out-open'], { stdio: 'inherit' })
execFileSync('node_modules/.bin/electron-builder', ['--mac', '--config', 'tools/open-builder.cjs', '--publish', 'never'], { stdio: 'inherit' })
const bundle = join('dist/open', macArch() === 'arm64' ? 'mac-arm64' : 'mac', `${open.name}.app`)
execFileSync('/usr/bin/codesign', ['--sign', '-', '--timestamp=none', '--force', '--deep', bundle], { stdio: 'inherit' })
execFileSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', bundle], { stdio: 'inherit' })
