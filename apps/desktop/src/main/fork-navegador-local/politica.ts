const origemFixture = 'https://pagina.example.invalid'

export type PedidoPagina = 'navegacao' | 'redirecionamento' | 'subrecurso'

export function permiteUrlFixture(tipo: PedidoPagina, endereco: string): boolean {
  const url = URL.parse(endereco)
  if (!url || url.origin !== origemFixture || url.href !== endereco || url.username || url.password) {
    return false
  }
  switch (tipo) {
    case 'navegacao':
    case 'redirecionamento':
      return endereco === `${origemFixture}/pagina`
    case 'subrecurso':
      return endereco === `${origemFixture}/imagem.png`
    default:
      return false
  }
}

export function guardasFixture(ativa: () => boolean) {
  return Object.freeze({
    permiteUrl: (tipo: PedidoPagina, endereco: string) => ativa() && permiteUrlFixture(tipo, endereco),
    permitePermissao: () => false,
    permiteDispositivo: () => false,
    permiteJanela: () => false,
    permiteDownload: () => false,
  })
}
