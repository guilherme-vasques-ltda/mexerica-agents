import { lstatSync, readdirSync, readlinkSync, type Stats } from 'node:fs'
import { basename, dirname, isAbsolute, join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { SOURCE_LANGUAGE } from '../../shared/fork-idioma.ts'
import { privateDirectory } from '../../shared/fork-private-files.ts'
import profile from '../../../config/open.json' with { type: 'json' }

export const open = Object.freeze(profile)

const chromiumLinks = new Map([
  ['SingletonLock', /^[^/]+-\d+$/],
  ['SingletonCookie', /^[^/]+$/],
  ['SingletonSocket', /^(?:\/[^/]+)+\/(?:org\.chromium\.Chromium\.|scoped_dir)[A-Za-z0-9]+\/SingletonSocket$/],
])

/** Links que o Chromium cria na pasta de dados da instância aberta: o destino não é arquivo do perfil. */
function isChromiumLink(root: string, entry: string, child: string, item: Stats) {
  const pattern = chromiumLinks.get(entry)
  if (!pattern || !item.isSymbolicLink() || item.nlink !== 1 || item.uid !== process.getuid?.()) return false
  const target = readlinkSync(child)
  if (!pattern.test(target)) return false
  if (entry !== 'SingletonSocket') return true
  const remote = dirname(target)
  let meta: Stats
  try { meta = lstatSync(remote) } catch (error) { return (error as NodeJS.ErrnoException).code === 'ENOENT' }
  if (!meta.isDirectory() || meta.uid !== item.uid || (meta.mode & 0o777) !== 0o700) return false
  try { return readlinkSync(join(remote, 'SingletonCookie')) === readlinkSync(join(root, 'SingletonCookie')) } catch { return false }
}

/** Pasta de ajudantes que o Codex recria a cada início no `.codex/tmp` do perfil de uma conta; fica com links quando
 * ele é morto à força. Nada aqui apaga: a abertura só tolera, e só na localização real, dentro do perfil de uma
 * conta do motor — uma pasta igual em outro lugar da pasta de dados continua sendo conferida. */
function isCodexHelpers(path: string, entry: string, item: Stats, accounts: string) {
  return /^arg0(?:-[a-z]+)?$/.test(entry) && item.isDirectory() && item.uid === process.getuid?.()
    && basename(path) === 'tmp' && basename(dirname(path)) === '.codex'
    && dirname(dirname(path)).startsWith(accounts + sep)
}

function assertPrivateTree(path: string, socketRoot?: string, accounts?: string, root = '') {
  for (const entry of readdirSync(path)) {
    const child = join(path, entry)
    const item = lstatSync(child)
    if (root && isChromiumLink(root, entry, child, item)) continue
    if (accounts && isCodexHelpers(path, entry, item, accounts)) continue
    if (item.uid !== process.getuid?.() || (item.mode & 0o022) !== 0) throw new Error(traduzir(SOURCE_LANGUAGE, 'marca.open.perfilMetadados'))
    if (item.isDirectory()) assertPrivateTree(child, socketRoot, accounts)
    else if (socketRoot && item.isSocket() && (item.mode & 0o077) === 0 && /^h-[A-Za-z0-9]{6}[/\\]s$/.test(relative(socketRoot, child))) continue
    else if (!item.isFile() || item.nlink !== 1) throw new Error(traduzir(SOURCE_LANGUAGE, 'marca.open.perfilArquivo'))
  }
}

export function openPaths(home: string, appData: string) {
  const userData = privateDirectory(join(appData, open.userDataFolder))
  const homeConfig = privateDirectory(join(home, open.dataFolder))
  const accounts = join(homeConfig, 'home', open.dataFolder, 'engine-setup', 'accounts')
  assertPrivateTree(userData, undefined, undefined, userData)
  assertPrivateTree(homeConfig, join(homeConfig, 'tmp'), accounts)
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
