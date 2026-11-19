// Passo 3 de conectar ("Usar em quais bots?"): a lista de bots do Space com busca, quem é cada um (rosto, nome,
// função), a equipe como título de grupo e "Marcar todos do grupo". O principal do Space aparece desabilitado, com o
// motivo na linha. Só desenha: quem agrupa e filtra é regras.ts; quem recusa o principal é o Hub.
import { useState } from 'react'
import { SearchField } from '../../../components/Controls'
import { VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { BotAvatar } from '../../../fork/avatars/Avatar'
import { useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { alternarVarios, BLOCO_SEM_EQUIPE, BOTS_PARA_BUSCAR, blocosDeBots, type EscolhaDeBot } from './regras.ts'

export function EscolherBots({ escolhas, marcados, aoMudar, desabilitado }: {
  escolhas: readonly EscolhaDeBot[]
  marcados: ReadonlySet<string>
  aoMudar: (marcados: Set<string>) => void
  desabilitado: boolean
}) {
  const t = useT()
  const [busca, setBusca] = useState('')
  const blocos = blocosDeBots(escolhas, busca)
  const alternar = (id: string) => { const novo = new Set(marcados); if (novo.has(id)) novo.delete(id); else novo.add(id); aoMudar(novo) }
  const comBusca = escolhas.length > BOTS_PARA_BUSCAR
  return (
    <VStack gap={10}>
      {comBusca ? <SearchField value={busca} onChange={setBusca} prompt={t('mercado.bots.busca')} clearLabel={t('mercado.busca.limpar')} /> : null}
      {blocos.length === 0 ? (
        <Text size="callout" tone="secondary" as="div" role="status">{t('mercado.bots.semResultado', { busca: busca.trim() })}</Text>
      ) : (
        <div className="mercado-bots-rolagem" role="group" aria-label={t('mercado.bots.titulo')}>
          {blocos.map((bloco) => {
            const titulo = bloco.titulo ?? (bloco.chave === BLOCO_SEM_EQUIPE ? t('mercado.bots.semEquipe') : null)
            const todos = bloco.marcaveis.length > 0 && bloco.marcaveis.every((id) => marcados.has(id))
            return (
              <section key={bloco.chave} className="mercado-bots-bloco" aria-label={titulo ?? undefined}>
                {titulo ? (
                  <div className="mercado-bots-cabeca">
                    <Text size="caption" weight="semibold" tone="secondary" truncate title={titulo}>{titulo}</Text>
                    {bloco.marcaveis.length > 1 ? (
                      <button className="mercado-link" disabled={desabilitado} aria-label={t(todos ? 'mercado.bots.desmarcarGrupoDe' : 'mercado.bots.marcarGrupoDe', { grupo: titulo })} onClick={() => aoMudar(alternarVarios(marcados, bloco.marcaveis))}>
                        <Text size="caption" weight="semibold">{t(todos ? 'mercado.bots.desmarcarGrupo' : 'mercado.bots.marcarGrupo')}</Text>
                      </button>
                    ) : null}
                  </div>
                ) : null}
                <div className="mercado-bots">
                  {bloco.bots.map((b) => <LinhaDoBot key={b.id} escolha={b} marcado={marcados.has(b.id)} desabilitado={desabilitado} aoAlternar={alternar} />)}
                </div>
              </section>
            )
          })}
        </div>
      )}
    </VStack>
  )
}

function LinhaDoBot({ escolha, marcado, desabilitado, aoAlternar }: { escolha: EscolhaDeBot; marcado: boolean; desabilitado: boolean; aoAlternar: (id: string) => void }) {
  const store = useStore()
  const t = useT()
  const bot = store.bots.get(escolha.id)
  const motivo = `mercado-bot-motivo-${escolha.id}`
  const bloqueado = escolha.principal
  return (
    <label className="mercado-bot" aria-disabled={bloqueado || desabilitado}>
      <input type="checkbox" checked={!bloqueado && marcado} disabled={bloqueado || desabilitado} aria-describedby={bloqueado ? motivo : undefined} onChange={() => aoAlternar(escolha.id)} />
      <span className="mercado-bot-rosto" aria-hidden>{bot ? <BotAvatar bot={bot} size={28} animated={false} /> : null}</span>
      <VStack gap={0} grow shrink>
        <Text size="body" weight="medium" tone="text" truncate>{escolha.name}</Text>
        {bloqueado ? <Text size="caption" tone="secondary" id={motivo}>{t('mercado.bots.principal')}</Text>
          : escolha.funcao ? <Text size="caption" tone="secondary" truncate title={escolha.funcao}>{escolha.funcao}</Text> : null}
      </VStack>
    </label>
  )
}
