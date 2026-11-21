// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
// Builds the host the app bundles for the same environment as the app: node tools/native-host.mjs
// dev|main compiles it in as CODYNC_ENV. Built on every package so a host made for another
// environment is never shipped. macOS: build/native/donnabot-host (universal; Donnabot Dev: this Mac's
// architecture only, since it never leaves the Mac that built it). Windows:
// build/native/donnabot-host.exe and donnabot-hostw.exe (runs it at sign-in), plus the computer-use
// driver the host starts from beside itself. Linux apps use the installed host, so there's
// nothing to do there.
import { copyFileSync, mkdirSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import macArch from './mac-arch.cjs'

if (!['darwin', 'win32'].includes(process.platform)) process.exit(0)
const env = process.argv[2]
if (!['dev', 'main'].includes(env) && (env !== 'open' || process.platform !== 'darwin')) throw new Error(`unknown environment ${env}`)
const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const host = join(root, '../../host')
const out = join(root, 'build/native')
// The pinned toolchain (rust-toolchain.toml) through rustup, even when another cargo (Homebrew's)
// comes first on PATH: only rustup's has the x86_64 target.
const tool = (name) => execFileSync('rustup', ['which', name], { cwd: host, encoding: 'utf8' }).trim()
const [cargo, rustc] = [tool('cargo'), tool('rustc')]
const run = (cmd, args) => execFileSync(cmd, args, { stdio: 'inherit', env: { ...process.env, CODYNC_ENV: env, RUSTC: rustc } })
// Donnabot Dev builds in the host's `dev-app` profile: seconds to rebuild instead of minutes.
const profile = env === 'dev' ? 'dev-app' : 'release'
const build = (target) => run(cargo, ['build', '--profile', profile, '--manifest-path', join(host, 'Cargo.toml'), '--target', target])
const built = (target, name) => join(host, 'target', target, profile, name)
mkdirSync(out, { recursive: true })
if (process.platform === 'win32') {
  const target = 'x86_64-pc-windows-msvc'
  build(target)
  for (const exe of ['donnabot-host.exe', 'donnabot-hostw.exe']) copyFileSync(built(target, exe), join(out, env === 'dev' && exe === 'donnabot-hostw.exe' ? 'donnabot-dev-hostw.exe' : exe))
  run(process.execPath, [join(root, '../../packaging/cua-driver/fetch.mjs'), 'windows-x86_64', out])
} else {
  if (env === 'open') {
    const target = macArch() === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin'
    run(cargo, ['build', '--locked', '--profile', profile, '--manifest-path', join(host, 'Cargo.toml'), '--target', target, '--bin', 'donnabot-host'])
    copyFileSync(built(target, 'donnabot-host'), join(out, 'mexerica-open-hub'))
  } else {
    const native = process.arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin'
    const targets = env === 'dev' ? [native] : ['aarch64-apple-darwin', 'x86_64-apple-darwin']
    for (const target of targets) build(target)
    run('lipo', ['-create', '-output', join(out, 'donnabot-host'), ...targets.map((t) => built(t, 'donnabot-host'))])
  }
}
console.log(`host: ${env}`)
