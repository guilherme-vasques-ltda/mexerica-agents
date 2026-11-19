/** Capturas internas: renderer escolhe ID, jamais prova/chave/caminho/opções. */
import { constants, promises as fs } from 'node:fs'
import { connect } from 'node:net'
import type { FileHandle } from 'node:fs/promises'
import { personalHash } from './personal-wire'
import { PersonalHelper } from './personal-helper'
import type { Principal } from './principal'
import { capturePersonalSsh, type PersonalSshProfile } from './personal-ssh'
export type PersonalOriginRecord=readonly ['localHelper',string,'/run/donnabot/bootstrap-helper-v1.sock',string] | readonly ['sshHelper',string,string,string,number,string,string,string,string]
export type PersonalCapture = Readonly<{ kind: 'localHelper' | 'sshHelper'; sourceId: string; helper: PersonalHelper; possessionURL: string; current(): boolean; provenance(): string; originRecord():PersonalOriginRecord; validateAt(epoch: number): void; check(): Promise<void>; close(): void }>
export type RejectedPersonalOrigin = Readonly<{ kind:'sshPinnedTofu'; reason:'unqualifiedOrigin' }>
const PATH = '/run/donnabot/bootstrap-helper-v1.sock'
async function protectedSocket(): Promise<readonly [bigint,bigint]> {
  if (process.platform !== 'linux') throw new Error('personalLocalRequiresLinux')
  let parent: FileHandle = await fs.open('/',constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW)
  try {
    for (const name of ['run','donnabot']) {
      const old = await parent.stat(); if (!old.isDirectory() || old.uid !== 0 || old.mode & 0o022) throw new Error('personalHelperParent')
      // /proc/self/fd conserva o diretório aberto; O_NOFOLLOW em cada componente novo.
      const next = await fs.open(`/proc/self/fd/${parent.fd}/${name}`,constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW)
      await parent.close(); parent = next
    }
    const directory = await parent.stat(); if (directory.uid !== 0 || (directory.mode & 0o7777) !== 0o750) throw new Error('personalHelperDirectory')
    const s = await fs.lstat(`/proc/self/fd/${parent.fd}/bootstrap-helper-v1.sock`,{bigint:true})
    if (!s.isSocket() || s.uid !== 0n || (s.mode & 0o7777n) !== 0o660n || s.nlink !== 1n) throw new Error('personalHelperSocket')
    return [s.dev,s.ino] as const
  } finally { await parent.close() }
}
export async function capturePersonalLocal(principal: Principal): Promise<PersonalCapture> {
  const before = await protectedSocket(); if (!principal.current()) throw new Error('personalPrincipalChanged')
  const socket = connect(PATH); let alive = true
  const current = () => alive && principal.current() && !socket.destroyed
  await new Promise<void>((resolve,reject) => { socket.once('connect',resolve); socket.once('error',reject) })
  const check = async () => { const now = await protectedSocket(); if (!current() || now[0] !== before[0] || now[1] !== before[1]) { alive=false; socket.destroy(); throw new Error('personalCaptureChanged') } }
  await check()
  const helper = new PersonalHelper(socket,socket,current,() => { alive=false; socket.destroy() })
  const provenance=()=>personalHash(['localHelper',PATH,before.map(String)])
  return Object.freeze({kind:'localHelper',sourceId:'local-linux',helper,possessionURL:`ws://127.0.0.1:${principal.deployment.env==='dev'?19423:19422}/personal-possession`,current,provenance,originRecord:()=>Object.freeze(['localHelper','local-linux',PATH,provenance()] as const),validateAt() {},check,close:() => helper.close()})
}
export async function capturePersonal(principal: Principal, sourceId: string, profile: PersonalSshProfile | null): Promise<PersonalCapture> {
  if (sourceId === 'local-linux') return capturePersonalLocal(principal)
  if (!profile || sourceId !== profile.id) throw new Error('personalOriginUnavailable')
  return capturePersonalSsh(principal,profile)
}
