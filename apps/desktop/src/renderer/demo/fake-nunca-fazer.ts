// MX-111 (fase 1), só na demo: o "Nunca fazer" em memória com o contrato do Hub (#700, host/src/fork/nunca_fazer.rs).
// `?nunca=cheio` semeia o Space Pessoal e a Prosa; `?nunca=conflito` faz a próxima escrita perder para "outra janela";
// `?nunca=antigo` responde como um Hub sem as RPCs (404). Dados fictícios.
import type { CallError } from '@shared/ipc'

const METODOS = ['nuncaFazer', 'nuncaFazerAdicionar', 'nuncaFazerEditar', 'nuncaFazerRemover', 'nuncaFazerReordenar']
const CAMPOS = new Set(['spaceId', 'botId', 'hubId', 'id', 'texto', 'ids', 'expectedRevision'])
const cena = () => new URLSearchParams(location.search).get('nunca')

interface Linha { id: string; texto: string; criadoEm: number; atualizadoEm: number }
const erro = (status: number, message: string): CallError => ({ status, message })

export class FakeNuncaFazer {
  private listas = new Map<string, { revision: number; itens: Linha[] }>()
  private rev = 0
  private seq = 0
  private conflitoPendente = cena() === 'conflito'

  constructor() {
    if (cena() !== 'cheio' && cena() !== 'conflito') return
    const pessoal = '00000000-0000-4000-8000-000000000001'
    this.semear(pessoal, '', ['nunca passar meus dados pessoais a terceiros', 'nunca fazer compras nem pagamentos'])
    this.semear(pessoal, 'prosa', ['nunca publicar notas de versão sem me mostrar', 'nunca mexer em arquivos fora de docs/'])
  }

  atende(method: string) { return METODOS.includes(method) }

  call(method: string, b: Record<string, unknown>, spaceExiste: (id: string) => boolean, botNoSpace: (bot: string) => string | null): unknown {
    if (cena() === 'antigo') throw erro(404, 'unknown method')
    if (Object.keys(b).some((k) => !CAMPOS.has(k)) || typeof b.spaceId !== 'string') throw erro(400, 'invalid_field: pedido inválido')
    const space = b.spaceId
    const bot = typeof b.botId === 'string' ? b.botId : ''
    if (!spaceExiste(space)) throw erro(400, 'space_not_found')
    if (bot && botNoSpace(bot) !== space) throw erro(400, 'bot_not_found')
    if (method === 'nuncaFazer') return this.estado(space, bot)
    const lista = this.lista(space, bot)
    if (this.conflitoPendente) {
      // Outra janela gravou antes: a revisão lida ficou velha.
      this.conflitoPendente = false
      this.inserir(lista, 'nunca responder clientes em meu nome')
      lista.revision = ++this.rev
    }
    if (b.expectedRevision !== lista.revision) throw erro(409, 'nunca_fazer_revision_conflict')
    const texto = () => {
      const t = typeof b.texto === 'string' ? b.texto.trim() : ''
      if (!t || [...t].length > 200) throw erro(400, 'invalid_field: texto deve ter de 1 a 200 caracteres')
      return t
    }
    const achar = () => {
      const i = lista.itens.findIndex((x) => x.id === b.id)
      if (i < 0) throw erro(400, 'item_not_found')
      return i
    }
    switch (method) {
      case 'nuncaFazerAdicionar':
        if (lista.itens.length >= 20) throw erro(400, 'limit_exceeded: máximo de 20 itens')
        this.inserir(lista, texto())
        break
      case 'nuncaFazerEditar': { const i = achar(); lista.itens[i] = { ...lista.itens[i]!, texto: texto(), atualizadoEm: Date.now() }; break }
      case 'nuncaFazerRemover': lista.itens.splice(achar(), 1); break
      case 'nuncaFazerReordenar': {
        const ids = Array.isArray(b.ids) ? b.ids as string[] : []
        if (ids.length !== lista.itens.length || new Set(ids).size !== ids.length || ids.some((id) => !lista.itens.some((x) => x.id === id))) throw erro(400, 'invalid_field: ids devem conter a lista inteira')
        lista.itens = ids.map((id) => lista.itens.find((x) => x.id === id)!)
        break
      }
    }
    lista.revision = ++this.rev
    return this.estado(space, bot)
  }

  private lista(space: string, bot: string) {
    const chave = `${space}/${bot}`
    let l = this.listas.get(chave)
    if (!l) { l = { revision: 0, itens: [] }; this.listas.set(chave, l) }
    return l
  }
  private inserir(lista: { itens: Linha[] }, texto: string) {
    const agora = Date.now()
    lista.itens.push({ id: `nf-${++this.seq}`, texto, criadoEm: agora, atualizadoEm: agora })
  }
  private semear(space: string, bot: string, textos: string[]) {
    const lista = this.lista(space, bot)
    for (const t of textos) this.inserir(lista, t)
    lista.revision = ++this.rev
  }
  private itens(space: string, bot: string) {
    return this.lista(space, bot).itens.map((x, ordem) => ({ ...x, spaceId: space, botId: bot || null, ordem, origem: bot ? 'bot' : 'space' }))
  }
  private estado(space: string, bot: string) {
    return { space: this.itens(space, ''), bot: bot ? this.itens(space, bot) : [], revision: this.lista(space, bot).revision, spaceRevision: this.lista(space, '').revision }
  }
}
