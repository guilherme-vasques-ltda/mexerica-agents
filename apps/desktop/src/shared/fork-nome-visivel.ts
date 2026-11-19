// D443 (antes D343/D348): o nome que a pessoa lê é "Mexerica". Desde o PR (d) o pacote também se chama Mexerica
// (`productName`: Mexerica.app, Dock, Finder, Ajustes do Sistema). O nome técnico, o que `app.getName()` devolve
// (`app.setName` em main/environment.ts), continua "Donnabot": pasta de dados, Keychain ("Donnabot Safe Storage"),
// trava de instância única e o nome do aparelho na nuvem dependem dele. Por isso a tela ainda traduz o nome aqui.

/** "Donnabot" → "Mexerica", "Donnabot Dev" → "Mexerica Dev"; outro nome fica como veio. */
export const nomeVisivel = (appName: string): string => appName.replace(/^Donnabot(?=$| )/, 'Mexerica')
