// A arquitetura deste Mac, não a do Node: `hw.optional.arm64` vale 1 no Apple Silicon mesmo com o
// Node (e o que ele inicia) rodando no Rosetta. Sem a chave, é um Mac Intel.
const { execFileSync } = require('node:child_process')
module.exports = function macArch() {
  try {
    return execFileSync('/usr/sbin/sysctl', ['-n', 'hw.optional.arm64'], { encoding: 'utf8' }).trim() === '1' ? 'arm64' : 'x64'
  } catch {
    return 'x64'
  }
}
