/** Valor real normalizado. Ausência/NaN/infinito não viram porcentagem inventada. */
export function progressoDeterminado(value: number): number | undefined {
  return Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : undefined
}
