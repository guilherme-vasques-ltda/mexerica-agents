import { passiveSkillsV2, SkillRequestScope, type SkillClient } from '../../../../shared/fork-skills.ts'
import type { CatalogSkill, InstalledSkillV2, SkillDetailsV2 } from '../../../store/fork-skills.ts'
import { Observable } from '../../../lib/observable.ts'
import { codigoDeSkill, ErroFrase, itemDoCatalogo, mensagemDeErro, motivoContraRemocao as motivoContra, type AlvoRemocao } from './regras.ts'
import { frase, frases, juntas, type Frase } from './textos.ts'
import type { SkillReviewComparison, SkillReviewRequest } from '../../../../shared/fork-skill-review.ts'
import { pedirRevisao } from './revisao.ts'

/** O pedido em andamento de um item: instalar (pelo `catalogId`), remover (pela encarnação vista) ou escrever à mão. */
export type Pedido = { tipo: 'instalar' | 'reinstalar' | 'remover' | 'escrever'; fase: 'enviando' | 'conferindo'; token: number }

/**
 * Um pedido enviado cuja resposta se perdeu ou chegou malformada (D166 R7): o Hub pode ter feito a
 * mudança ou não. Só uma releitura bem-sucedida do mesmo Hub depois disso (`conferido`) libera um
 * gesto novo na mesma chave; nada é repetido, desfeito ou dado como feito por inferência.
 */
export type Incerto = {
  tipo: Pedido['tipo']
  nome: string
  /** O método do wrapper (`installSkill`/`removeSkill`). */
  metodo: string
  /** Na remoção, a encarnação pedida: o aviso só vale para essa linha. */
  alvo: string | null
  conferido: boolean
  /** O aviso do envio (com a resposta do Hub, quando houve uma) e o aviso inteiro de agora (frases, lição da R1 do #572). */
  base: Frase
  mensagem: Frase
  /** O que a lista relida mostra agora (sem atribuir a causa ao pedido); `null`: nada a dizer (escrita à mão). */
  descrever: (() => Frase) | null
}

/** O que a região `status` diz enquanto o pedido anda e quando falha: uma frase inteira por tipo. */
const andando = (tipo: Pedido['tipo'], nome: string) => frase(`skills.loja.andando.${tipo}`, { nome })
const naoDeu = (tipo: Pedido['tipo'], nome: string, motivo: Frase) => frase(`skills.loja.falha.${tipo}`, { nome, motivo })

/**
 * A escolha humana pertence ao Hub, à época e à conexão em que foi feita. `cliente` e `geracao` são
 * o link vivo (`connectedClient`) e a geração dele (`principalEpoch`, que muda a cada `hello` do
 * stream) lidos no gesto: não é permissão, só "qual conexão a pessoa estava vendo".
 */
export type IntencaoHub = Readonly<{ hub: string; epoca: number; cliente: SkillClient | null; geracao: number }>
export type InspecaoSkills = Readonly<{ origem: IntencaoHub; valida: () => boolean }>
type ContextoHub = { hub: string | null; client: SkillClient | null; geracao?: number }

const CONEXAO_MUDOU = frase('skills.loja.conexaoMudou')
const SEM_CONEXAO = frase('skills.loja.semConexao')

/**
 * O catálogo D166 de um Hub, lido direto dele por RPC (fora do mirror e de qualquer cache).
 * Cada Hub tem a sua lista; trocar de Hub descarta respostas, erros e pedidos do anterior.
 */
export class CatalogoStore extends Observable {
  itens: CatalogSkill[] | null = null
  instaladas: InstalledSkillV2[] | null = null
  /** O catálogo veio com itens fora do contrato D166: eles não aparecem. */
  hubAntigo = false
  /**
   * O Hub não fala o protocolo forte de skills (resposta sem `skillProtocol: 2`, inclusive lista
   * vazia): sem lista e sem nenhuma ação daqui; nada volta para o formato antigo.
   */
  semProtocolo = false
  carregando = false
  erro: Frase | null = null
  /** Pedidos por chave: `catalogId` para instalar, `rm:<id>` para remover. */
  readonly pedidos = new Map<string, Pedido>()
  /** A última falha de cada chave, até tentar de novo. */
  readonly falhas = new Map<string, Frase>()
  /** Pedidos enviados sem resposta confiável, por chave, até um gesto novo ou outro Hub. */
  readonly incertos = new Map<string, Incerto>()
  /** O que a região `status` anuncia. */
  aviso: Frase | null = null
  /** O último item instalado por esta tela, para o aviso de "nenhum bot usa ainda". */
  instaladaAgora: string | null = null
  /** Muda a cada troca de Hub; a folha de detalhes recomeça quando muda. */
  epoca = 0

