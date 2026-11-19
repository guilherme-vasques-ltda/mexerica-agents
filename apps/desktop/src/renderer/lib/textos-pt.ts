// Donnabot: textos do upstream que aparecem nas linhas de bot e no topo da conversa, em pt-BR.
// O upstream continua devolvendo os nomes em inglês (`builtInBackendName`, `folderName`).

const backends: Record<string, string> = {
  'Custom agent': 'Agente personalizado',
}

/** "Custom agent" → "Agente personalizado"; os nomes próprios (Claude Code, Codex…) ficam. */
export const ptBackend = (name: string): string => backends[name] ?? name

/** "Personal workspace" → "Espaço pessoal"; uma pasta de projeto fica com o nome dela. */
export const ptFolder = (name: string): string => (name === 'Personal workspace' ? 'Espaço pessoal' : name)
