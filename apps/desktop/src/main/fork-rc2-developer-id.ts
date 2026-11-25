import { createHash } from 'node:crypto'
import { lstatSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { Rc2Component, Rc2ComponentRole } from '../shared/fork-rc2-profile.ts'

type NativeVerifier = (path: string, role: Rc2ComponentRole) => string | null

export function verifyRc2SignedCopy(bytes: Buffer, component: Rc2Component, verify: NativeVerifier): void {
  const directory = mkdtempSync(join(tmpdir(), 'mexerica-rc2-signature-'))
  try {
    const copy = join(directory, 'component')
    writeFileSync(copy, bytes, { flag: 'wx', mode: 0o400 })
    const original = lstatSync(copy)
    const matches = () => {
      const current = lstatSync(copy)
      return current.isFile() && current.dev === original.dev && current.ino === original.ino &&
        current.size === original.size && current.ctimeMs === original.ctimeMs &&
        createHash('sha256').update(readFileSync(copy)).digest('hex') === component.sha256
    }
    if (!matches() || verify(copy, component.role) !== component.cdhash || !matches()) {
      throw new Error('invalid_rc2_artifact')
    }
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
}

export function rc2DeveloperIdVerifier(resources: string) {
  let verify: NativeVerifier | undefined
  return (bytes: Buffer, component: Rc2Component) => {
    if (!verify) {
      const addon = { exports: {} as { verify?: NativeVerifier } }
      process.dlopen(addon, join(resources, 'rc2-verify.node'))
      if (typeof addon.exports.verify !== 'function') throw new Error('invalid_rc2_artifact')
      verify = addon.exports.verify
    }
    verifyRc2SignedCopy(bytes, component, verify)
  }
}
