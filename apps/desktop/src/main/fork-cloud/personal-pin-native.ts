/** Main privado. A prova Node26 não admite uma imagem no Electron. */
import type { PinDirectory } from './personal-pin-fs'
export const pinNativeMissing=Object.freeze([
  'electron-44.5.1-patched-node-24.21-image-production-arm64-x64',
  // Contrato B (docs/reference/hub-linux-pin-native-load.md): substitui 'signed-whole-image-filefd-load-without-path-reopen'.
  'system-preload-authentication-cdhash-library-constraint-hardened-runtime',
  'product-signing-team-id-and-constraint-embedding-proof',
  'preload-negatives-unsigned-other-team-same-team-swapped-before-initializers',
  'protected-parent-walk-and-historical-pair-adoption',
  'sealed-before-pack-receipt-and-installed-package-proof',
] as const)
/** Nenhuma flag/env/DTO/recibo cru instala uma implementação ou aprende digest.
 * Até fechar as provas enumeradas, nem dlopen nem open userData são chamados.
 * A factory qualificada futura é interna deste módulo, nunca um plugin registry.
 */
export async function openPersonalPinDirectory(current:()=>void):Promise<PinDirectory>{
  current();throw new Error('personalPinNativeQualificationPending')
}

export function personalPinNativeAdmitted():boolean{return false}
