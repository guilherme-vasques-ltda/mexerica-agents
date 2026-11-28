import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const source = fileURLToPath(new URL('../native/fork-hub/supervisor.c', import.meta.url))

export function buildHubSupervisor(output, architectures, defines = []) {
  execFileSync('/usr/bin/clang', [
    '-std=c11', '-D_DARWIN_C_SOURCE', '-O2', '-Wall', '-Wextra', '-Werror',
    ...architectures.flatMap(arch => ['-arch', arch]),
    ...defines.map(define => '-D' + define),
    source, '-o', output,
  ], { stdio: 'inherit' })
  execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', '--timestamp=none', output], { stdio: 'inherit' })
}
