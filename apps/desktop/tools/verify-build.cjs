// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
// Fail before packaging if a stale account-config.json belongs to the other app variant.
const { readFileSync } = require('node:fs')
const { join } = require('node:path')
module.exports = async (context) => {
  await require('./personal-pin-receipt.cjs')(context)
  const expected = context.packager.appInfo.id === 'app.donnabot.mac.dev' ? 'dev' : 'main'
  const config = JSON.parse(readFileSync(join(context.packager.projectDir, 'resources/account-config.json'), 'utf8'))
  if (config.environment !== expected) throw new Error(`Packaging ${expected} with ${config.environment} account config; run the matching dist script.`)
  if (context.electronPlatformName === 'darwin') {
    const symbols = join(context.packager.projectDir, 'out/renderer/symbols')
    const manifest = JSON.parse(readFileSync(join(symbols, 'manifest.json'), 'utf8'))
    // O logotipo da Apple não é exportado: marca de terceiros, o app usa um símbolo genérico de conta.
    if (manifest['apple.logo']) throw new Error('The macOS build exports the Apple logo; remove apple.logo from src/renderer/components/symbols.json.')
  }
}