  private hub: string | null = null
  private origemDasListas: IntencaoHub | null = null
  private readonly leituras = new SkillRequestScope()
  private readonly mutacoes = new SkillRequestScope()
  private token = 0
  private ultimaLeitura: Promise<void> = Promise.resolve()

  private readonly cliente: () => Promise<SkillClient>
  /** Depois de instalar/remover: a lista de skills dos ajustes de bot relê do Hub. */
  private readonly depoisDeMudar: () => void
  private readonly contexto?: () => ContextoHub

  constructor(cliente: () => Promise<SkillClient>, depoisDeMudar: () => void = () => {}, contexto?: () => ContextoHub) {
    super()
    this.cliente = cliente
    this.depoisDeMudar = depoisDeMudar
    this.contexto = contexto
  }

  /** O Hub atual (`hello.hostId`). Outro Hub: tudo do anterior é descartado e a lista é relida. */
  sincronizar(hub: string | null, opcoes?: { adiarCarga: boolean }) {
    if (hub === this.hub) return
    this.hub = hub
    this.leituras.invalidate()
    this.mutacoes.invalidate()
    this.epoca += 1
    this.origemDasListas = null
    this.itens = null
    this.instaladas = null
    this.hubAntigo = false
    this.semProtocolo = false
    this.carregando = false
    this.erro = null
    this.pedidos.clear()
    this.falhas.clear()
    this.incertos.clear()
    this.aviso = null
    this.instaladaAgora = null
    this.changed()
    if (hub !== null && !opcoes?.adiarCarga) void this.carregar()
  }

  /** A tela abriu: Hub novo recomeça; o mesmo Hub relê, porque a lista pode ter mudado fora daqui. */
  abrir(hub: string | null) {
    if (hub !== this.hub) this.sincronizar(hub)
    else if (hub !== null) void this.carregar()
  }

  pedido(chave: string) {
    return this.pedidos.get(chave) ?? null
  }

  /** Captura agora, onde o gesto nasce (render da linha, abertura de folha ou confirmação). */
  capturarIntencao(): IntencaoHub | null {
    const vivo = this.contexto?.()
    const intencao = this.hub === null ? null : { hub: this.hub, epoca: this.epoca, cliente: vivo?.client ?? null, geracao: vivo?.geracao ?? 0 }
    return this.intencaoAtual(intencao) ? intencao : null
  }

  /**
   * A intenção ainda é do Hub e da época desta loja. Consulta também o Hub vivo: a troca pode
   * preceder o effect que sincroniza a tela. Decide se uma resposta já pedida ainda vale aqui.
   */
  intencaoAtual(intencao: IntencaoHub | null): intencao is IntencaoHub {
    return intencao != null && intencao.hub === this.hub && intencao.epoca === this.epoca
      && (!this.contexto || this.contexto().hub === intencao.hub)
  }

  /**
   * Além do Hub e da época, a conexão viva ainda é a do gesto: mesmo cliente e mesma geração.
   * A→B→A sem render (ou com a tela desmontada) mantém hostId e época, mas não a conexão.
   * Decide se um gesto pode virar pedido.
   */
  conexaoAtual(intencao: IntencaoHub | null): intencao is IntencaoHub {
    if (!this.intencaoAtual(intencao)) return false
    if (!this.contexto) return true
    const vivo = this.contexto()
    return vivo.client === intencao.cliente && (vivo.geracao ?? 0) === intencao.geracao
  }

  /**
   * Catálogo e instaladas juntos; só a leitura mais nova vale. Quem espera uma leitura que ficou
   * velha espera a mais nova: depois de `await carregar()`, a lista é a do Hub agora.
   */
  async carregar(inspecao?: InspecaoSkills) {
    if (inspecao && !this.leituraAtual(inspecao.origem, inspecao)) return
    let leitura = this.ler(inspecao)
    this.ultimaLeitura = leitura
    await leitura
    while (!inspecao && this.ultimaLeitura !== leitura) {
      leitura = this.ultimaLeitura
      await leitura
    }
  }

