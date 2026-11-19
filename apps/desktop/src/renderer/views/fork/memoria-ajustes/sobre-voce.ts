// MX-8: "Sobre você" em Ajustes → Memória. Uma nota compartilhada (camada global da memória) com cinco seções fixas.
// Contrato que a tela espera do Hub (PR de Hub separado, depois do OK do dono; hoje só o fake da demo responde):
//   memoryProfile {}                      → { content: string | null, revision: string | null, loose: number }
//   saveMemoryProfile { content, base }   → a mesma resposta; `content: null` apaga; `base` diferente → memory_profile_conflict
// Sem DOM nem React.
import { Observable } from '../../../lib/observable.ts'

export const CAMPOS = ['quem', 'pensa', 'faz', 'fala', 'evitar'] as const
export type Campo = (typeof CAMPOS)[number]
export type Perfil = Record<Campo, string>

// Formato do arquivo lido pelos bots, igual em todos os idiomas (não é rótulo da tela).
const TITULOS: Record<Campo, string> = {
  quem: 'Quem é', // textos-ok: título fixo da nota
  pensa: 'Como pensa', // textos-ok: título fixo da nota
  faz: 'O que faz', // textos-ok: título fixo da nota
  fala: 'Como fala', // textos-ok: título fixo da nota
  evitar: 'Prefiro que evite', // textos-ok: título fixo da nota (decisão do dono, 11/11)
}
/** Títulos de antes, ainda lidos no mesmo campo (notas gravadas antes da troca de nome). */
const TITULOS_ANTIGOS: Partial<Record<Campo, readonly string[]>> = {
  evitar: ['O que não fazer'], // textos-ok: título antigo da nota, só para leitura
}
const CABECA = '# Sobre você' // textos-ok: cabeçalho fixo da nota
/** Marca do formato com escape, na primeira linha. Sem ela a nota é legada: lida literalmente, sem desescapar. */
export const MARCA = '<!-- mexerica:sobre-voce v1 -->'

export const vazio = (): Perfil => ({ quem: '', pensa: '', faz: '', fala: '', evitar: '' })

