// Adaptado de Codync 6dcee39da4e475d233bc9c245a9505bd0c10f27b, por leepokai
// e coautoria histórica Claude Opus 5.5. Licença Apache-2.0; veja THIRD_PARTY.md.
type Picked = { name: string; data: Uint8Array }

export async function readPicked(files: File[]): Promise<Picked[]> {
  const results = await Promise.all(files.map(async (file): Promise<Picked | null> => {
    try {
      return { name: file.name || `pasted-${Date.now()}.png`, data: new Uint8Array(await file.arrayBuffer()) }
    } catch {
      return null
    }
  }))
  return results.filter((file): file is Picked => file !== null)
}

export async function readPasted(files: File[], native: () => Promise<Picked[]>): Promise<Picked[]> {
  try {
    const picked = await native()
    if (picked.length) return picked
  } catch { /* A falha nativa não impede a leitura dos arquivos capturados. */ }
  return readPicked(files)
}
