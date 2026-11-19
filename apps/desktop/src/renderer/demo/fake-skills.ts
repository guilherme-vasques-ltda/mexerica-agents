import type { CatalogSkill, InstalledSkillV2, SkillContentStatus, SkillReceipt } from '@shared/fork-skills'

import bundle from '../../../../../host/src/fork/skill_catalog/registry.json'

// Estado/recibos do demo são sintéticos; nomes, descrições e instruções vêm do bundle
// fixado, sem tradução. As licenças/avisos permanecem nos assets do mesmo bundle.
// Nenhuma fonte, comando, link ou script é executado por esta fixture.
// D166 R8: fala o protocolo forte como o Hub R7 (envelopes `skillProtocol: 2`, encarnação
// `namespace:sequência` a cada instalação, remoção estrita). Não representa um Hub real.

type Metodo = 'marketSkills' | 'skills' | 'skillDetails' | 'installSkill' | 'removeSkill'
const METODOS: readonly string[] = ['marketSkills', 'skills', 'skillDetails', 'installSkill', 'removeSkill']

/** Hex fictício e estável para revisões e digests do demo (não é hash de nada). */
const hex = (semente: string, tamanho: number) => {
  let h = 2166136261
  let out = ''
  while (out.length < tamanho) {
    for (const c of semente + out.length) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0
    out += h.toString(16).padStart(8, '0')
  }
  return out.slice(0, tamanho)
}

type Base = Pick<CatalogSkill, 'id' | 'name' | 'description' | 'category' | 'availability'> & Partial<CatalogSkill> & { repo: string }

function entrada(b: Base, versao = 'v1'): CatalogSkill {
  const { repo, ...resto } = b
  const installable = b.installable ?? (b.availability === 'text' || b.availability === 'adapted')
  return {
    source: b.id, aliases: [], license: 'MIT', reason: '', requirements: [], losses: [], adaptation: null,
    files: [
      { path: 'SKILL.md', bytes: 4096 + b.id.length * 97, sha256: hex(`${b.id}-skill-${versao}`, 64), mode: '100644' },
      ...(b.availability === 'adapted' ? [{ path: 'NOTICE-DONNABOT.md', bytes: 612, sha256: hex(`${b.id}-notice`, 64), mode: '100644' as const }] : []),
    ],
    installed: false, installedId: null, readiness: null,
    ...resto,
    installable,
    origin: { repo, revision: hex(`${repo}-rev`, 40), path: `skills/${b.id}`, sourceDigest: hex(`${b.id}-fonte`, 64) },
    digest: hex(`${b.id}-envelope-${versao}`, 64),
  }
}

const CATALOGO: Base[] = bundle.map(({ metadata }) => ({
  ...metadata,
  repo: metadata.origin.repo,
  availability: metadata.availability as CatalogSkill['availability'],
  readiness: metadata.readiness as CatalogSkill['readiness'],
  files: metadata.files.map((f) => ({ ...f, mode: '100644' as const })),
}))

const TEXTO = (id: string | undefined) => {
  const md = bundle.find((p) => p.metadata.id === id)?.assets.find((a) => a.path === 'SKILL.md')?.content
  // Equivalente à separação da moldura para exibição: nenhum byte do pacote é regravado.
  return md?.replace(/^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/, '').trim() ?? ''
}

// O texto manual desta fixture já era originalmente em português.
const TEXTO_MANUAL = (nome: string) => [
  `# ${nome}`, '', 'Texto sintético do demo: mostra como a skill aparece, sem conteúdo real.', '',
  '- Leia o pedido antes de agir.', '- Confira [a documentação](https://example.invalid/skill) quando precisar.', '',
  '<script>alert("isto é texto")</script>',
].join('\n')

const recibo = (item: CatalogSkill): SkillReceipt => ({
  schema: 1, catalogId: item.id, digest: item.digest, origin: item.origin, adaptation: item.adaptation, license: item.license,
  files: item.files, result: 'imported', coverage: 'pinnedPassiveTextOnly',
  limitations: ['Confere origem e integridade dos arquivos.', 'Não é scanner nem aprovação de segurança.'],
})

/** Namespace sintético de encarnações de cada Hub do demo (não é UUID de nada). */
const NAMESPACE = { a: 'deadbeef-0000-4000-8000-00000000000a', b: 'deadbeef-0000-4000-8000-00000000000b' }

type Hub = { itens: CatalogSkill[]; instaladas: InstalledSkillV2[]; sequencia: number; namespace: string }

