/** Seams produtivas estritamente Main. Nenhuma fábrica vem de IPC/env/JSON. */
import { safeStorage } from 'electron'
import type { Principal } from './principal'
import type { DeviceBinding } from '../fork-account/service'
import type { PersonalManifest } from './personal-protocol'
import { PersonalPinStore } from './personal-pin-store'
import { openPersonalPinDirectory } from './personal-pin-native'
export async function openPersonalPinStore(p:Principal,binding:Readonly<DeviceBinding>,authority:PersonalManifest,current:()=>void){
  const secure=()=>{
    current();if(!p.current() || !safeStorage.isEncryptionAvailable() || !['darwin','linux'].includes(process.platform) || process.platform==='linux' && !['gnome_libsecret','kwallet','kwallet5','kwallet6'].includes(safeStorage.getSelectedStorageBackend()))throw new Error('personalPinStorageUnavailable')
  }
  secure();const directory=await openPersonalPinDirectory(secure)
  try{secure();return new PersonalPinStore(p,binding,authority,{current:secure,encryptString:s=>safeStorage.encryptString(s),decryptString:b=>safeStorage.decryptString(b)},directory)}catch(e){directory.close();throw e}
}