  /** Cache só é exibido na inspeção se nasceu da mesma origem, nunca de uma geração anterior. */
  listasDaOrigem(origem: IntencaoHub) {
    const lida = this.origemDasListas
    return this.conexaoAtual(origem) && lida !== null && lida.hub === origem.hub && lida.epoca === origem.epoca
      && lida.cliente === origem.cliente && lida.geracao === origem.geracao
  }

  /** A sessão de inspeção nunca adota a origem de uma reconexão. */
  private leituraAtual(origem: IntencaoHub | null, inspecao?: InspecaoSkills): origem is IntencaoHub {
    return this.conexaoAtual(origem) && (!inspecao || (inspecao.origem === origem && inspecao.valida()))
  }

  private async ler(inspecao?: InspecaoSkills) {
    const origem = inspecao?.origem ?? this.capturarIntencao()
    if (!this.leituraAtual(origem, inspecao)) return
    if (this.contexto && origem.cliente === null) {
      this.erro = SEM_CONEXAO
      this.changed()
      return
    }
    this.leituras.invalidate()
    this.carregando = true
    this.changed()
    try {
      const lido = await this.leituras.run(async () => {
        const client = await this.cliente()
        // ready() pode terminar no link de outro Hub: nada de ler por ele para esta tela.
        if (!this.leituraAtual(origem, inspecao)) return null
        if (this.contexto && client !== origem.cliente) throw new ErroFrase(CONEXAO_MUDOU)
        return Promise.all([passiveSkillsV2.catalog(client), passiveSkillsV2.installed(client)])
      })
      if (!lido.current) return
      if (!this.leituraAtual(origem, inspecao) || !lido.value) {
        this.carregando = false
        this.changed()
        return
      }
      const [catalogo, instaladas] = lido.value
      this.origemDasListas = origem
      this.itens = catalogo.filter(itemDoCatalogo)
      this.hubAntigo = catalogo.length > this.itens.length
      this.instaladas = instaladas
      this.semProtocolo = false
      this.erro = null
      this.carregando = false
      this.conferirIncertos()
    } catch (e) {
      if (!this.leituraAtual(origem, inspecao)) {
        this.carregando = false
        this.changed()
        return
      }
      // `run` só lança para a leitura atual: erro antigo não apaga a lista nem o estado novo.
      if (codigoDeSkill(e) === 'skill_installation_unsupported') {
        // Sem a prova de versão, nada desta resposta é usado: nem lista, nem ação.
        this.semProtocolo = true
        this.itens = null
        this.instaladas = null
        this.hubAntigo = false
        this.erro = null
      } else this.erro = mensagemDeErro(e)
      this.carregando = false
    }
    this.changed()
  }

  /**
   * Uma lista relida do mesmo Hub depois de um pedido incerto é a conferência: o aviso passa a dizer o
   * que a lista mostra agora, e a chave volta a aceitar um gesto novo.
   */
  private conferirIncertos() {
    for (const incerto of this.incertos.values()) {
      if (incerto.conferido) continue
      incerto.conferido = true
      const agora = incerto.descrever?.()
      incerto.mensagem = agora ? juntas(incerto.base, agora) : incerto.base
      this.aviso = incerto.mensagem
    }
  }

  /** O pedido desta chave ficou incerto e a lista ainda não foi relida depois: nenhum gesto novo. */
  aguardandoConferir(chave: string) {
    return this.incertos.get(chave)?.conferido === false
  }

  /** Origem obrigatória, do gesto: nunca a conexão de agora como valor padrão. */
  async instalar(item: CatalogSkill, origem: IntencaoHub | null) {
    if (!this.conexaoAtual(origem)) return
    const chave = item.id
    if (this.pedidos.has(chave) || item.installed || !item.installable) return
    // A intenção leva o digest que a pessoa viu; o Hub confere.
    const intencao = { catalogId: item.id, expectedDigest: item.digest }
    const instaladaNaLista = () => this.itens?.some((i) => i.id === item.id && i.installed) === true
    await this.mutar(origem, chave, 'instalar', item.name, (client) => passiveSkillsV2.install(client, intencao), (r) => {
      // Só a lista relida diz que está instalada; a resposta da mutação não basta.
      if (!instaladaNaLista()) return null
      this.instaladaAgora = item.name
      return r.alreadyInstalled ? frase('skills.loja.jaInstalada', { nome: item.name }) : frase('skills.loja.instalada', { nome: item.name })
    }, undefined, {
      alvo: null,
      descrever: () => instaladaNaLista() ? frases.incertoAgora.instalada(item.name) : frases.incertoAgora.naoInstalada(item.name),
    })
  }

