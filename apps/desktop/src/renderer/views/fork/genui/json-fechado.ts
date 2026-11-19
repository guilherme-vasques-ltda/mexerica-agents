/** Resultado sem eco do JSON recebido. O consumidor aplica seu limite bruto antes deste helper. */
export type ClosedJson = { ok: true; value: unknown } | { ok: false; error: 'genui_invalid_payload' }

export interface ClosedJsonOptions {
  /** Nomes decodificados, em qualquer objeto/profundidade, cujo valor bruto deve ser literal uint. */
  unsignedIntegerFields?: readonly string[]
}

/**
 * JSON padrão, recusando chaves duplicadas em qualquer objeto, inclusive nomes escapados equivalentes.
 * JSON.parse valida toda a sintaxe; a varredura iterativa acrescenta duplicidade e, por opção explícita,
 * forma lexical uint (só dígitos, sem sinal, fração ou expoente). Demais números ficam intactos.
 * Não valida catálogo, faixa ou presença: cada consumidor ainda confere forma fechada, tipos e limites.
 * Não recupera forma lexical de objetos já decodificados. Sem reviver, execução de strings, recursão,
 * I/O ou dependências. Autoria: Codex Sol, correções R1/R2.
 */
export function parseClosedJson(raw: string, options: ClosedJsonOptions = {}): ClosedJson {
  const invalid = { ok: false, error: 'genui_invalid_payload' } as const
  try {
    const value: unknown = JSON.parse(raw)
    const uintFields = new Set(options.unsignedIntegerFields ?? [])
    // Cada objeto tem seu Set; arrays só delimitam o escopo. Estruturas dentro de strings não contam.
    const stack: (Set<string> | null)[] = []
    for (let i = 0; i < raw.length; i++) {
      const char = raw[i]
      if (char === '{') stack.push(new Set())
      else if (char === '[') stack.push(null)
      else if (char === '}' || char === ']') stack.pop()
      else if (char === '"') {
        const start = i
        // O JSON já é válido: cada escape tem o próximo caractere e toda string tem fechamento.
        for (i++; i < raw.length; i++) {
          if (raw[i] === '\\') i++
          else if (raw[i] === '"') break
        }
        let next = i + 1
        while (next < raw.length && /[ \t\r\n]/.test(raw[next]!)) next++
        if (raw[next] === ':') {
          const keys = stack[stack.length - 1]
          if (!keys) return invalid
          const key: string = JSON.parse(raw.slice(start, i + 1))
          if (keys.has(key)) return invalid
          keys.add(key)
          if (uintFields.has(key)) {
            let startValue = next + 1
            while (startValue < raw.length && /[ \t\r\n]/.test(raw[startValue]!)) startValue++
            let endValue = startValue
            while (endValue < raw.length && raw[endValue]! >= '0' && raw[endValue]! <= '9') endValue++
            // Sintaxe já validada: basta exigir dígitos seguidos de delimitador JSON ou fim.
            if (endValue === startValue || (endValue < raw.length && !/[ \t\r\n,}\]]/.test(raw[endValue]!))) return invalid
          }
        }
      }
    }
    return { ok: true, value }
  } catch {
    return invalid
  }
}
