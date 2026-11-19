import type { BotAvatarConfig } from '@shared/fork-avatar-config'
import type { Bot } from '@shared/models'
import { Icon } from '../../components/Icon'
import { DonnabotAvatar } from '../../fork/avatars/Avatar'
import type { Mood } from '../../lib/donnabot-avatars/src'
import type { BotStore } from '../../store/bot-store'
import { BotRow } from '../BotRow'
import { useT } from '../../lib/fork-i18n/estado'

/** O dado do nome; desenhado aqui porque o conjunto de ícones do app não tem um. */
function Dado() {
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
    <rect x="3.5" y="3.5" width="17" height="17" rx="4" />
    {[[8.5, 8.5], [15.5, 8.5], [12, 12], [8.5, 15.5], [15.5, 15.5]].map(([x, y]) => <circle key={`${x}-${y}`} cx={x} cy={y} r="1.2" fill="currentColor" stroke="none" />)}
  </svg>
}

/**
 * A prévia ao vivo à esquerda (DON-865): rosto, Aleatório e Personalizar, nome com o dado, etiquetas,
 * resumo e a linha real da barra lateral. Só mostra o rascunho; quem grava é o botão final.
 */
export function Previa({ store, compacta, config, mood, nome, funcao, motor, onde, equipe, liberar, bot, onNome, onSortear, onAleatorio, onPersonalizar, onEnter }: {
  store: BotStore
  compacta: boolean
  config: BotAvatarConfig
  mood: Mood
  nome: string
  funcao: string
  motor: string
  onde: string
  equipe: string
  liberar: boolean
  bot: Bot
  onNome: (nome: string) => void
  onSortear: () => void
  onAleatorio: () => void
  onPersonalizar: () => void
  onEnter: () => void
}) {
  const t = useT()
  const permissao = liberar ? t('botCreate.liberar') : t('botCreate.pedir')
  const linhas: [string, string, string][] = [
    ['person.2', t('botCreate.resumoEquipe'), equipe],
    ['house.fill', t('botCreate.resumoOnde'), onde],
    ['doc.text', t('botCreate.resumoPastas'), t('botCreate.areaPropria')],
    ['shield', t('botCreate.resumoPermissoes'), permissao],
    ['puzzlepiece.extension', t('botCreate.resumoSkills'), t('botCreate.nenhuma')],
    ['point.3.connected.trianglepath.dotted', t('botCreate.resumoConectores'), t('botCreate.nenhum')],
  ]
  return <section className="nb-previa" aria-label={t('botCreate.previa')}>
    <div className="nb-palco">
      <DonnabotAvatar config={config} size={compacta ? 72 : 150} mood={mood}
        label={t('botCreate.rostoDe', { nome: nome.trim() || t('botCreate.semNome'), funcao: funcao || t('botCreate.semFuncao') })} />
      <div className="nb-palco-botoes">
        <button type="button" className="nb-chip-botao" onClick={onAleatorio}><Icon name="arrow.triangle.2.circlepath" size={13} />{t('botCreate.aleatorio')}</button>
        <button type="button" className="nb-chip-botao" onClick={onPersonalizar}><Icon name="slider.horizontal.3" size={13} />{t('botCreate.personalizar')}</button>
      </div>
    </div>
    <div className="nb-previa-dados">
      <div className="nb-nome-linha">
        <input className="field-box nb-nome-campo" autoFocus value={nome} placeholder={t('botCreate.nomeVazio')} aria-label={t('botCreate.nome')}
          onChange={(e) => onNome(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) onEnter()
          }} />
        <button type="button" className="nb-dado" title={t('botCreate.sortearNome')} aria-label={t('botCreate.sortearNome')} onClick={onSortear}><Dado /></button>
      </div>
      <div className="nb-etiquetas">
        {funcao ? <span className="nb-etiqueta nb-etiqueta-papel">{funcao}</span> : null}
        <span className="nb-etiqueta"><Icon name="server.rack" size={11} />{motor}</span>
        <span className="nb-etiqueta"><Icon name="shield" size={11} />{permissao}</span>
      </div>
      {compacta ? null : <dl className="nb-resumo" aria-label={t('botCreate.resumo')}>
        {linhas.map(([icone, rotulo, valor]) => <div key={rotulo} className="nb-resumo-linha">
          <dt><Icon name={icone} size={13} />{rotulo}</dt><dd>{valor}</dd>
        </div>)}
      </dl>}
    </div>
    {compacta ? null : <div className="nb-lateral" aria-label={t('botCreate.barraLateral')}>
      <div className="nb-lateral-linha"><BotRow bot={bot} store={store} compact={false} usingComputer={false} members={[]} /></div>
      <span className="nb-lateral-rotulo" aria-hidden="true">{t('botCreate.barraLateral')}</span>
    </div>}
  </section>
}