  /**
   * D306: repõe a versão do recibo de uma curada alterada fora do app, só na encarnação vista no gesto.
   * O Hub confere o texto depois de gravar; só a lista relida com essa encarnação conferida confirma.
   */
  async reinstalar(skill: InstalledSkillV2, origem: IntencaoHub | null) {
    if (!this.conexaoAtual(origem) || !skill.receipt || skill.contentStatus === 'verified') return
    const chave = `re:${skill.id}`
    if (this.pedidos.has(chave)) return
    const intencao = { catalogId: skill.receipt.catalogId, expectedDigest: skill.receipt.digest, expectedInstallationId: skill.installationId }
    const agora = () => this.instaladas?.find((s) => s.id === skill.id)
    const outra = () => { const a = agora(); return a && a.installationId !== skill.installationId ? frases.instalacaoMudou : null }
    await this.mutar(origem, chave, 'reinstalar', skill.name, (client) => passiveSkillsV2.install(client, intencao), () => {
      const a = agora()
      return a?.installationId === skill.installationId && a.contentStatus === 'verified' ? frases.reinstalada(skill.name) : null
    }, outra, {
      alvo: skill.installationId,
      descrever: () => agora()?.contentStatus === 'verified' ? frases.incertoAgora.conferida(skill.name) : frases.incertoAgora.naoConferida(skill.name),
    })
  }

  /**
   * Por que a confirmação capturada já não vale para a lista lida agora: outra encarnação (ou outro
   * registro) na ID, ou conteúdo que deixou de conferir. ID ausente não contradiz: o Hub decide.
   */
  motivoContraRemocao(alvo: AlvoRemocao) {
    return motivoContra(alvo, this.instaladas ?? [])
  }

  /** A confirmação vale só para a instalação que capturou: outra na mesma ID a fecha e a recusa. */
  outraInstalacaoNaId(alvo: AlvoRemocao) {
    return this.motivoContraRemocao(alvo) !== null
  }

  /**
   * Remove só a encarnação vista, com o pedido estrito capturado no gesto (`capturarRemocao`): nunca
   * captura de novo depois de esperar. Origem obrigatória: um objeto retido nunca cria uma confirmação
   * no Hub de agora.
   */
  async remover(alvo: AlvoRemocao, origem: IntencaoHub | null) {
    if (!this.conexaoAtual(origem) || !alvo?.pedido) return
    const { skill, pedido } = alvo
    const chave = `rm:${pedido.id}`
    if (this.pedidos.has(chave)) return
    const motivo = this.motivoContraRemocao(alvo)
    if (motivo) {
      this.falhas.set(chave, motivo)
      this.aviso = naoDeu('remover', skill.name, motivo)
      this.changed()
      return
    }
    const naLista = () => this.instaladas?.some((s) => s.id === pedido.id && s.installationId === pedido.expectedInstallationId) === true
    await this.mutar(origem, chave, 'remover', skill.name, (client) => passiveSkillsV2.remove(client, pedido), () => {
      if (naLista()) return null
      if (this.instaladaAgora === skill.name) this.instaladaAgora = null
      return frase('skills.loja.removida', { nome: skill.name })
    }, () => this.motivoContraRemocao(alvo), {
      alvo: pedido.expectedInstallationId,
      // Ausência de agora é só ausência: o aviso não diz que este pedido a causou.
      descrever: () => naLista() ? frases.incertoAgora.presente(skill.name) : frases.incertoAgora.ausente(skill.name),
    })
  }