/** Cada instalação recebe outra encarnação, mesmo com texto, nome e digest idênticos. */
function encarnar(hub: Hub, skill: Omit<InstalledSkillV2, 'installationId' | 'contentStatus'>, contentStatus: SkillContentStatus = 'verified'): InstalledSkillV2 {
  hub.sequencia += 1
  return { ...skill, installationId: `${hub.namespace}:${hub.sequencia}`, contentStatus }
}

const instalada = (item: CatalogSkill) => ({
  id: item.name, name: item.name, description: item.description, source: item.source, path: `skills/${item.name}`, receipt: recibo(item),
})

/** Um Hub do demo: catálogo e instaladas próprios. */
function hubInicial(qual: 'a' | 'b'): Hub {
  const itens = (qual === 'a' ? CATALOGO : CATALOGO.filter((b) => ['Pesquisa', 'Vídeo'].includes(b.category))).map((b) => entrada(b))
  const porId = new Map(itens.map((i) => [i.id, i]))
  const hub: Hub = { itens, instaladas: [], sequencia: 0, namespace: NAMESPACE[qual] }
  const find = porId.get('find-skills')
  if (find && qual === 'a') {
    hub.instaladas.push(encarnar(hub, instalada(find)))
    find.installed = true
    find.installedId = find.name
  }
  if (qual === 'a') {
    // Versão anterior de uma skill do catálogo (outro digest) e dois legados sem recibo.
    const antiga = entrada(CATALOGO.find((b) => b.id === 'content-strategy')!, 'v0')
    hub.instaladas.push(encarnar(hub, { ...instalada(antiga), id: 'content-strategy-v0' }))
    hub.instaladas.push(encarnar(hub, { id: 'pdf', name: 'pdf', description: 'Skill antiga instalada antes do catálogo.', source: 'pdf', path: 'skills/pdf', receipt: null }))
    hub.instaladas.push(encarnar(hub, { id: 'demo-skill', name: 'Revisar texto (demo)', description: 'Skill sintética escrita à mão, para conferir a seleção por bot.', source: '', path: 'skills/demo-skill', receipt: null }))
  }
  return hub
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

export class FakeSkills {
  /** Atraso de cada resposta (ms), por método. */
  readonly atraso: Partial<Record<Metodo, number>> = {}
  /** A próxima chamada deste método falha com esta mensagem. */
  readonly falhar: Partial<Record<Metodo, string>> = {}
  /** Responde como um Hub sem o protocolo forte (antes do R7): sem `skillProtocol` nas respostas. */
  formatoAntigo = false
  /** A próxima chamada deste método faz o efeito e a resposta se perde (resultado incerto no app). */
  readonly perderResposta: Partial<Record<Metodo, true>> = {}
  readonly chamadas: { metodo: string; corpo: unknown; hub: string; em: number }[] = []
  private hubs: Record<'a' | 'b', Hub> = { a: hubInicial('a'), b: hubInicial('b') }
  hub: 'a' | 'b' = 'a'

  atende(metodo: string) {
    return METODOS.includes(metodo)
  }

  /** Muda o estado do conteúdo de uma instalada no Hub atual (texto alterado ou arquivo ilegível). */
  marcarConteudo(id: string, contentStatus: SkillContentStatus) {
    const estado = this.hubs[this.hub]
    estado.instaladas = estado.instaladas.map((s) => (s.id === id ? { ...s, contentStatus } : s))
  }

  async call(metodo: string, corpo: Record<string, unknown>, fail: (m: string) => unknown): Promise<unknown> {
    const hub = this.hub
    this.chamadas.push({ metodo, corpo: structuredClone(corpo), hub, em: Date.now() })
    const atraso = this.atraso[metodo as Metodo]
    if (atraso) await sleep(atraso)
    const falha = this.falhar[metodo as Metodo]
    if (falha) {
      delete this.falhar[metodo as Metodo]
      throw fail(falha)
    }
    const estado = this.hubs[hub]
    const forte = corpo?.skillProtocol === 2
    // Hub antigo: ignora o corpo das leituras, responde sem prova de versão e recusa o campo novo antes do efeito.
    if (this.formatoAntigo) {
      if (metodo === 'marketSkills') return { items: estado.itens.map((i) => ({ source: i.id, name: i.name, description: i.description, installed: false })) }
      if (metodo === 'skills') return { items: estado.instaladas.map(({ installationId: _, contentStatus: __, ...s }) => s) }
      if (forte) throw fail(metodo === 'removeSkill' ? 'Intenção de remoção inválida' : metodo === 'installSkill' ? 'Texto manual inválido' : 'Escolha uma habilidade por id ou source')
    }
    const resposta = this.responder(estado, metodo, corpo, fail)
    if (this.perderResposta[metodo as Metodo]) {
      // O efeito já aconteceu; só a resposta se perde.
      delete this.perderResposta[metodo as Metodo]
      throw fail('A conexão caiu antes da resposta.')
    }
    return resposta
  }

  private responder(estado: Hub, metodo: string, corpo: Record<string, unknown>, fail: (m: string) => unknown): unknown {
    const v2 = <T extends object>(r: T) => ({ skillProtocol: 2 as const, ...r })
    switch (metodo) {
      case 'marketSkills':
        return v2({ items: structuredClone(estado.itens) })
      case 'skills':
        return v2({ items: structuredClone(estado.instaladas) })
      case 'skillDetails': {
        const inst = typeof corpo.id === 'string' ? estado.instaladas.find((s) => s.id === corpo.id) : null
        if (typeof corpo.id === 'string' && !inst) throw fail('skill_not_found: skill não encontrada neste Hub.')
        if (inst && inst.contentStatus !== 'verified') throw fail(inst.contentStatus === 'changed' ? 'skill_installation_changed' : 'skill_installation_unavailable')
        const item = inst ? (inst.receipt ? estado.itens.find((i) => i.id === inst.receipt!.catalogId) ?? null : null)
          : typeof corpo.source === 'string' ? estado.itens.find((i) => i.source === corpo.source) ?? null : null
        if (!inst && !item) throw fail('skill_not_found: skill não encontrada neste Hub.')
        return v2({ markdown: item ? TEXTO(item.id) : TEXTO_MANUAL(inst!.name), catalog: structuredClone(item), receipt: inst?.receipt ?? null, installation: inst ? structuredClone(inst) : null })
      }
      case 'installSkill': {
        if (typeof corpo.source === 'string') throw fail('Instalação por source foi encerrada; escolha catalogId e expectedDigest do catálogo curado')
        if (typeof corpo.name === 'string') {
          const base = corpo.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-')
          // Como o Hub: o nome de pasta nunca sobrescreve outra skill.
          let id = base
          for (let n = 2; estado.instaladas.some((s) => s.id === id); n++) id = `${base}-${n}`
          const skill = encarnar(estado, { id, name: corpo.name.trim(), description: String(corpo.description ?? ''), source: 'custom', path: `skills/${id}`, receipt: null })
          estado.instaladas.push(skill)
          return v2({ skill, alreadyInstalled: false })
        }
        const item = estado.itens.find((i) => i.id === corpo.catalogId)
        if (!item) throw fail('skill_not_found: esta skill não está no catálogo deste Hub.')
        if (item.digest !== corpo.expectedDigest) throw fail('A versão do catálogo mudou; confira os detalhes antes de instalar')
        if (!item.installable) throw fail(`skill_unavailable: ${item.reason}`)
        if (item.installed) return v2({ skill: estado.instaladas.find((s) => s.id === item.installedId), alreadyInstalled: true })
        const skill = encarnar(estado, instalada(item))
        estado.instaladas.push(skill)
        item.installed = true
        item.installedId = skill.id
        return v2({ skill, alreadyInstalled: false })
      }
      case 'removeSkill': {
        // Só a remoção estrita: id, encarnação e recibo esperado capturados juntos.
        const esperado = corpo.expectedReceipt as { kind?: string; catalogId?: string; digest?: string } | undefined
        if (typeof corpo.id !== 'string' || typeof corpo.expectedInstallationId !== 'string' || !esperado || !['none', 'catalog'].includes(String(esperado.kind))) {
          throw fail('skill_installation_unsupported')
        }
        const skill = estado.instaladas.find((s) => s.id === corpo.id)
        if (!skill) return v2({})
        const recibo = skill.receipt
        const mesmoRecibo = esperado.kind === 'none' ? recibo === null : recibo !== null && recibo.catalogId === esperado.catalogId && recibo.digest === esperado.digest
        if (skill.installationId !== corpo.expectedInstallationId || !mesmoRecibo) throw fail('skill_installation_stale')
        if (skill.contentStatus !== 'verified') throw fail(skill.contentStatus === 'changed' ? 'skill_installation_changed' : 'skill_installation_unavailable')
        estado.instaladas = estado.instaladas.filter((s) => s !== skill)
        for (const i of estado.itens) if (i.installedId === skill.id) { i.installed = false; i.installedId = null }
        return v2({})
      }
    }
    throw fail('demo: método desconhecido')
  }
}
