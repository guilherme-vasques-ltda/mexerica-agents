import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { defineConfig } from 'electron-vite'
import { compileRc2Profile, rc2ManifestPath } from './src/shared/fork-rc2-profile.ts'

export default defineConfig(({ mode }) => {
  if (mode !== 'official-rc2') throw new Error('invalid_rc2_profile')
  const version = JSON.parse(readFileSync(resolve('package.json'), 'utf8')).version
  const components = JSON.parse(readFileSync(resolve('build/official-rc2/components.json'), 'utf8'))
  const compiled = compileRc2Profile(mode, version, process, components)
  if (!compiled) throw new Error('invalid_rc2_profile')
  return {
    main: {
      define: {
        __MEXERICA_RC2_PROFILE__: JSON.stringify(compiled.distribution.profile),
        __MEXERICA_RC2_MANIFEST_HASH__: JSON.stringify(compiled.pin),
      },
      build: { outDir: 'build/official-rc2/out/main', rollupOptions: { input: { rc2: resolve('src/main/fork-rc2-entry.ts') } } },
      plugins: [{ name: 'mexerica-rc2-profile', generateBundle() {
        this.emitFile({ type: 'asset', fileName: rc2ManifestPath, source: compiled.bytes })
      } }],
    },
  }
})