  /**
   * A skill escrita à mão vai para o Hub da origem, pelo RPC manual forte. `apareceu` só quando a lista
   * relida do mesmo Hub mostra a encarnação que o Hub devolveu (nunca por nome ou texto: duas iguais são
   * possíveis); `incerto` quando o pedido saiu e a resposta não chegou inteira; `falhou` no resto. Fora
   * de `apareceu`, o rascunho fica com quem chamou. O texto é da pessoa: vai como foi escrito.
   */
  async escrever(rascunho: { name: string; description: string; instructions: string }, origem: IntencaoHub | null): Promise<'apareceu' | 'falhou' | 'incerto'> {
    if (!this.conexaoAtual(origem)) return 'falhou'
    const chave = 'escrever'
    if (this.pedidos.has(chave)) return 'falhou'
    let apareceu = false
    const incerto = await this.mutar(origem, chave, 'escrever', rascunho.name, (client) => passiveSkillsV2.add(client, rascunho.name, rascunho.description, rascunho.instructions), (r) => {
      const encarnacao = r.skill.installationId
      if (!this.instaladas?.some((s) => s.installationId === encarnacao)) return null
      apareceu = true
      return frase('skills.loja.adicionada', { nome: rascunho.name })
    }, undefined, { alvo: null, descrever: null })
    if (!this.intencaoAtual(origem)) return 'falhou'
    return apareceu ? 'apareceu' : incerto ? 'incerto' : 'falhou'
  }

  fecharAviso() {
    this.instaladaAgora = null
    this.changed()
  }

  /**
   * Detalhes no Hub e na época da origem (a folha que pediu), pelo link vivo desse Hub. Resposta que
   * chega depois de trocar de Hub volta `null`: nada dela aparece.
   */
  async detalhes(selecao: { id: string } | { source: string }, origem: IntencaoHub | null, inspecao?: InspecaoSkills): Promise<SkillDetailsV2 | null> {
    if (!this.leituraAtual(origem, inspecao)) return null
    if (this.contexto && origem.cliente === null) throw new ErroFrase(SEM_CONEXAO)
    try {
      const client = await this.cliente()
      if (!this.leituraAtual(origem, inspecao)) return null
      if (this.contexto && client !== origem.cliente) throw new ErroFrase(CONEXAO_MUDOU)
      const lido = await passiveSkillsV2.details(client, selecao)
      return this.leituraAtual(origem, inspecao) ? lido : null
    } catch (e) {
      if (!this.leituraAtual(origem, inspecao)) return null
      throw e
    }
  }

  /**
   * d2a: compara (ou abre a base de) uma skill escrita à mão, só por `skillDetails` com `review`.
   * Leitura sem efeito, presa à conexão exata do gesto (Hub, época, cliente e geração): resposta que
   * chega depois de qualquer troca ou reconexão volta `null`. Nunca captura a conexão de agora.
   */
  async revisar(id: string, pedido: SkillReviewRequest, origem: IntencaoHub | null): Promise<SkillReviewComparison | null> {
    if (!this.conexaoAtual(origem)) return null
    if (this.contexto && origem.cliente === null) throw new ErroFrase(SEM_CONEXAO)
    try {
      const client = await this.cliente()
      if (!this.conexaoAtual(origem)) return null
      if (this.contexto && client !== origem.cliente) throw new ErroFrase(CONEXAO_MUDOU)
      const lido = await pedirRevisao(client, id, pedido)
      return this.conexaoAtual(origem) ? lido : null
    } catch (e) {
      if (!this.conexaoAtual(origem)) return null
      throw e
    }
  }

