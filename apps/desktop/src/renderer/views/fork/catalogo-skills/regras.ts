import { passiveSkillsV2 } from '../../../../shared/fork-skills.ts'
import type { CatalogSkill, InstalledSkillV2, InternalSkill, RemoveInstalledSkillV2, SkillAvailability, SkillDetailsV2 } from '../../../store/fork-skills.ts'
import { cru, ErroFrase, frase, frases, type Frase } from './textos.ts'

export { ErroFrase }
import { formatarNumero } from '../../../lib/fork-i18n/index.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'

// Regras puras do catálogo D166: o que cada item mostra, a partir só do que o Hub respondeu.

/** O que a ação de um item pode fazer agora (sem o pedido em andamento, que é da loja). */
export type EstadoItem =
  /** O Hub confirmou `installed` para o digest atual. */
  | { tipo: 'instalada'; instaladaId: string | null }
  /** Há um registro deste item com recibo de outro digest: não é a versão atual. */
  | { tipo: 'outraVersao'; instalada: InstalledSkillV2 }
  | { tipo: 'instalavel' }
  /** Licença ou dependência indisponível, ou o Hub não deixa instalar. */
  | { tipo: 'bloqueada' }

const DISPONIVEIS: readonly SkillAvailability[] = ['text', 'adapted']

/**
 * Nunca por nome: só `installed` do Hub conta como instalada, e só um recibo com o mesmo
 * `catalogId` e outro digest conta como outra versão. Legados sem recibo não casam com o catálogo.
 */
export function estadoDoItem(item: CatalogSkill, instaladas: readonly InstalledSkillV2[]): EstadoItem {
  if (item.installed) return { tipo: 'instalada', instaladaId: item.installedId }
  const anterior = instaladas.find((s) => s.receipt?.catalogId === item.id && s.receipt.digest !== item.digest)
  if (anterior) return { tipo: 'outraVersao', instalada: anterior }
  if (item.installable && DISPONIVEIS.includes(item.availability)) return { tipo: 'instalavel' }
  return { tipo: 'bloqueada' }
}

/** O registro instalado deste item, para remover com o digest do recibo. */
export function instaladaDoItem(item: CatalogSkill, instaladas: readonly InstalledSkillV2[]): InstalledSkillV2 | null {
  if (item.installedId) {
    const exata = instaladas.find((s) => s.id === item.installedId)
    if (exata) return exata
  }
  return instaladas.find((s) => s.receipt?.catalogId === item.id) ?? null
}

/**
 * Os detalhes de uma instalada falam do registro e do recibo dela. Os metadados que o Hub devolve
 * em `skillDetails.catalog` só valem se forem os da versão do recibo (mesmo `catalogId` e digest);
 * recibo desconhecido fica só com o recibo. A versão de agora do catálogo, com o mesmo `catalogId`
 * e outro digest, só aparece à parte. Nunca por nome.
 */
export function fichaDaInstalada(instalada: InstalledSkillV2, capturado: CatalogSkill | null, atual: CatalogSkill | null) {
  const recibo = instalada.receipt
  const daVersao = (c: CatalogSkill | null): c is CatalogSkill => c !== null && recibo !== null && c.id === recibo.catalogId && c.digest === recibo.digest
  return {
    nome: instalada.name,
    descricao: instalada.description,
    recibo,
    capturado: daVersao(capturado) ? capturado : null,
    outraVersao: recibo !== null && atual !== null && atual.id === recibo.catalogId && atual.digest !== recibo.digest ? atual : null,
  }
}

/**
 * A versão que uma folha de detalhes descreve, só com o que o Hub manda (D166 R5/R8). De uma
 * instalada, a encarnação emitida pelo Hub (`installationId`) com o registro inteiro e o estado do
 * conteúdo: reinstalação idêntica tem outra encarnação, e qualquer diferença também é outra versão.
 * Do catálogo, o item pelo id e pelo digest; mudanças só de estado (`installed`…) não são outra versão.
 */
export function versaoDaFolha(item: CatalogSkill | null, instalada: InstalledSkillV2 | null): string | null {
  if (instalada) {
    return JSON.stringify(['instalada', instalada.id, instalada.installationId, instalada.contentStatus,
      instalada.name, instalada.description, instalada.source, instalada.path, instalada.receipt])
  }
  if (item) return JSON.stringify(['catalogo', item.id, item.digest])
  return null
}

/** A mesma instalação: a mesma ID, a mesma encarnação e o mesmo registro. Uma confirmação só vale para ela. */
export function mesmaInstalacao(a: InstalledSkillV2, b: InstalledSkillV2) {
  return versaoDaFolha(null, a) === versaoDaFolha(null, b)
}

/**
 * A resposta de `skillDetails` é da versão que a folha mostra? Instalada: `installation` é a mesma
 * encarnação e o mesmo registro da linha (o wrapper já confere que o recibo do envelope é o dela).
 * Catálogo: o item devolvido tem o mesmo id e digest, sem instalação. Resposta de outra versão não
 * empresta texto nem metadados a esta.
 */
