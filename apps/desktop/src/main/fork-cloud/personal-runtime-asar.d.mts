import type { RuntimePolicy,RuntimeReader } from './personal-runtime-format.mjs'
export type DarwinAsarIntegrity=Readonly<Record<'Resources/app.asar'|'Resources/default_app.asar',Readonly<{algorithm:'SHA256';hash:string}>>>
export function defaultAsar(reader:RuntimeReader,policy:RuntimePolicy):void
export function darwinIntegrity(rawPlist:Uint8Array):DarwinAsarIntegrity
export function darwinDigest(rawIntegrity:DarwinAsarIntegrity,effective:unknown):string
declare const asar:{defaultAsar:typeof defaultAsar;darwinIntegrity:typeof darwinIntegrity;darwinDigest:typeof darwinDigest}
export default asar
