// Replaced with a literal by electron-vite: true only for a complete, matching Dev or qualified main
// release (`admitsCloudBuild`). The legacy name is kept so the gate tests keep their fixture.
declare const __DONNA_DEV_CLOUD_ACCESS__: boolean
declare const __MEXERICA_OFFICIAL__: boolean
declare const __MEXERICA_MAC_LOCAL_HUB__: boolean

export const officialDesktop = typeof __MEXERICA_OFFICIAL__ !== 'undefined' && __MEXERICA_OFFICIAL__
export const bundledMacLocalHub = typeof __MEXERICA_MAC_LOCAL_HUB__ !== 'undefined' && __MEXERICA_MAC_LOCAL_HUB__

/** Nenhuma preferência ou variável de ambiente do app reativa estes cortes. */
export const forkFeatures = {
  updates: false,
  analytics: false,
  cloud: false,
  cloudAccess: typeof __DONNA_DEV_CLOUD_ACCESS__ !== 'undefined' && __DONNA_DEV_CLOUD_ACCESS__,
  /** Instalar/iniciar um host neste computador sem a escolha do usuário (tela "Acesso ao computador"); a D76 deixou de valer, ver `macLocalHub`. */
  localHost: false,
  /**
   * Hub local do Mac ("Neste Mac"): constante de build, falsa por padrão; o app só administra o Hub
   * local com a escolha `hubLocation` do usuário. Sem exceção de plataforma aqui: o controlador só a
   * usa no macOS. `localHost` segue falso por ser a chave da tela "Acesso ao computador".
   */
  macLocalHub: bundledMacLocalHub,
}

export function showsAccountControls(): boolean {
  return officialDesktop || forkFeatures.cloudAccess
}

/**
 * Se o app mostra "Computer access" (o Donnabot Screen deste computador): a etapa do primeiro uso,
 * a página de Ajustes, o item do menu da barra e o comando de janela. Sem host local (D76) o app é
 * só cliente do Hub e não há o que configurar aqui: tudo fica escondido enquanto `localHost` for
 * `false`, mesmo com um host de desenvolvimento (`DONNABOT_PORT`). A condição é só `localHost`, e
 * ligá-lo também instala e inicia o host neste computador: para trazer a tela com o ajudante do Mac
 * (D83, futura tela Computadores) com os bots ainda só no Hub, esta função vai precisar de uma chave
 * própria. `host` é o estado do host deste computador, quando quem pergunta o conhece; o código do
 * upstream fica intacto. O interruptor "Usar o computador" dos bots não depende daqui: segue a tela
 * que o computador dos bots informa (`renderer/views/fork/computador/estado.ts`, D240).
 */
export function showsComputerAccess(host?: { kind: string }): boolean {
  return (forkFeatures.localHost || bundledMacLocalHub) && host?.kind !== 'disabled'
}
