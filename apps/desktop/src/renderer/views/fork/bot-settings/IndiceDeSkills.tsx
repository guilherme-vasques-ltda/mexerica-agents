import { useEffect, useState } from 'react'
import type { Bot } from '@shared/models'
import type { BotStore } from '../../../store/bot-store'
import { loadPlugins, type InstalledSkill, type Plugins } from '../../bots/drafts'
import { mesmaOrigem, origemDaFonte } from '../catalogo-skills/consumidores'
import { estadoDoIndice, lerSkillsDoBot, textoDoIndice, versaoDaSelecao, type LeituraIndice } from './indice-skills'
import { tradutor, useIdioma } from '../../../lib/fork-i18n/estado'

export type PluginsDoBot = Plugins & { indice: LeituraIndice | null }

/**
 * R3: `usePlugins` da seção Ferramentas de um bot, com a medida do índice na mesma leitura de skills.
 * Lê ao abrir, ao voltar a conexão, em Tentar de novo e quando a versão da seleção na tela muda; nada
 * de timer. Na mudança de seleção a lista fica na tela e só as skills são relidas, uma vez por versão.
 */
export function usePluginsDoBot(store: BotStore, bot: Bot): PluginsDoBot | null {
  const [plugins, setPlugins] = useState<PluginsDoBot | null>(null)
  const [attempt, setAttempt] = useState(0)
  const online = store.connection.kind === 'online'
  const client = store.connectedClient
  const hub = store.hello?.hostId
  const geracao = store.principalEpoch
  const versao = versaoDaSelecao(bot)
  useEffect(() => {
    if (!client || !hub) return
    const origem = origemDaFonte(store)
    let live = true
    let indice: LeituraIndice | null = null
    void loadPlugins(store, async (client) => {
      const lido = await lerSkillsDoBot<InstalledSkill>(store, client, bot.id, versao)
      indice = lido.leitura
      return lido
    }).then((p) => live && mesmaOrigem(store, origem) && setPlugins({ ...p, indice, retry: () => { setPlugins(null); setAttempt((n) => n + 1) } }))
    return () => { live = false }
  }, [store, online, attempt, bot.id, client, hub, geracao])
  const pedidaPara = plugins?.indice?.pedidaPara
  useEffect(() => {
    if (!plugins || pedidaPara === versao || !client || !hub) return
    // A origem nova já está sendo lida pelo efeito acima; não reler a seleção da antiga.
    if (plugins.indice?.origem && !mesmaOrigem(store, plugins.indice.origem)) return
    const origem = origemDaFonte(store)
    let live = true
    const falhou: LeituraIndice = { origem: null, pedidaPara: versao, versao: null, indice: null, lidoEm: Date.now() }
    void lerSkillsDoBot<InstalledSkill>(store, client, bot.id, versao)
      .then(({ items, leitura }) => live && mesmaOrigem(store, origem) && setPlugins((p) => p && { ...p, skills: items, indice: leitura }))
      .catch(() => live && mesmaOrigem(store, origem) && setPlugins((p) => p && { ...p, indice: falhou }))
    return () => { live = false }
  }, [store, bot.id, versao, pedidaPara, plugins !== null, client, hub, geracao])
  return plugins
}

/** Ao lado do título Skills: a medida, "Conferindo o índice…" ou indisponível; nunca um zero que o Hub não confirmou. */
export function IndiceDeSkills({ store, bot, leitura }: { store: BotStore; bot: Bot; leitura: LeituraIndice | null }) {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const estado = estadoDoIndice(store, leitura, versaoDaSelecao(bot))
  const medida = estado.tipo === 'medido' ? textoDoIndice(t, idioma, estado.indice, estado.lidoEm) : null
  const texto = medida ? medida.linha : t(estado.tipo === 'conferindo' ? 'bots.ajustes.indice.conferindo' : 'bots.ajustes.indice.indisponivel')
  const dica = medida ? t('bots.ajustes.indice.dica', { linha: medida.linha, detalhe: medida.detalhe }) : texto
  return <span className="bot-settings-indice-skills" title={dica} aria-label={dica}>{texto}</span>
}
