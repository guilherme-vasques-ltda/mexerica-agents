/** Abas das Configurações do bot que a Open mostra ao dono local: só o que roda no Hub isolado, em texto. Comportamento (pasta, autonomia, navegador, computador), Ferramentas (conectores, logins), WhatsApp e Voz dependem de shell, controle do computador, credenciais ou conta e ficam fora. */
export const ABAS_DA_OPEN = ['identity', 'engine', 'memory', 'notifications', 'export'] as const

export const abaDaOpen = (id: string) => (ABAS_DA_OPEN as readonly string[]).includes(id)
