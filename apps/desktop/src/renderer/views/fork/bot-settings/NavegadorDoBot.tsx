import { useEffect, useState, type ReactNode } from 'react'
import type { Bot, ScreenState } from '@shared/models'
import { browserStatusRequest, parseBrowserStatus } from '@shared/fork-browser'
import type { BotEditor, BotEditorProfile } from '@shared/fork-bot-editor'
import { Button, CardSection, ChoiceList } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { VStack } from '../../../components/Stack'
import { useStore } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import type { BotSettingsStore } from './bot-settings-store'
import { motivoDoIntegrado, nomeDoComputador, preferenciaConfirmadaDoNavegador, preferenciaDoNavegador, situacaoDoComputador, type MotivoDoIntegrado } from './navegador'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { VerComputadores } from '../computador/UsarOComputador'

// O mesmo Space padrão que TelaPorComputador usa quando o mirror ainda não traz spaceId.
const ESPACO_PADRAO = '00000000-0000-4000-8000-000000000001'

/**
 * Grupo "Navegador" da aba Comportamento (entrega E1 + adendo E2, decisões do dono de 10/11/2026):
 * Meu computador, Integrado ao app e Desligado. A escolha é do editor versionado e salva como
 * as demais. Desligado = só navegador (escopo revisto pelo dono em 10/11): não mexe em
 * "Usar o computador" — o detalhe da opção aponta para o controle logo acima.
 */
export function NavegadorDoBot({ bot, screen, editor, draft, model, disabled }: {
  bot: Bot
  screen: ScreenState | null
  editor: BotEditor | null
  draft: BotEditorProfile | null
  model: BotSettingsStore
  disabled: boolean
}) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const efetiva = preferenciaDoNavegador(draft, bot)
  // O quadro de estado só segue o valor que o Hub confirmou; o rascunho pendente ou uma
  // gravação recusada não mostram "salvo" nem mudam o que a tela afirma.
  const confirmada = preferenciaConfirmadaDoNavegador(editor?.profile, bot)
  const editavel = !!editor?.permissions.canEdit && editor.capabilities.editableFields.includes('browserPreference')
  const motivo = useMotivoIntegrado(store, bot, confirmada === 'integrated')

  const escolher = (id: 'computer' | 'integrated' | 'off') => {
    if (id === efetiva) return
    model.change('browserPreference', id)
  }

  return <CardSection title={t('ajustes.navegador.titulo')}>
    <VStack gap={10} {...superficieIdioma(idioma)}>
      <fieldset className="navegador-opcoes" disabled={disabled || !editavel}>
        <ChoiceList
          label={t('ajustes.navegador.titulo')}
          selection={efetiva}
          options={[
            { id: 'computer', label: t('ajustes.navegador.computador'), detail: t('ajustes.navegador.computadorDetalhe') },
            { id: 'integrated', label: t('ajustes.navegador.integrado'), detail: `${t('ajustes.navegador.integradoTag')}. ${t('ajustes.navegador.integradoDetalhe')}` },
            { id: 'off', label: t('ajustes.navegador.desligado'), detail: t('ajustes.navegador.desligadoDetalhe') },
          ]}
          onChange={escolher}
        />
      </fieldset>
      {confirmada === 'computer' ? <StatusComputador bot={bot} screen={screen} helloName={store.hello?.name} />
        : confirmada === 'integrated' ? <StatusIntegrado motivo={motivo} />
        : confirmada === 'off' ? <Caixa tom="ok" icone="checkmark">
          <b>{t('ajustes.navegador.desligadoSalvoTitulo')}</b>
          <p>{t('ajustes.navegador.desligadoSalvoDetalhe')}</p>
        </Caixa> : null}
    </VStack>
  </CardSection>
}

/** O motivo do integrado vem do `reason` estável do contrato v2; falha de consulta vira `generico`. */
function useMotivoIntegrado(store: BotStore, bot: Bot, ativa: boolean): MotivoDoIntegrado | 'carregando' {
  const [motivo, setMotivo] = useState<MotivoDoIntegrado | 'carregando'>('carregando')
  const hostId = store.hello?.hostId ?? ''
  const spaceId = bot.spaceId ?? ESPACO_PADRAO
  useEffect(() => {
    if (!ativa) return
    let vivo = true
    setMotivo('carregando')
    const origin = { hubId: hostId, spaceId, botId: bot.id }
    void (async () => {
      try {
        const raw = await (await store.ready()).call('donnaBrowserStatus', browserStatusRequest(origin, 'desktop'))
        const parsed = parseBrowserStatus(raw, origin, 'desktop')
        if (vivo) setMotivo(motivoDoIntegrado(parsed))
      } catch {
        if (vivo) setMotivo('generico')
      }
    })()
    return () => { vivo = false }
  }, [ativa, store, bot.id, spaceId, hostId])
  return motivo
}

function StatusComputador({ bot, screen, helloName }: { bot: Bot; screen: ScreenState | null; helloName?: string }) {
  const t = useT()
  const situacao = situacaoDoComputador(bot, screen)
  const nome = nomeDoComputador(bot, helloName)
  const nota = <p className="navegador-nota">{t('ajustes.navegador.naoLibera')}</p>
  if (situacao === 'pronto') {
    return <>
      <Caixa tom="ok" icone="checkmark">
        <span>{nome ? t('ajustes.navegador.pronto', { computador: nome }) : t('ajustes.navegador.prontoSemNome')}</span>
      </Caixa>
      {nota}
    </>
  }
  if (situacao === 'semUso') {
    return <>
      <Caixa tom="warn" icone="exclamationmark.triangle">
        <b>{t('ajustes.navegador.semUsoTitulo')}</b>
        <p>{t('ajustes.navegador.semUsoDetalhe')}</p>
        <div className="navegador-acoes">
          <Button kind="secondary" onClick={() => document.getElementById('bot-usa-computador')?.scrollIntoView({ block: 'center' })}>
            {t('ajustes.navegador.semUsoAcao')}
          </Button>
        </div>
      </Caixa>
      {nota}
    </>
  }
  return <>
    <Caixa tom="warn" icone="exclamationmark.triangle">
      <b>{t('ajustes.navegador.semTelaTitulo')}</b>
      <p>{t('ajustes.navegador.semTela')}</p>
      <div className="navegador-acoes"><VerComputadores /></div>
    </Caixa>
    {nota}
  </>
}

function StatusIntegrado({ motivo }: { motivo: MotivoDoIntegrado | 'carregando' }) {
  const t = useT()
  return <Caixa tom="quiet" icone="info.circle">
    <b>{t('ajustes.navegador.indisponivel')}</b>
    {motivo !== 'carregando' ? <p>{t(`ajustes.navegador.motivo.${motivo}`)}</p> : null}
    <p>{t('ajustes.navegador.integradoExplica')}</p>
  </Caixa>
}

function Caixa({ tom, icone, children }: { tom: 'ok' | 'warn' | 'quiet'; icone: string; children: ReactNode }) {
  return <div className={`navegador-caixa is-${tom}`} role={tom === 'warn' ? 'alert' : undefined}>
    <Icon name={icone} size={15} />
    <div className="navegador-caixa-corpo">{children}</div>
  </div>
}
