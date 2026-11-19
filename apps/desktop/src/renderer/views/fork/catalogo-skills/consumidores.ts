import { passiveSkillsV2, type SkillClient } from '../../../../shared/fork-skills.ts'
import type { InstalledSkillV2, SkillDetailsV2 } from '../../../store/fork-skills.ts'
import { codigoDeSkill } from './regras.ts'
import type { Frase } from './textos.ts'

// Consumidores de skills fora da página Skills (D166 R8), com ganchos curtos no código herdado:
// a projeção do `Plugins` (contador e atalho de Extensões, `/` do Composer, seção Skills de
// Instalados) e a leitura da folha de uma skill nos ajustes do bot. Só leitura, protocolo forte.

/** O que estes consumidores leem do `BotStore`, sem alterá-lo: o Hub, o link vivo e a geração dele. */
export type FonteSkills = {
  readonly hello: { hostId: string } | null
  readonly connectedClient: SkillClient | null
  readonly principalEpoch: number
  ready: () => Promise<SkillClient>
}

/**
 * As instaladas do Hub de agora para quem só lista. Lê pelo wrapper forte e pelo link vivo; a resposta
 * só entra se for a do pedido mais novo e se Hub, link e geração ainda forem os do pedido. Mostra só a
 * projeção do Hub de agora (outro Hub: nenhuma, sem esperar releitura). Hub sem o protocolo forte:
 * nenhuma. Erro passageiro conserva a última do mesmo Hub.
 */
export class ProjecaoSkills {
  private lida: { hub: string; itens: InstalledSkillV2[] } | null = null
  private pedido = 0

  itens(fonte: FonteSkills): InstalledSkillV2[] {
    const hub = fonte.hello?.hostId ?? null
    return this.lida !== null && hub !== null && this.lida.hub === hub ? this.lida.itens : []
  }

  /** Relê do Hub de agora; `true` quando a projeção mudou. */
  async ler(fonte: FonteSkills): Promise<boolean> {
    const hub = fonte.hello?.hostId ?? null
    const client = fonte.connectedClient
    const geracao = fonte.principalEpoch
    if (hub === null || client === null) return false
    const meu = ++this.pedido
    const atual = () => meu === this.pedido && fonte.hello?.hostId === hub && fonte.connectedClient === client && fonte.principalEpoch === geracao
    try {
      const itens = await passiveSkillsV2.installed(client)
      if (!atual()) return false
      this.lida = { hub, itens }
      return true
    } catch (e) {
      if (!atual() || codigoDeSkill(e) !== 'skill_installation_unsupported') return false
      this.lida = { hub, itens: [] }
      return true
    }
  }
}

/**
 * A origem de uma folha de leitura (D166 R10): o Hub, o link vivo e a geração dele (`connectedClient` e
 * `principalEpoch`, a mesma tupla do F2), capturados no gesto que abriu a folha.
 */
export type OrigemLeitura = Readonly<{ hub: string; client: SkillClient | null; geracao: number }> | null
export const origemDaFonte = (fonte: FonteSkills): OrigemLeitura =>
  (fonte.hello ? { hub: fonte.hello.hostId, client: fonte.connectedClient, geracao: fonte.principalEpoch } : null)

/** A conexão viva ainda é exatamente a da origem: mesmo Hub, mesmo link e mesma geração. */
export function mesmaOrigem(fonte: FonteSkills, origem: OrigemLeitura): boolean {
  return origem !== null && origem.client !== null && fonte.hello?.hostId === origem.hub
    && fonte.connectedClient === origem.client && fonte.principalEpoch === origem.geracao
}

/**
 * Os detalhes de uma instalada para a folha dos ajustes do bot: pelo wrapper forte, pelo link da origem.
 * `null` quando a tupla mudou antes do pedido ou da resposta: nada dela aparece.
 */
export async function lerInstalada(fonte: FonteSkills, origem: OrigemLeitura, id: string): Promise<SkillDetailsV2 | null> {
  if (!origem || !mesmaOrigem(fonte, origem)) return null
  const client = await fonte.ready()
  // ready() pode terminar noutro link (outro Hub ou reconexão): nada de ler por ele para esta folha.
  if (!mesmaOrigem(fonte, origem) || client !== origem.client) return null
  const lido = await passiveSkillsV2.details(client, { id })
  return mesmaOrigem(fonte, origem) ? lido : null
}

/** Uma leitura da folha, presa à origem e à tentativa em que foi pedida. */
export type LeituraFolha = Readonly<{
  origem: NonNullable<OrigemLeitura>
  tentativa: number
  resultado: { detalhes: SkillDetailsV2 } | { erro: Frase; codigo: string | null }
}>

/** O que a folha pode mostrar agora. Só `pronta` traz dados da skill; os outros estados são neutros. */
export type EstadoFolha =
  | { tipo: 'pronta'; detalhes: SkillDetailsV2 }
  | { tipo: 'lendo' }
  | { tipo: 'outroHub' }
  | { tipo: 'conexaoMudou'; semLink: boolean }
  | { tipo: 'semProtocolo' }
  | { tipo: 'conteudo'; status: 'changed' | 'unavailable' }
  | { tipo: 'erro'; mensagem: Frase }

/**
 * A apresentação inteira da folha (D166 R10): só uma leitura feita nesta mesma origem, ainda viva, com o
 * protocolo não desmentido pela página do mesmo Hub, mostra dados da skill. Sem fallback para os dados
 * da linha que abriu a folha nem para uma leitura anterior.
 */
export function estadoDaFolha(fonte: FonteSkills, origem: OrigemLeitura, tentativa: number, leitura: LeituraFolha | null, semProtocoloNoHub: boolean): EstadoFolha {
  const hubAgora = fonte.hello?.hostId ?? null
  if (origem === null) return { tipo: 'conexaoMudou', semLink: true }
  if (hubAgora !== null && hubAgora !== origem.hub) return { tipo: 'outroHub' }
  if (!mesmaOrigem(fonte, origem)) return { tipo: 'conexaoMudou', semLink: hubAgora === null || fonte.connectedClient === null }
  if (semProtocoloNoHub) return { tipo: 'semProtocolo' }
  if (leitura === null || leitura.origem !== origem || leitura.tentativa !== tentativa) return { tipo: 'lendo' }
  const r = leitura.resultado
  if ('detalhes' in r) return { tipo: 'pronta', detalhes: r.detalhes }
  if (r.codigo === 'skill_installation_unsupported') return { tipo: 'semProtocolo' }
  if (r.codigo === 'skill_installation_changed') return { tipo: 'conteudo', status: 'changed' }
  if (r.codigo === 'skill_installation_unavailable') return { tipo: 'conteudo', status: 'unavailable' }
  return { tipo: 'erro', mensagem: r.erro }
}
