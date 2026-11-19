import type { InstalledSkillV2, SkillFile, SkillReceipt } from '../../../../shared/fork-skills.ts'
import type { IntencaoHub } from './catalogo-store.ts'

/** Somente apresentação: nunca recebe um cliente, callback, caminho do disco ou autoridade nova. */
export type ResumoDaInstalacao = {
  vigente: boolean
  origem: 'manual' | 'catalogo' | 'legado' | null
  endereco: string | null
  arquivos: SkillFile[] | null
}

/** O estado único da Conferência da folha (componente existente `ConferenciaSkill`). */
export type EstadoConferencia = 'verified' | 'changed' | 'unavailable' | 'semRecibo' | 'anterior'

/** Os sinais vivos que a loja já usa (`catalogoFor`): mesma forma do BotStore, só lidos. */
export type SinaisDaConexao = { readonly hello: { readonly hostId: string } | null; readonly connectedClient: unknown; readonly principalEpoch: number }

/** O que a folha já calcula sobre a versão aberta e a resposta da consulta atual. */
export type SinaisDaFolha = {
  /** A versão (id, encarnação, estado, recibo) mudou desde a abertura. */
  mudou: boolean
  /** O Hub respondeu os detalhes por outra versão. */
  outraNoHub: boolean
  /** Há resposta da consulta atual e ela é desta versão (`detalhesDaVersao`). */
  detalhesDaVersao: boolean
  conteudoRecusado: 'changed' | 'unavailable' | null
}

/**
 * A leitura exibida ainda é desta conexão e desta instalação: a origem capturada na abertura, com
 * cliente, geração e Hub vivos iguais, a lista lida nessa origem, a mesma versão e nenhuma resposta
 * de outra versão. Só reutiliza guardas existentes; não recaptura origem.
 */
export function leituraVigente(
  loja: { conexaoAtual(o: IntencaoHub | null): boolean; listasDaOrigem(o: IntencaoHub): boolean },
  conexao: SinaisDaConexao,
  origem: IntencaoHub | null,
  folha: Pick<SinaisDaFolha, 'mudou' | 'outraNoHub'>,
): boolean {
  return origem !== null && origem.cliente !== null && loja.conexaoAtual(origem)
    && conexao.connectedClient === origem.cliente && conexao.principalEpoch === origem.geracao
    && conexao.hello?.hostId === origem.hub && loja.listasDaOrigem(origem)
    && !folha.mudou && !folha.outraNoHub
}

/** O positivo exige vigência, recibo e a resposta atual sem recusa; o resto é neutro, sem causa. */
export function estadoDaConferencia(vigente: boolean, recibo: SkillReceipt | null, folha: SinaisDaFolha): EstadoConferencia {
  if (!vigente) return 'anterior'
  if (folha.conteudoRecusado) return folha.conteudoRecusado
  if (!recibo) return 'semRecibo'
  return folha.detalhesDaVersao ? 'verified' : 'unavailable'
}

const objeto = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const idValido = (id: string) => id.length > 0 && id.length <= 64 && !id.startsWith('-') && !id.endsWith('-') && !/[^a-z0-9-]/.test(id)

/** Mesma forma ASCII de registry::safe_path; não normaliza nem tenta consertar entrada hostil. */
function caminhoValido(path: unknown): path is string {
  if (typeof path !== 'string' || path.length > 240) return false
  const partes = path.split('/')
  return partes.length <= 5 && partes.every((p) => p.length > 0 && p.length <= 100 && !p.startsWith('.') && !/[^a-zA-Z0-9._-]/.test(p))
}

/** Manifesto DECLARADO, não inventário/verificação do disco. Um erro recusa a lista inteira. 1–100 como o produtor. */
export function manifestoApresentavel(raw: unknown): SkillFile[] | null {
  if (!Array.isArray(raw) || raw.length < 1 || raw.length > 100) return null
  const arquivos: SkillFile[] = []
  const nomes = new Set<string>()
  let bytes = 0
  for (const f of raw) {
    if (!objeto(f) || !caminhoValido(f.path) || f.mode !== '100644'
      || typeof f.bytes !== 'number' || !Number.isSafeInteger(f.bytes) || f.bytes < 0 || f.bytes > 1024 * 1024
      || typeof f.sha256 !== 'string' || f.sha256.length !== 64 || /[^0-9a-f]/.test(f.sha256)) return null
    // Caminhos aceitos só têm ASCII: lowerCase aqui equivale a to_ascii_lowercase no produtor.
    const nome = f.path.toLowerCase()
    if (nomes.has(nome)) return null
    nomes.add(nome)
    bytes += f.bytes
    if (bytes > 5 * 1024 * 1024) return null
    arquivos.push({ path: f.path, bytes: f.bytes, sha256: f.sha256, mode: '100644' })
  }
  return nomes.has('skill.md') && arquivos.some((f) => f.path === 'SKILL.md') ? arquivos : null
}

/** A folha calcula vigência por `leituraVigente`, fora desta projeção pura e efêmera. */
export function resumoDaInstalada(skill: InstalledSkillV2, vigente: boolean): ResumoDaInstalacao {
  if (!vigente) return { vigente, origem: null, endereco: null, arquivos: null }
  return {
    vigente,
    origem: skill.receipt ? 'catalogo' : skill.source === 'custom' ? 'manual' : 'legado',
    endereco: idValido(skill.id) ? `Skills / ${skill.id} / SKILL.md` : null,
    arquivos: skill.receipt ? manifestoApresentavel(skill.receipt.files) : null,
  }
}
