// Fase 2a das Mexeriquinhas: regras puras do seletor de aparência (sem React, sem I/O).
// Escolher um avatar muda só a aparência do bot (avatarConfig v2); nome, função e instruções ficam como estão.
import {
  CATEGORIAS,
  CONFIG_BASE,
  MODELOS,
  conjuntoDoModelo,
  modeloPorId,
  type AvatarConfig,
  type CategoriaId,
  type Modelo,
} from '../../../lib/mexeriquinhas/index.ts'
import type { MexeriquinhaConfig } from '../../../../shared/fork-mexeriquinha-config.ts'
import { avatarDoBot, avatarMexeriquinhaHubNotice, camposAvatarDoBot, normalizarAvatarDoBot, normalizedAvatar, type AvatarConfig as AvatarConfigLegado, type BotAvatarConfig } from '../../../../shared/fork-avatar-config.ts'

export type Estilo = MexeriquinhaConfig['estilo']
export type ConfigSimples = Extract<MexeriquinhaConfig, { estilo: 'simples' }>
export type Aparencia = ConfigSimples['aparencia']
export type Parte = keyof Aparencia

/** Cópia fechada das cinco peças; nunca devolve a referência do catálogo. */
export function aparenciaDe(c: AvatarConfig): Aparencia {
  return { cor: c.cor, olhos: c.olhos, roupa: c.roupa, acessorio: c.acessorio, objeto: c.objeto ?? 'nenhum' }
}

const dobrar = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().trim()

/** Os nomes prontos visíveis: busca sem acento por nome, e categoria opcional. Ordem do catálogo. */
export function filtrarModelos(busca: string, categoria: CategoriaId | '' = ''): Modelo[] {
  const q = dobrar(busca)
  return MODELOS.filter((m) => (!categoria || m.categoriaId === categoria) && (!q || dobrar(m.nome).includes(q)))
}

/** Categorias com pelo menos um nome no filtro atual, com os nomes delas. */
export function gruposVisiveis(busca: string, categoria: CategoriaId | '' = ''): { id: CategoriaId; nome: string; modelos: Modelo[] }[] {
  const visiveis = filtrarModelos(busca, categoria)
  return CATEGORIAS.map((c) => ({ id: c.id, nome: c.nome, modelos: visiveis.filter((m) => m.categoriaId === c.id) }))
    .filter((g) => g.modelos.length > 0)
}

/** Avatar pronto no estilo pedido: Simples leva as peças do nome; 3D leva o conjunto aprovado dele. */
export function configDoPronto(m: Modelo, estilo: Estilo): MexeriquinhaConfig {
  return estilo === 'simples'
    ? { v: 2, estilo: 'simples', modeloId: m.id, aparencia: aparenciaDe(m.config) }
    : { v: 2, estilo: '3d', modeloId: m.id, conjuntoId: conjuntoDoModelo(m).id }
}

/** "Crie o seu": a fruta base do Simples, sem nome pronto. Não depende de chave nem de servidor. */
export function configCriada(): ConfigSimples {
  return { v: 2, estilo: 'simples', aparencia: aparenciaDe(CONFIG_BASE) }
}

/**
 * Trocar uma peça no Simples. Se a aparência deixa de ser exatamente a do nome pronto, o nome sai:
 * o avatar passou a ser criado pela pessoa.
 */
export function comParte(atual: ConfigSimples, parte: Parte, valor: string): ConfigSimples {
  const aparencia = { ...atual.aparencia, [parte]: valor } as Aparencia
  const pronto = 'modeloId' in atual && atual.modeloId ? modeloPorId(atual.modeloId) : undefined
  const igual = pronto && (Object.keys(aparencia) as Parte[]).every((k) => aparenciaDe(pronto.config)[k] === aparencia[k])
  return { v: 2, estilo: 'simples', ...(igual ? { modeloId: pronto.id } : {}), aparencia }
}

/** Ponto de partida do editor Simples para a aparência atual (um 3D abre com as peças do seu nome, se houver). */
export function simplesDe(atual: MexeriquinhaConfig): ConfigSimples {
  if (atual.estilo === 'simples') return atual
  const m = 'modeloId' in atual && atual.modeloId ? modeloPorId(atual.modeloId) : undefined
  return m ? configDoPronto(m, 'simples') as ConfigSimples : configCriada()
}

/** O nome pronto marcado na galeria deste estilo, ou nenhum. */
export function prontoMarcado(atual: MexeriquinhaConfig, estilo: Estilo): string | null {
  return atual.estilo === estilo && 'modeloId' in atual && atual.modeloId ? atual.modeloId : null
}

/**
 * Campos de avatar enviados ao criar um bot. Hub com v2: avatarConfig legado (projeção) + mexeriquinha.
 * Hub que só conhece o robô: só o avatarConfig legado, sem mandar o formato novo a quem não o anuncia.
 */
export function camposDaCriacao(bot: { avatarConfig?: unknown; mexeriquinha?: unknown }, hello: { avatarConfigVersion?: number } | null | undefined): { avatarConfig: AvatarConfigLegado; mexeriquinha?: MexeriquinhaConfig } {
  const atual = avatarDoBot(bot)
  return avatarMexeriquinhaHubNotice(hello) ? { avatarConfig: normalizedAvatar(atual) } : camposAvatarDoBot(normalizarAvatarDoBot(atual))
}

/** Aplica uma aparência a um rascunho: os dois campos juntos; legado tira a mexeriquinha antiga (o Hub limpa a v2). */
export function comAvatar<T extends { avatarConfig?: unknown; mexeriquinha?: unknown }>(rascunho: T, avatar: BotAvatarConfig): Omit<T, 'avatarConfig' | 'mexeriquinha'> & ReturnType<typeof camposAvatarDoBot> {
  const { avatarConfig: _legado, mexeriquinha: _v2, ...resto } = rascunho
  return { ...resto, ...camposAvatarDoBot(avatar) }
}
