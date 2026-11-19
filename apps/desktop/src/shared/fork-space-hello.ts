/** Capacidades anunciadas pelo Hub no hello. Ausência significa versão zero. */
export interface SpaceHello {
  spaceBackgroundContract?: number
  spaceUsageContract?: number
  spaceStorageContract?: number
  spaceEngineAccountsContract?: number
  spaceMemoryProfileContract?: number
  spaceDelegationContract?: number
}
