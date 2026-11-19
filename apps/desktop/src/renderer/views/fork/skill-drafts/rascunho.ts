/** Dados externos passivos: nunca destino, comando, ferramenta ou aprovação. */
export type RascunhoSkill = Readonly<{ name: string; description: string; instructions: string }>
export type IndicacaoSkill = Readonly<{ tipo: 'indicacao'; reason: string; reference: string }>
export type PropostaSkill = (RascunhoSkill & ({ tipo: 'rascunho' } | { tipo: 'recomendacao'; reason: string })) | IndicacaoSkill
const encoder = new TextEncoder()
export const LIMITES_RASCUNHO = { mensagem: 128 * 1024, envelope: 64 * 1024, instrucoes: 32 * 1024 } as const
const bytes = (s: string) => encoder.encode(s).byteLength

function lerEnvelope(texto: string, tipo: PropostaSkill['tipo']): PropostaSkill | null {
  if (bytes(texto) > LIMITES_RASCUNHO.envelope) return null
  try {
    const valor: unknown = JSON.parse(texto)
    if (!valor || typeof valor !== 'object' || Array.isArray(valor)) return null
    const v = valor as Record<string, unknown>
    const indicacao = tipo === 'indicacao'
    const recomendacao = tipo === 'recomendacao'
    if (Object.keys(v).sort().join(',') !== (indicacao ? 'reason,reference,schema' : recomendacao ? 'description,instructions,name,reason,schema' : 'description,instructions,name,schema') || v.schema !== 1) return null
    if (indicacao && (typeof v.reference !== 'string' || !v.reference.trim() || [...v.reference].length > 256)) return null
    if (!indicacao && (typeof v.name !== 'string' || !v.name.trim() || [...v.name].length > 64
      || typeof v.description !== 'string' || !v.description.trim() || [...v.description].length > 1024
      || typeof v.instructions !== 'string' || !v.instructions.trim() || bytes(v.instructions) > LIMITES_RASCUNHO.instrucoes)) return null
    if ((recomendacao || indicacao) && (typeof v.reason !== 'string' || !v.reason.trim() || [...v.reason].length > 1024)) return null
    // JSON.parse já validou a sintaxe. Contar chaves no nível raiz também recusa duplicadas.
    let nivel = 0
    let chaves = 0
    for (const token of texto.matchAll(/"(?:[^"\\]|\\.)*"|[{}\[\]]/g)) {
      if (token[0] === '{' || token[0] === '[') nivel++
      else if (token[0] === '}' || token[0] === ']') nivel--
      else if (nivel === 1 && /^\s*:/.test(texto.slice(token.index + token[0].length))) chaves++
    }
    if (chaves !== (indicacao ? 3 : recomendacao ? 5 : 4)) return null
    if (indicacao) return { tipo: 'indicacao', reason: v.reason as string, reference: v.reference as string }
    const corpo = { name: v.name as string, description: v.description as string, instructions: v.instructions as string }
    return recomendacao ? { ...corpo, tipo: 'recomendacao', reason: v.reason as string } : { ...corpo, tipo: 'rascunho' }
  } catch { return null }
}

/** Extração limitada de fences; não interpreta Markdown nem altera o texto exibido. */
export function extrairProposta(texto: string): PropostaSkill | null {
  // Primeiro o comprimento barato: não codificar mensagem ilimitada para conferir bytes.
  if (texto.length > LIMITES_RASCUNHO.mensagem || bytes(texto) > LIMITES_RASCUNHO.mensagem) return null
  let fence: { marca: string; tamanho: number; inicio: number; proposta: PropostaSkill['tipo'] | null } | null = null
  let propostas = 0
  let resultado: PropostaSkill | null = null
  const linhas = texto.split(/\r?\n/)
  for (let i = 0; i < linhas.length; i++) {
    const linha = linhas[i]!
    if (fence) {
      const fim = /^ {0,3}(`{3,}|~{3,})[ \t]*$/.exec(linha)
      if (fim && fim[1]![0] === fence.marca && fim[1]!.length >= fence.tamanho) {
        if (fence.proposta) resultado = lerEnvelope(linhas.slice(fence.inicio, i).join('\n'), fence.proposta)
        fence = null
      }
      continue
    }
    // Uma proposta só começa na coluna zero; recusa blocos indentados e citações.
    const abertura = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(linha)
    if (!abertura || (abertura[1]![0] === '`' && abertura[2]!.includes('`'))) continue
    const proposta = !linha.startsWith('`') ? null : abertura[2] === 'donna-skill-draft' ? 'rascunho'
      : abertura[2] === 'donna-skill-recommendation' ? 'recomendacao' : abertura[2] === 'donna-skill-reference' ? 'indicacao' : null
    if (proposta && ++propostas > 1) return null
    fence = { marca: abertura[1]![0]!, tamanho: abertura[1]!.length, inicio: i + 1, proposta }
  }
  if (fence?.proposta) return null
  return propostas === 1 ? resultado : null
}

/** API de119A: formato b1 não vira rascunho a; mistura de propostas também não. */
export function extrairRascunho(texto: string): RascunhoSkill | null {
  const proposta = extrairProposta(texto)
  return proposta?.tipo === 'rascunho' ? { name: proposta.name, description: proposta.description, instructions: proposta.instructions } : null
}
