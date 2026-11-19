// Fase 2b: regras puras do gerador "Crie o seu" 3D (sem React, sem I/O direto).
// O Hub executa tudo; aqui só intenções, estado e a config v3 da escolha salva.
import type { HostClient } from '../../../client/host-client'
import type { MexeriquinhaConfig } from '@shared/fork-mexeriquinha-config'
import { assetIdValido } from '@shared/fork-mexeriquinha-config'

export interface EstadoGerador { gerador: boolean; cofre: boolean; processador: boolean }
export interface JobGerador { id: string; status: 'queued' | 'generating' | 'ready' | 'error' | 'cancelled'; codigo?: string; conjunto?: { id: string; nome: string } }
export interface ConjuntoGerado { id: string; nome: string; criadoEm?: number }

/** Aparências que os atalhos aceitam; os IDs batem com `variantes.json` no Hub. */
export const ATALHOS = ['cor', 'olhos', 'roupa', 'acessorio'] as const
export type Atalho = (typeof ATALHOS)[number]
export interface Briefing { nome: string; descricao: string; atalhos: Partial<Record<Atalho, string>> }

export const briefingValido = (b: Briefing): boolean =>
  !!b.nome.trim() && b.nome.trim().length <= 80 && !!b.descricao.trim() && b.descricao.trim().length <= 2000
    && Object.entries(b.atalhos).every(([campo, id]) => (ATALHOS as readonly string[]).includes(campo) && typeof id === 'string' && !!id)

export const estadoGerador = (client: HostClient): Promise<EstadoGerador> =>
  client.call<EstadoGerador>('mexeriquinhaGeradorEstado', {}, 10_000)

/** A chave entra pelo campo e some ao enviar; nunca é lida de volta (só estado). */
export const salvarChave = (client: HostClient, chave: string): Promise<EstadoGerador> =>
  client.call<EstadoGerador>('mexeriquinhaGeradorChave', { chave }, 15_000)

export const gerar = (client: HostClient, briefing: Briefing): Promise<{ id: string }> =>
  client.call<{ id: string }>('mexeriquinhaGerar', briefing, 15_000)

export const job = (client: HostClient, id: string): Promise<JobGerador> =>
  client.call<JobGerador>('mexeriquinhaGeradorJob', { id }, 10_000)

export const cancelar = (client: HostClient, id: string): Promise<unknown> =>
  client.call('mexeriquinhaGeradorCancelar', { id }, 10_000)

export const salvarConjunto = (client: HostClient, id: string): Promise<{ conjunto: ConjuntoGerado }> =>
  client.call<{ conjunto: ConjuntoGerado }>('mexeriquinhaConjuntoSalvar', { id }, 15_000)

export const conjuntos = (client: HostClient): Promise<{ conjuntos: ConjuntoGerado[] }> =>
  client.call<{ conjuntos: ConjuntoGerado[] }>('mexeriquinhaConjuntos', {}, 10_000)

/** Config v3 persistida: assetId do conjunto salvo + conjunto do catálogo como fallback visual. */
export function configGerada(assetId: string, fallbackConjuntoId: string): MexeriquinhaConfig | null {
  return assetIdValido(assetId) ? { v: 3, estilo: '3d', assetId, fallbackConjuntoId } : null
}

/** Mensagens de erro chegam como código estável do Hub; a tela traduz pelo sufixo. */
export function codigoDoErro(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  const m = msg.match(/[A-Z_]{4,}/)
  return m?.[0] ?? 'GERACAO_FALHOU'
}

const ERRO_CHAVES = {
  COTA_ATINGIDA: 'botRoles.mexeriquinha.gerador.erro.COTA_ATINGIDA',
  GERADOR_OCUPADO: 'botRoles.mexeriquinha.gerador.erro.GERADOR_OCUPADO',
  GERADOR_INDISPONIVEL: 'botRoles.mexeriquinha.gerador.erro.GERADOR_INDISPONIVEL',
  ATLAS_INVALIDO: 'botRoles.mexeriquinha.gerador.erro.ATLAS_INVALIDO',
  BRIEFING_INVALIDO: 'botRoles.mexeriquinha.gerador.erro.BRIEFING_INVALIDO',
  CONJUNTO_INDISPONIVEL: 'botRoles.mexeriquinha.gerador.erro.CONJUNTO_INDISPONIVEL',
  GERADOR_SEM_PROCESSADOR: 'botRoles.mexeriquinha.gerador.erro.GERADOR_SEM_PROCESSADOR',
  GERACAO_FALHOU: 'botRoles.mexeriquinha.gerador.erro.GERACAO_FALHOU',
} as const
/** Só códigos com texto de catálogo; o resto cai na mensagem genérica. */
export function chaveErro(codigo: string): (typeof ERRO_CHAVES)[keyof typeof ERRO_CHAVES] {
  return codigo in ERRO_CHAVES ? ERRO_CHAVES[codigo as keyof typeof ERRO_CHAVES] : ERRO_CHAVES.GERACAO_FALHOU
}
