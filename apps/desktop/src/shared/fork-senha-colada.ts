// MX-214 (parte da MX-146): detector LOCAL de senha, chave ou token colado na caixa de mensagem.
// Puro: sem rede, sem log, sem estado, sem leitura da área de transferência. Recebe o texto que a pessoa acabou de
// colar e devolve onde está o trecho suspeito (ou null). O texto nunca sai desta função: quem chama decide o que
// fazer, e nunca grava o trecho em rascunho, histórico, espelho, analytics nem relatório de erro.

export type TipoSegredo = 'chave-privada' | 'prefixo-conhecido' | 'alta-entropia'

/** Posição do trecho dentro do texto colado: `texto.slice(inicio, fim)` é o segredo. */
export interface TrechoSegredo {
  inicio: number
  fim: number
  tipo: TipoSegredo
}

// Prefixos conhecidos. Cada regra vale para o trecho inteiro (âncora no começo e no fim do token) e o corpo ainda
// precisa parecer aleatório, para "sk-learn-algo-comprido" ou "gh-pages" não dispararem.
const PREFIXOS: readonly RegExp[] = [
  /sk-ant-[A-Za-z0-9_-]{20,}/y,
  /sk-[A-Za-z0-9_-]{20,}/y,
  /gh[pousr]_[A-Za-z0-9]{30,}/y,
  /github_pat_[A-Za-z0-9_]{22,}/y,
  /xox[bpoas]-[A-Za-z0-9-]{10,}/y,
  /xapp-[A-Za-z0-9-]{20,}/y,
  // Stripe (chave secreta, restrita e segredo de webhook), npm, PyPI, Hugging Face, SendGrid, Shopify, Google OAuth,
  // DigitalOcean, RubyGems. Cada um vale em qualquer lugar do texto (atribuição, .env de várias linhas).
  /(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}/y,
  /whsec_[A-Za-z0-9]{24,}/y,
  /npm_[A-Za-z0-9]{36}/y,
  /pypi-[A-Za-z0-9_-]{50,}/y,
  /hf_[A-Za-z0-9]{30,}/y,
  /SG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}/y,
  /shp(?:at|ca|pa|ss)_[A-Za-z0-9]{32}/y,
  /ya29\.[A-Za-z0-9_-]{30,}/y,
  /dop_v1_[A-Za-z0-9]{64}/y,
  /rubygems_[A-Za-z0-9]{48}/y,
  /(?:AKIA|ASIA)[0-9A-Z]{16}/y,
  /AIza[0-9A-Za-z_-]{35}/y,
  /glpat-[A-Za-z0-9_-]{20,}/y,
  /eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}/y,
]

// Montadas por partes para o próprio arquivo não parecer uma chave privada ao Gitleaks.
const COMECO_CHAVE_PRIVADA = new RegExp('-{5}BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-{5}')
const FIM_CHAVE_PRIVADA = new RegExp('-{5}END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-{5}')

const MIN_ENTROPIA = 16
const LIMITE_BITS = 3.4

/** Entropia de Shannon em bits por caractere. */
export function entropia(texto: string): number {
  const total = [...texto].length
  if (!total) return 0
  const contagem = new Map<string, number>()
  for (const c of texto) contagem.set(c, (contagem.get(c) ?? 0) + 1)
  let bits = 0
  for (const n of contagem.values()) bits -= (n / total) * Math.log2(n / total)
  return bits
}

const classes = (t: string) => ({
  minuscula: /[a-z]/.test(t),
  maiuscula: /[A-Z]/.test(t),
  digito: /[0-9]/.test(t),
  simbolo: /[^A-Za-z0-9_.\-/]/.test(t),
})

/** Corpo de uma chave com prefixo: precisa de mistura e de entropia, senão é só uma frase com hífen. */
function pareceAleatorio(corpo: string): boolean {
  const c = classes(corpo)
  const tipos = [c.minuscula, c.maiuscula, c.digito].filter(Boolean).length
  return tipos >= 2 && entropia(corpo) >= 3.0
}

/** Todos os blocos de chave privada do texto (um bloco sem a linha final vai até o fim do que veio). */
function chavesPrivadas(texto: string): TrechoSegredo[] {
  const achados: TrechoSegredo[] = []
  let de = 0
  for (;;) {
    const comeco = COMECO_CHAVE_PRIVADA.exec(texto.slice(de))
    if (!comeco) return achados
    const inicio = de + comeco.index
    const fim = FIM_CHAVE_PRIVADA.exec(texto.slice(inicio))
    const acaba = fim ? inicio + fim.index + fim[0].length : texto.length
    achados.push({ inicio, fim: acaba, tipo: 'chave-privada' })
    de = acaba
  }
}

/** Todas as chaves com prefixo conhecido, em qualquer ponto do texto. */
function prefixosConhecidos(texto: string): TrechoSegredo[] {
  const achados: TrechoSegredo[] = []
  // Cada token começa depois de algo que não é letra, dígito, "_" ou "-" (espaço, aspas, "=", ":", "/"…).
  const inicios = /(?<![A-Za-z0-9_-])(?=[A-Za-z])/g
  for (let m = inicios.exec(texto); m; m = inicios.exec(texto)) {
    inicios.lastIndex = m.index + 1 // casamento de largura zero não avança sozinho
    for (const regra of PREFIXOS) {
      regra.lastIndex = m.index
      const achou = regra.exec(texto)
      if (!achou) continue
      const fim = m.index + achou[0].length
      // O trecho tem de acabar num limite: "sk-…aaaa-bbbb" inteiro, não um começo de palavra maior.
      if (/[A-Za-z0-9_-]/.test(texto[fim] ?? '')) continue
      const corpo = achou[0].replace(/^[A-Za-z_]+[-_]/, '')
      if (!achou[0].startsWith('eyJ') && !pareceAleatorio(corpo)) continue
      achados.push({ inicio: m.index, fim, tipo: 'prefixo-conhecido' })
      inicios.lastIndex = fim
      break
    }
  }
  return achados
}

