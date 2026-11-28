/**
 * Tira dos favoritos os motores que o Hub confirma como desinstalados (`installed === false`). Só vale depois de o
 * Hub informar a lista de motores; motor ausente da lista ou sem a informação fica, e a mesma lista volta quando nada sai.
 */
export function semDesinstalados(favoritos: string[], backends: ReadonlyArray<{ id: string; installed?: boolean }>): string[] {
  const fora = new Set(backends.filter((b) => b.installed === false).map((b) => b.id))
  if (!fora.size) return favoritos
  const resto = favoritos.filter((k) => !fora.has(k.split('/')[0]!))
  return resto.length === favoritos.length ? favoritos : resto
}
