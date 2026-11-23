import type { Bot } from '@shared/models'
import { BotAvatar } from '../../../components/Avatar'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useT } from '../../../lib/fork-i18n/estado'
import { STARTER_TASKS } from '../../../../shared/fork-onboarding.ts'

/**
 * Etapa 4 (primeira conversa util com o Berga):
 * 6 cartoes clicaveis que preenchem o compositor editavel sem envio automatico.
 */
export function FirstChatOnboarding({
  bot,
  hasReadyAi,
  onPickTask,
  onConnectAi,
}: {
  bot: Bot
  hasReadyAi: boolean
  onPickTask: (promptText: string) => void
  onConnectAi?: () => void
}) {
  const t = useT()
  return (
    <section className="onboarding-first-chat" aria-label={t('onboarding.passo4.bergaTitulo')}>
      <div className="onboarding-first-chat-head">
        <BotAvatar bot={bot} size={64} />
        <h2>{t('onboarding.passo4.bergaTitulo')}</h2>
        <p>{t('onboarding.passo4.bergaSubtitulo')}</p>
      </div>

      {!hasReadyAi ? (
        <div className="onboarding-no-ai-banner" role="status">
          <Icon name="exclamationmark.triangle" size={15} />
          <span>{t('onboarding.passo4.semIaAviso')}</span>
          {onConnectAi ? (
            <Button kind="secondary" onClick={onConnectAi}>{t('onboarding.passo4.conectarAgora')}</Button>
          ) : null}
        </div>
      ) : null}

      <div className="onboarding-starter-grid">
        {STARTER_TASKS.map((task) => (
          <button
            key={task.id}
            type="button"
            className="onboarding-starter-card"
            onClick={() => onPickTask(t(task.promptKey))}
          >
            <span className="onboarding-starter-icon" aria-hidden="true">
              <Icon name={task.icon} size={18} />
            </span>
            <span className="onboarding-starter-copy">
              <strong>{t(task.titleKey)}</strong>
              <span>{t(task.descKey)}</span>
            </span>
          </button>
        ))}
      </div>
    </section>
  )
}