const URL_OU_DOMINIO = /^(?:[a-z][a-z0-9+.-]*:\/\/|www\.|[a-z0-9-]+(?:\.[a-z0-9-]+)+(?:[/:?#]|$))/i
const EMAIL = /^[^\s@]+@[^\s@]+\.[A-Za-z]{2,}$/
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
const SO_HEX = /^[0-9a-f]+$/i
const COMECO_CAMINHO = /^(?:~[/\\]|\.{1,2}[/\\]|\/|[A-Za-z]:[\\/]|\\\\)/
// Nome de pasta ou arquivo: só a primeira letra pode ser maiúscula ("Composer.tsx", "node_modules", ".github").
const SEGMENTO_NOME = /^\.?[A-Za-z]?[a-z0-9._-]*$/

function pareceCaminho(t: string): boolean {
  if (!/[/\\]/.test(t)) return false
  const partes = t.replace(COMECO_CAMINHO, '').split(/[/\\]+/).filter(Boolean)
  if (!partes.length) return true
  return partes.every((p) => SEGMENTO_NOME.test(p)) || (COMECO_CAMINHO.test(t) && /\.[A-Za-z0-9]{1,6}$/.test(t) && !/[A-Z].*[0-9].*[A-Z]|[0-9].*[A-Z].*[0-9]/.test(partes.at(-1)!))
}

/** Palavras coladas em camelCase/snake_case ("useSyncExternalStore2", "get_user_by_id_v2") não são senha. */
function pareceIdentificador(t: string): boolean {
  const pedacos = t.split(/[_.-]+/).filter(Boolean)
  if (pedacos.length > 1 && pedacos.every((p) => /^[a-z]+[0-9]{0,3}$/i.test(p) && !/[a-z][A-Z]{2}/.test(p))) {
    return pedacos.some((p) => p.length >= 3)
  }
  // camelCase: só letras, maiúscula sempre seguida de minúscula.
  if (/^[A-Za-z]+[0-9]{0,3}$/.test(t) && !/[A-Z]{2}/.test(t)) return true
  return false
}

/** Trecho isolado, sem espaço, longo, de alta entropia e com mistura de classes. */
function altaEntropia(texto: string): TrechoSegredo | null {
  const aparado = texto.trim()
  if (!aparado || /\s/.test(aparado)) return null
  let inicio = texto.indexOf(aparado)
  let candidato = aparado
  // "CHAVE=valor": o segredo é o valor.
  const atribuicao = /^[A-Za-z_][A-Za-z0-9_.]*=(.+)$/.exec(candidato)
  if (atribuicao) {
    inicio += candidato.length - atribuicao[1]!.length
    candidato = atribuicao[1]!
  }
  // Aspas ao redor não fazem parte do segredo.
  const aspas = /^(["'`])(.+)\1$/.exec(candidato)
  if (aspas) { inicio += 1; candidato = aspas[2]! }
  if (candidato.length < MIN_ENTROPIA) return null
  if (URL_OU_DOMINIO.test(candidato) || EMAIL.test(candidato) || UUID.test(candidato) || SO_HEX.test(candidato)) return null
  if (pareceCaminho(candidato) || pareceIdentificador(candidato)) return null
  const c = classes(candidato)
  const mistura = [c.minuscula, c.maiuscula, c.digito, c.simbolo].filter(Boolean).length
  if (mistura < 3) return null
  if (entropia(candidato) < LIMITE_BITS) return null
  return { inicio, fim: inicio + candidato.length, tipo: 'alta-entropia' }
}

/**
 * TODOS os trechos que parecem senha, chave ou token no texto colado, em ordem e sem sobreposição. Chave privada e
 * prefixo conhecido valem em qualquer texto (inclusive um .env de várias linhas); o trecho isolado de alta entropia
 * só vale quando o texto colado inteiro é um único trecho sem espaço (código e frases comuns nunca disparam por ele).
 */
export function acharSegredosColados(texto: string): TrechoSegredo[] {
  if (!texto) return []
  const todos = [...chavesPrivadas(texto), ...prefixosConhecidos(texto)]
  const unico = altaEntropia(texto)
  if (unico && !todos.length) todos.push(unico)
  todos.sort((a, b) => a.inicio - b.inicio || b.fim - a.fim)
  const unidos: TrechoSegredo[] = []
  for (const t of todos) {
    const ultimo = unidos.at(-1)
    if (ultimo && t.inicio < ultimo.fim) ultimo.fim = Math.max(ultimo.fim, t.fim)
    else unidos.push({ ...t })
  }
  return unidos
}

/** O primeiro trecho que parece senha (ou null). */
export function acharSegredoColado(texto: string): TrechoSegredo | null {
  return acharSegredosColados(texto)[0] ?? null
}
