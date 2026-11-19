import { VOZ_PROVEDORES_APARELHO, type CategoriaChave, type EstadoChaves, type VozBridge } from '@shared/fork-voz'

// voz-04, só na demo: um cofre em memória, sem provedor nem rede. Uma chave que começa com `demo-` passa
// na conferência; qualquer outra é recusada como o provedor recusaria. Nada é gravado em disco.

export function demoVozBridge(): VozBridge {
  const guardadas = new Set<string>()
  const ouvintes = new Set<() => void>()
  const estado = (): EstadoChaves => ({
    cofre: 'seguro',
    chaves: Object.fromEntries(VOZ_PROVEDORES_APARELHO.map((p) => [p, { situacao: guardadas.has(p) ? 'guardada' : 'ausente', reserva: false }])) as EstadoChaves['chaves'],
  })
  const avisar = () => ouvintes.forEach((f) => f())
  const r = (ok: boolean, categoria: CategoriaChave | null) => Promise.resolve({ ok, categoria, estado: estado() })
  return {
    catalogoXai: async () => ({ ok: false, categoria: 'sem-chave' }),
    credencialXai: async () => ({ ok: false, categoria: 'sem-chave' }),
    conferirSessaoXai: async () => ({ ok: false, categoria: 'sem-chave' }),
    fecharSessaoXai: async () => ({ ok: true }),
    estado: async () => estado(),
    guardar: (p, chave) => {
      if (!chave.startsWith('demo-')) return r(false, 'chave')
      guardadas.add(p); avisar(); return r(true, null)
    },
    limpar: (p) => { guardadas.delete(p); avisar(); return r(true, null) },
    testar: (p) => r(guardadas.has(p), guardadas.has(p) ? null : 'sem-chave'),
    usarReserva: () => r(false, 'invalida'),
    credencial: async () => ({ ok: false, categoria: 'provedor' }),
    onChange: (f) => { ouvintes.add(f); return () => void ouvintes.delete(f) },
  }
}