  /**
   * Um pedido por chave; os outros itens continuam livres. O item só deixa de estar ocupado
   * depois de a lista ser relida do Hub: a tela nunca mostra "Instalada" pela resposta da mutação.
   * `confirmar` roda com a lista relida e devolve o anúncio, ou `null` se o Hub não mostra o efeito.
   * `alvoMudou`, conferido junto da conexão logo antes do RPC, devolve o motivo de não enviar mais.
   * Resposta perdida ou malformada depois do envio vira `incerto` (uma chamada, sem repetir), e a
   * lista do mesmo Hub é relida; devolve `true` nesse caso.
   */
  private async mutar<T>(origem: IntencaoHub, chave: string, tipo: Pedido['tipo'], nome: string, enviar: (client: SkillClient) => Promise<T>, confirmar: (r: T) => Frase | null, alvoMudou: () => Frase | null = () => null, seIncerto: Pick<Incerto, 'alvo' | 'descrever'>): Promise<boolean> {
    // Gesto de uma tela que já não existe, Hub sem o protocolo forte ou pedido incerto ainda não
    // conferido nesta chave: nada de pedido, RPC ou aviso.
    if (!this.conexaoAtual(origem) || this.semProtocolo || this.aguardandoConferir(chave)) return false
    if (this.contexto && origem.cliente === null) {
      // Sem link no gesto: esperar a reconexão poderia chegar a outro Hub.
      this.falhas.set(chave, SEM_CONEXAO)
      this.aviso = naoDeu(tipo, nome, SEM_CONEXAO)
      this.changed()
      return false
    }
    const token = ++this.token
    this.pedidos.set(chave, { tipo, fase: 'enviando', token })
    this.falhas.delete(chave)
    // Gesto novo: o aviso incerto anterior desta chave sai; a intenção antiga não é reaproveitada.
    this.incertos.delete(chave)
    this.aviso = andando(tipo, nome)
    this.changed()
    const meu = () => this.intencaoAtual(origem) && this.pedidos.get(chave)?.token === token
    let resposta: { valor: T } | null = null
    let incerto = false
    try {
      try {
        const r = await this.mutacoes.run(async () => {
          const client = await this.cliente()
          // ready() pode terminar no cliente de outro Hub, ou de A de novo por outra conexão.
          // Nenhum await entre a guarda e o RPC.
          if (!meu()) return null
          if (this.contexto && (this.contexto().client !== client || client !== origem.cliente || !this.conexaoAtual(origem))) throw new ErroFrase(CONEXAO_MUDOU)
          // A lista pode ter sido relida durante ready() com outra instalação na mesma ID.
          const mudou = alvoMudou()
          if (mudou) throw new ErroFrase(mudou)
          return { valor: await enviar(client) }
        })
        if (!r.current || !meu()) return false
        if (!r.value) return false
        this.pedidos.set(chave, { tipo, fase: 'conferindo', token })
        this.aviso = frase('skills.loja.conferindo', { nome })
        this.changed()
        resposta = r.value
      } catch (e) {
        if (!meu()) return false
        if (codigoDeSkill(e) === 'skill_mutation_outcome_unknown') {
          // Enviado, sem resposta confiável: nem falha nem sucesso. Releitura do mesmo Hub logo abaixo.
          incerto = true
          // A resposta do Hub, quando houve uma, aparece como dado; não prova que nada mudou.
          const causa = (e as { cause?: unknown }).cause
          const doHub = causa instanceof Error && causa.message && codigoDeSkill(causa) === null ? mensagemDeErro(causa) : null
          const base = doHub ? juntas(frases.incerto[tipo](nome), frase('skills.loja.respostaDoHub', { resposta: doHub })) : frases.incerto[tipo](nome)
          this.incertos.set(chave, { tipo, nome, metodo: String((e as { method?: unknown }).method ?? ''), conferido: false, base, mensagem: base, ...seIncerto })
          this.aviso = base
        } else {
          const motivo = mensagemDeErro(e)
          this.falhas.set(chave, motivo)
          this.aviso = naoDeu(tipo, nome, motivo)
        }
      }
      // Sucesso, falha ou incerto, o estado real é o do Hub (o mesmo da origem).
      if (meu()) await this.carregar()
      if (!meu()) return incerto
      if (resposta) {
        const anuncio = this.erro ? null : confirmar(resposta.valor)
        if (anuncio) this.aviso = anuncio
        else {
          const motivo = this.erro ? frase('skills.loja.enviadoSemConferir', { erro: this.erro }) : frase('skills.loja.aceitoSemMudanca')
          this.falhas.set(chave, motivo)
          this.aviso = motivo
        }
      }
    } finally {
      // Por qualquer saída, o pedido desta chamada solta o botão; o de outra chamada fica.
      if (this.pedidos.get(chave)?.token === token) {
        this.pedidos.delete(chave)
        this.changed()
      }
    }
    // Ajustes de bot e o contador releem (só leitura) também quando o resultado é incerto.
    if (resposta || incerto) this.depoisDeMudar()
    return incerto
  }
}

const lojas = new WeakMap<object, CatalogoStore>()

/**
 * Uma loja por Hub (o `BotStore` do computador), compartilhada entre a biblioteca e os instalados.
 * O vínculo da conexão é o mesmo par que a disponibilidade de chamada já usa: `connectedClient` e
 * `principalEpoch`, só lidos.
 */
export function catalogoFor(hub: { hello: { hostId: string } | null; connectedClient: SkillClient | null; principalEpoch: number }, cliente: () => Promise<SkillClient>, depoisDeMudar?: () => void) {
  let loja = lojas.get(hub)
  if (!loja) {
    loja = new CatalogoStore(cliente, depoisDeMudar, () => ({ hub: hub.hello?.hostId ?? null, client: hub.connectedClient, geracao: hub.principalEpoch }))
    lojas.set(hub, loja)
  }
  return loja
}
