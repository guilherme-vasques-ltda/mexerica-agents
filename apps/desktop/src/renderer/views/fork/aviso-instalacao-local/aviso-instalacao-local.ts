/** O aviso vale só para o Hub deste computador: sem sinal de que é ele, não aparece. */
export function ehHubDesteComputador(local: { computer: { id: string } } | null, store: { computer: { id: string } }): boolean {
  return local !== null && local.computer.id === store.computer.id
}
