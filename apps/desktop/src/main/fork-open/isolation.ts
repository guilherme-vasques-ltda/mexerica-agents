import { lstatSync, readdirSync } from 'node:fs'
import { isAbsolute, join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { SOURCE_LANGUAGE } from '../../shared/fork-idioma.ts'
import { privateDirectory } from '../../shared/fork-private-files.ts'
import profile from '../../../config/open.json' with { type: 'json' }

export const open = Object.freeze(profile)

const chromiumLocks = ['SingletonLock', 'SingletonCookie', 'SingletonSocket']

function assertPrivateTree(path: string, locks = false) {
  for (const entry of readdirSync(path)) {
    const child = join(path, entry)
    const item = lstatSync(child)
    if (locks && chromiumLocks.includes(entry) && item.uid === process.getuid?.() && (item.isSymbolicLink() || (entry === 'SingletonSocket' && item.isSocket()))) continue
    if (item.uid !== process.getuid?.() || (item.mode & 0o022) !== 0) throw new Error(traduzir(SOURCE_LANGUAGE, 'marca.open.perfilMetadados'))
    if (item.isDirectory()) assertPrivateTree(child)
    else if (!item.isFile() || item.nlink !== 1) throw new Error(traduzir(SOURCE_LANGUAGE, 'marca.open.perfilArquivo'))
  }
}

export function openPaths(home: string, appData: string) {
  const userData = privateDirectory(join(appData, open.userDataFolder))
  const homeConfig = privateDirectory(join(home, open.dataFolder))
  assertPrivateTree(userData, true)
  assertPrivateTree(homeConfig)
  return { userData, homeConfig }
}

export function bringToFront(window: { isMinimized(): boolean, restore(): void, show(): void, focus(): void }, activateApp: () => void) {
  if (window.isMinimized()) window.restore()
  window.show()
  window.focus()
  activateApp()
}

export function isOpenAsset(root: string, url: string) {
  try {
    const asset = relative(root, fileURLToPath(url))
    return !!asset && asset !== '..' && !asset.startsWith(`..${sep}`) && !isAbsolute(asset)
  } catch {
    return false
  }
}
