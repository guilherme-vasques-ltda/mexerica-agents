// As abas de cada conversa só em memória, como no protótipo: a sessão aberta e as abas fechadas com "×"
// (a sessão continua no painel). Sobrevivem à troca de conversa, não ao reinício do app.
const memory = new Map<string, { open: string | null; closed: Set<string> }>()

export function tabMemory(chatId: string) {
  let m = memory.get(chatId)
  if (!m) memory.set(chatId, (m = { open: null, closed: new Set() }))
  return m
}

/** Dois cliques medidos pelo intervalo (400 ms), não pelo `dblclick`: o primeiro clique redesenha a barra. */
export function doubleClicks(ms = 400) {
  let last: { key: string; at: number } | null = null
  return (key: string, now = performance.now()) => {
    const double = !!last && last.key === key && now - last.at < ms
    last = double ? null : { key, at: now }
    return double
  }
}