// Escape (só no formato marcado): linha de campo que começa com `#` (com ou sem `\` antes) ganha um `\` na frente
// ao gravar e perde um ao ler. Assim nenhum texto do dono vira título, e a ida e volta é exata (campos aparados).
// Nota legada (sem MARCA) é lida como antes, literal: `\#tag` continua `\#tag` e é regravada escapada.
const escapar = (linha: string) => (/^\\*#/.test(linha) ? `\\${linha}` : linha)
const desescapar = (linha: string) => (/^\\+#/.test(linha) ? linha.slice(1) : linha)

/** A nota → os cinco campos. Título repetido (nota editada à mão) junta o texto no mesmo campo; texto fora das
 * cinco seções vai para "Quem é". Nada some da tela. */
export function lerPerfil(content: string | null): Perfil {
  const perfil = vazio()
  if (!content) return perfil
  const porTitulo = new Map<string, Campo>(CAMPOS.flatMap((c) => [TITULOS[c], ...(TITULOS_ANTIGOS[c] ?? [])].map((t) => [t.toLowerCase(), c] as const)))
  const solto: string[] = []
  let atual: Campo | null = null
  let inicio = true
  const partes: Record<Campo, string[][]> = { quem: [], pensa: [], faz: [], fala: [], evitar: [] }
  const linhas = content.replace(/\r\n?/g, '\n').split('\n')
  const marcada = linhas[0]?.trim() === MARCA
  if (marcada) linhas.shift()
  for (const linha of linhas) {
    const titulo = /^##[ \t]+(.+?)[ \t]*$/.exec(linha)
    const campo = titulo ? porTitulo.get(titulo[1]!.toLowerCase()) : undefined
    if (campo) { atual = campo; partes[campo].push([]); inicio = false; continue }
    if (inicio && linha.trim() === CABECA) { inicio = false; continue }
    if (linha.trim()) inicio = false
    const limpa = marcada ? desescapar(linha) : linha
    if (atual) partes[atual].at(-1)!.push(limpa)
    else solto.push(limpa)
  }
  for (const c of CAMPOS) perfil[c] = partes[c].map((p) => p.join('\n').trim()).filter(Boolean).join('\n\n')
  const resto = solto.join('\n').trim()
  if (resto) perfil.quem = perfil.quem ? `${resto}\n\n${perfil.quem}` : resto
  return perfil
}

/** Os cinco campos → a nota. Seção vazia não entra; tudo vazio → null (apaga a nota). */
export function escreverPerfil(perfil: Perfil): string | null {
  const secoes = CAMPOS.filter((c) => perfil[c].trim()).map((c) => `## ${TITULOS[c]}\n${perfil[c].trim().replace(/\r\n?/g, '\n').split('\n').map(escapar).join('\n')}`)
  return secoes.length ? `${MARCA}\n${CABECA}\n\n${secoes.join('\n\n')}\n` : null
}

/** Identidade durável do cartão: computador, Hub e dono (o contexto da conta). Sem a época do transporte nem a
 * conexão: cair e voltar ao mesmo Hub (hello novo, época 1 → 2) não recria o store nem apaga o rascunho. */
export const identidade = (h: { contextId: string; computer: { id: string }; hello: { hostId?: string | null } | null }) =>
  JSON.stringify([h.contextId, h.computer.id, h.hello?.hostId ?? ''])

export const iguais = (a: Perfil, b: Perfil) => CAMPOS.every((c) => a[c].trim() === b[c].trim())

interface Resposta { content: string | null; revision: string | null; loose: number }
export function lerResposta(value: unknown): Resposta {
  const v = value as Partial<Resposta> | null
  if (!v || typeof v !== 'object' || (v.content !== null && typeof v.content !== 'string') || (v.revision !== null && typeof v.revision !== 'string')) throw new Error('profile_invalid')
  return { content: v.content, revision: v.revision, loose: typeof v.loose === 'number' && Number.isSafeInteger(v.loose) && v.loose >= 0 ? v.loose : 0 }
}

// Erros do Hub (#698): a mensagem traz o código (HTTP 400, `{e:#}` pode somar contexto antes); Hub antigo: 404 `unknown method`.
// `secret_detected` não repete o trecho recusado.
export type Problema = 'indisponivel' | 'leitura' | 'gravacao' | 'conflito' | 'segredo'
interface Transport { call<T>(method: string, body?: unknown): Promise<T> }
const mensagem = (error: unknown) => String((error as Error)?.message ?? error)

export class SobreVoceStore extends Observable {
  salvo: Perfil | null = null
  rascunho: Perfil = vazio()
  revisao: string | null = null
  soltas = 0
  problema: Problema | null = null
  ocupado = false
  gravou = false
  /** O último campo editado: recebe o foco quando o Hub recusa um segredo (ele não diz qual trecho). */
  ultimo: Campo | null = null
  private pedido = 0
  private readonly client: () => Promise<Transport>

  constructor(client: () => Promise<Transport>) { super(); this.client = client }

  /** O campo que recebe o foco depois de `secret_detected`: o último editado, senão o primeiro com texto. */
  get campoDoSegredo(): Campo | null {
    return this.problema !== 'segredo' ? null : this.ultimo ?? CAMPOS.find((c) => this.rascunho[c].trim()) ?? null
  }

  get sujo() { return this.salvo !== null && !iguais(this.salvo, this.rascunho) }

  /** Lê do Hub. Com rascunho, só atualiza a contagem: o rascunho e a base dele ficam até o dono descartar
   * (`descartar` ou Recarregar depois de conflito, que passam `trocar`). */
  async load(trocar = false) {
    const pedido = ++this.pedido
    try {
      const r = lerResposta(await (await this.client()).call('memoryProfile', {}))
      if (pedido !== this.pedido) return
      this.soltas = r.loose
      if (trocar || !this.sujo) this.aplicar(r)
      this.problema = null
    } catch (error) {
      if (pedido !== this.pedido) return
      this.problema = /unknown method/i.test(mensagem(error)) ? 'indisponivel' : 'leitura'
    }
    this.changed()
  }

  editar(campo: Campo, texto: string) {
    this.rascunho = { ...this.rascunho, [campo]: texto }
    this.ultimo = campo
    this.gravou = false
    // Mexer no texto depois da recusa tira o aviso; a próxima gravação confere de novo.
    if (this.problema === 'segredo') this.problema = null
    this.changed()
  }

  /** Descarte explícito: volta ao que está salvo; depois de conflito, relê a versão nova. */
  descartar() {
    if (this.problema === 'conflito') { void this.load(true); return }
    if (this.salvo) { this.rascunho = { ...this.salvo }; if (this.problema === 'gravacao' || this.problema === 'segredo') this.problema = null; this.changed() }
  }

  async save() {
    if (this.ocupado || !this.sujo) return
    const pedido = ++this.pedido
    this.ocupado = true
    this.problema = null
    this.changed()
    try {
      const r = lerResposta(await (await this.client()).call('saveMemoryProfile', { content: escreverPerfil(this.rascunho), base: this.revisao }))
      if (pedido === this.pedido) { this.aplicar(r); this.soltas = r.loose; this.gravou = true }
    } catch (error) {
      // Resultado incerto ou conflito: o rascunho fica; a tela oferece Tentar novamente ou Recarregar.
      if (pedido === this.pedido) {
        const m = mensagem(error)
        this.problema = m.includes('memory_profile_conflict') ? 'conflito' : m.includes('secret_detected') ? 'segredo' : 'gravacao'
      }
    } finally {
      this.ocupado = false
      this.changed()
    }
  }

  private aplicar(r: Resposta) {
    this.salvo = lerPerfil(r.content)
    this.rascunho = { ...this.salvo }
    this.revisao = r.revision
    this.soltas = r.loose
  }
}
