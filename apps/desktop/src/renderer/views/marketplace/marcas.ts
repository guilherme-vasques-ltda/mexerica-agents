// Sem logos de terceiros neste recorte: todo serviço do catálogo usa o globo neutro.
// Para liberar o logo de um serviço, é preciso antes comprovar o direito de uso (ver pendências de direitos do recorte).
export const MARCAS: Readonly<Record<string, string>> = {}

/** O arquivo da `marca` dada, ou `null` (o chamador usa o globo). Neste recorte, sempre `null`. */
export const arquivoDaMarca = (marca: string | null | undefined): string | null =>
  marca && Object.hasOwn(MARCAS, marca) ? MARCAS[marca] : null
