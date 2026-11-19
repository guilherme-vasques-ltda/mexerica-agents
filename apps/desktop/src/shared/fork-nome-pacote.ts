// D443 (PR d): o nome do pacote instalado. O `productName` passou a "Mexerica" ("Mexerica Dev"), então o bundle é
// Mexerica.app; o nome técnico (`identity.name`, `app.setName`) continua "Donnabot" e com ele a pasta de dados e o
// item do Chaveiro. Quem procura um bundle pelo nome (o ajudante de tela de outra variante, o donnabot-host num
// computador remoto) olha os dois: o novo primeiro e o antigo enquanto houver instalação de antes ou o rollback.

/** Os nomes de `<nome>.app` desta variante, do mais novo para o mais antigo. */
export function pacotesDoApp(environment: 'main' | 'dev'): readonly string[] {
  // textos-ok: nomes de pasta `.app` no disco (caminho de bundle), não texto de tela.
  return environment === 'dev' ? ['Mexerica Dev', 'Donnabot Dev'] : ['Mexerica', 'Donnabot']
}