export function detalhesDaVersao(d: SkillDetailsV2, item: CatalogSkill | null, instalada: InstalledSkillV2 | null): boolean {
  if (instalada) return d.installation !== null && mesmaInstalacao(d.installation, instalada)
  if (item) return d.installation === null && d.catalog != null && d.catalog.id === item.id && d.catalog.digest === item.digest
  return false
}

/** O que uma confirmação de remoção guarda: a linha vista e o pedido estrito capturado dela no gesto. */
export type AlvoRemocao = Readonly<{ skill: InstalledSkillV2; pedido: RemoveInstalledSkillV2 }>

/**
 * Captura, no próprio gesto, a remoção estrita da linha vista: id, encarnação e recibo esperado
 * (`none` ou o catálogo exato). Conteúdo alterado ou indisponível também se remove (D306); `null` só
 * quando a linha está fora do contrato.
 */
export function capturarRemocao(skill: InstalledSkillV2): AlvoRemocao | null {
  try {
    return { skill, pedido: passiveSkillsV2.captureRemoval(skill) }
  } catch {
    return null
  }
}

/**
 * Por que uma confirmação capturada já não vale para a lista lida agora: outra encarnação (ou outro
 * registro) na ID, ou conteúdo que deixou de conferir. ID ausente não contradiz: o Hub decide.
 */
export function motivoContraRemocao(alvo: AlvoRemocao, instaladas: readonly InstalledSkillV2[]): Frase | null {
  const agora = instaladas.find((s) => s.id === alvo.pedido.id)
  if (!agora) return null
  if (agora.installationId !== alvo.pedido.expectedInstallationId) return frases.instalacaoMudou
  return mesmaInstalacao(agora, alvo.skill) ? null : frases.instalacaoMudou
}

export const rotuloDisponibilidade: Record<SkillAvailability, Frase> = {
  text: frase('skills.disponibilidade.text'),
  adapted: frase('skills.disponibilidade.adapted'),
  licenseUnavailable: frase('skills.disponibilidade.licenseUnavailable'),
  dependencyUnavailable: frase('skills.disponibilidade.dependencyUnavailable'),
}

export const TODAS = '__todas__'
/** As skills sem categoria: valor interno do grupo (nunca mostrado cru); a tela mostra `skills.categoriaOutras`. */
export const SEM_CATEGORIA = '__outras__'

/** As categorias do Hub, em ordem alfabética pt-BR, com quantas skills cada uma tem. */
export function categorias(itens: readonly CatalogSkill[]): { id: string; nome: string; total: number }[] {
  const contagem = new Map<string, number>()
  for (const item of itens) {
    const nome = item.category.trim() || SEM_CATEGORIA
    contagem.set(nome, (contagem.get(nome) ?? 0) + 1)
  }
  // As sem categoria vão por último (antes ficavam em "Outras", na ordem alfabética).
  return [...contagem].sort(([a], [b]) => Number(a === SEM_CATEGORIA) - Number(b === SEM_CATEGORIA) || a.localeCompare(b, 'pt-BR')).map(([nome, total]) => ({ id: nome, nome, total }))
}

const normalizar = (texto: string) => texto.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase()

/** Busca por ID, nome, descrição, categoria e apelidos, sem acento nem caixa. */
export function filtrar(itens: readonly CatalogSkill[], busca: string, categoria: string): CatalogSkill[] {
  const termo = normalizar(busca.trim())
  return itens.filter((item) => {
    if (categoria !== TODAS && (item.category.trim() || SEM_CATEGORIA) !== categoria) return false
    if (!termo) return true
    return [item.id, item.name, item.description, item.category, ...item.aliases].some((campo) => normalizar(campo).includes(termo))
  })
}

/** As instaladas que batem com a busca (nome, descrição e id), sem acento nem caixa; na ordem do Hub. */
export function filtrarInstaladas(lista: readonly InstalledSkillV2[], busca: string): InstalledSkillV2[] {
  const termo = normalizar(busca.trim())
  if (!termo) return [...lista]
  return lista.filter((s) => [s.name, s.description, s.id].some((campo) => normalizar(campo).includes(termo)))
}

/** As internas que a busca encontra, pelos mesmos campos das instaladas. */
export function filtrarInternas(lista: readonly InternalSkill[], busca: string): InternalSkill[] {
  const termo = normalizar(busca.trim())
  if (!termo) return [...lista]
  return lista.filter((s) => [s.name, s.description, s.id].some((campo) => normalizar(campo).includes(termo)))
}

export type GrupoInstaladas = { id: 'catalogo' | 'semRecibo'; skills: InstalledSkillV2[] }

