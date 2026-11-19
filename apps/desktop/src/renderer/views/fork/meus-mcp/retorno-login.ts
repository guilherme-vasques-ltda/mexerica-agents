/**
 * M3-B: campos do retorno do login de um MCP remoto (`codync://oauth?…`) levados ao Hub,
 * que confere o emissor (`iss`). A query é decodificada uma vez por `URLSearchParams`;
 * nenhum valor é aparado ou normalizado, e campo repetido recusa em vez de escolher o primeiro.
 */
export const RETORNO_MALFORMADO = 'O retorno do login veio malformado; login recusado. Inicie o login novamente.'

export type RetornoLogin = { state: string; code: string | null; error: string | null; iss: string | null }

const CAMPOS = ['state', 'code', 'error', 'error_description', 'iss'] as const

export function lerRetornoLogin(volta: string): RetornoLogin {
  if (!URL.canParse(volta)) throw new Error(RETORNO_MALFORMADO)
  const q = new URL(volta).searchParams
  if (CAMPOS.some((campo) => q.getAll(campo).length > 1)) throw new Error(RETORNO_MALFORMADO)
  return { state: q.get('state') ?? '', code: q.get('code'), error: q.get('error_description') ?? q.get('error'), iss: q.get('iss') }
}
