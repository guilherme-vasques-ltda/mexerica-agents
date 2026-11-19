// Compila o codync-speech (reconhecimento de fala no próprio Mac, para as chamadas de voz) em
// build/native/codync-speech, só para a arquitetura deste Mac. Usado por `npm run dist:mac:local`.
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import macArch from './mac-arch.cjs'

if (process.platform !== 'darwin') process.exit(0)
const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const pkg = join(root, 'native/speech-macos')
// A arquitetura do Mac, também com o Node no Rosetta (o swift herdaria a do Node).
const args = ['build', '-c', 'release', '--arch', macArch() === 'arm64' ? 'arm64' : 'x86_64', '--package-path', pkg]
execFileSync('swift', args, { stdio: 'inherit' })
const bin = execFileSync('swift', [...args, '--show-bin-path'], { encoding: 'utf8' }).trim()
mkdirSync(join(root, 'build/native'), { recursive: true })
copyFileSync(join(bin, 'codync-speech'), join(root, 'build/native/codync-speech'))
console.log('speech: built')