/**
 * Os grupos que os dados sustentam: com recibo do catálogo e sem recibo (antiga ou escrita à mão).
 * As que vêm com o app chegam à parte, em internas, e não entram aqui. Grupo vazio sai.
 */
export function agruparInstaladas(lista: readonly InstalledSkillV2[]): GrupoInstaladas[] {
  const grupos: GrupoInstaladas[] = [
    { id: 'catalogo', skills: lista.filter((s) => s.receipt !== null) },
    { id: 'semRecibo', skills: lista.filter((s) => s.receipt === null) },
  ]
  return grupos.filter((g) => g.skills.length > 0)
}

/** Seções por categoria, na mesma ordem dos chips. */
export function porCategoria(itens: readonly CatalogSkill[]): { categoria: string; itens: CatalogSkill[] }[] {
  return categorias(itens).map((c) => ({ categoria: c.nome, itens: itens.filter((i) => (i.category.trim() || SEM_CATEGORIA) === c.nome) }))
}

/** ←/→ (com volta), Home e End numa lista de abas horizontal; `null`: tecla que não é das abas. */
export function alvoDaAba(e: { key: string; altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, indice: number, total: number): number | null {
  if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || total === 0) return null
  if (e.key === 'ArrowRight') return (indice + 1) % total
  if (e.key === 'ArrowLeft') return (indice - 1 + total) % total
  if (e.key === 'Home') return 0
  if (e.key === 'End') return total - 1
  return null
}

/** Revisão curta para a tela; a inteira vai no `title`. */
export const curto = (valor: string, tamanho = 12) => (valor.length > tamanho ? valor.slice(0, tamanho) : valor)

/** Tamanho de arquivo com o separador decimal do idioma da tela. */
export function tamanho(bytes: number, idioma: AppLanguage) {
  if (bytes < 1024) return `${formatarNumero(idioma, bytes)} B`
  if (bytes < 1024 * 1024) return `${formatarNumero(idioma, bytes / 1024, { maximumFractionDigits: 1 })} KB`
  return `${formatarNumero(idioma, bytes / 1024 / 1024, { maximumFractionDigits: 1 })} MB`
}

const texto = (v: unknown): v is string => typeof v === 'string'
const listaDeTextos = (v: unknown): v is string[] => Array.isArray(v) && v.every(texto)
const objeto = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null

/**
 * Um Hub antigo responde `marketSkills` no formato anterior (só `source`/nome/descrição). A tela
 * só mostra o que tem os campos do contrato D166; o resto não vira item instalável nem quebra a lista.
 */
export function itemDoCatalogo(v: unknown): v is CatalogSkill {
  if (!objeto(v)) return false
  const origem = v.origin
  return texto(v.id) && texto(v.name) && texto(v.description) && texto(v.category) && texto(v.digest)
    && texto(v.license) && texto(v.reason) && listaDeTextos(v.aliases) && listaDeTextos(v.requirements) && listaDeTextos(v.losses)
    && typeof v.installable === 'boolean' && typeof v.installed === 'boolean' && Array.isArray(v.files)
    && (v.availability === 'text' || v.availability === 'adapted' || v.availability === 'licenseUnavailable' || v.availability === 'dependencyUnavailable')
    && objeto(origem) && texto(origem.repo) && texto(origem.revision) && texto(origem.path)
}

/** Recusas comprovadas antes do efeito (D166 R6/R7), com o texto da tela; o código nunca aparece cru. */
const RECUSAS: Record<string, Frase> = {
  skill_installation_unsupported: frases.recusaProtocolo,
  skill_installation_stale: frases.instalacaoMudou,
  skill_installation_changed: frases.conteudoMotivo.changed,
  skill_installation_unavailable: frases.conteudoMotivo.unavailable,
}

/** O código de skill de um erro (`SkillInstallationError`, recusa do Hub com o código ou resultado incerto). */
export function codigoDeSkill(e: unknown): string | null {
  const codigo = objeto(e) && texto(e.code) ? e.code : e instanceof Error ? e.message : null
  return codigo !== null && (Object.hasOwn(RECUSAS, codigo) || codigo === 'skill_mutation_outcome_unknown') ? codigo : null
}

/**
 * A mensagem para a pessoa, como `Frase`: recusa conhecida e erro da própria interface pelo catálogo; o texto do Hub,
 * sem prefixo técnico (`codigo: texto`), como dado.
 */
export function mensagemDeErro(e: unknown): Frase {
  if (e instanceof ErroFrase) return e.frase
  const codigo = codigoDeSkill(e)
  if (codigo !== null && Object.hasOwn(RECUSAS, codigo)) return RECUSAS[codigo]!
  const bruto = e instanceof Error ? e.message : objeto(e) && texto(e.message) ? e.message : String(e)
  const sem = bruto.replace(/^[a-z][a-z0-9_]*:\s*/, '').trim()
  return sem ? cru(sem) : frase('skills.hubNaoRespondeu')
}
