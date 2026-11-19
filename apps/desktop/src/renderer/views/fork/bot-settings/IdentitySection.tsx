import { useId, useState, type ReactNode } from 'react'
import { avatarDoBot, avatarMexeriquinhaHubNotice, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import { FUNCAO_LIMITE } from '@shared/fork-bot-funcao'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { ESTADOS, type EstadoId } from '../../../lib/mexeriquinhas/index.ts'
import { SeletorMexeriquinha } from '../mexeriquinha/SeletorMexeriquinha'
import { useStore } from '../../../store/context'
import { useModel } from '../../../lib/observable'
import type { BotSettingsStore, EditorState } from './bot-settings-store'
import '../../../fork/avatars/avatars.css'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { textoDaMensagem } from './mensagem'

// Só prévia dos cinco estados reais das Mexeriquinhas; nada é salvo daqui.
const SIZES = [18, 22, 24, 28, 36]

export function IdentitySection({ state, model, blocked = false, fixed = false, preferences }: { state: EditorState; model: BotSettingsStore; blocked?: boolean; fixed?: boolean; preferences?: ReactNode }) {
  const id = useId()
  const [estado, setEstado] = useState<EstadoId>('livre')
  const hello = useModel(useStore()).hello
  const t = useT()
  const idioma = superficieIdioma(useIdioma())
  if (!state.draft || !state.editor) return null
  const { draft, editor } = state
  const avatar = normalizarAvatarDoBot(avatarDoBot(draft), draft.name, draft.description)
  const nomeEstado = ESTADOS.find((e) => e.id === estado)!.nome
  // Hub sem o formato v2: o seletor fica bloqueado com o aviso (um Hub antigo recusaria a gravação).
  const avisoHub = avatarMexeriquinhaHubNotice(hello)
  const disabled = blocked || !state.online || !editor.permissions.canEdit
  return <section id="bot-settings-identity" aria-labelledby={`${id}-heading`}>
    <h2 id={`${id}-heading`} tabIndex={-1} {...idioma}>{t('botSettings.abas.identity')}</h2>
    <div className="bot-settings-card">
      {fixed ? <div className="bot-settings-readonly" role="group" aria-labelledby={`${id}-name-label`} {...idioma}>
        <h3 id={`${id}-name-label`}>{t('botSettings.identidade.nome')}</h3>
        <p dir="auto">{draft.name}</p>
        <p className="bot-settings-help">{t('botSettings.identidade.funcaoFixa')}</p>
      </div> : <>
        <label htmlFor={`${id}-name`} {...idioma}>{t('botSettings.identidade.nome')}</label>
        <input id={`${id}-name`} className="field-box" value={draft.name} disabled={disabled || !editor.capabilities.editableFields.includes('name')}
          onChange={(e) => model.change('name', e.target.value)} onBlur={() => { void model.flush() }} autoComplete="off" />
        <p className="bot-settings-help" {...idioma}>{t('botSettings.identidade.nomeAjuda')}</p>
      </>}
      {fixed ? <div className="bot-settings-readonly" role="group" aria-labelledby={`${id}-title-label`} {...idioma}>
        <h3 id={`${id}-title-label`}>{t('botSettings.identidade.funcao')}</h3>
        <p dir="auto">{draft.title?.trim() || t('botSettings.pagina.principal')}</p>
        <p className="bot-settings-help">{t('botSettings.identidade.funcaoFixa')}</p>
      </div> : editor.capabilities.editableFields.includes('title') ? <>
        <label htmlFor={`${id}-title`} {...idioma}>{t('botSettings.identidade.funcao')}</label>
        <input id={`${id}-title`} className="field-box" value={draft.title ?? ''} maxLength={FUNCAO_LIMITE} placeholder={t('botSettings.identidade.exemploFuncao')} disabled={disabled}
          aria-describedby={`${id}-title-help`} onChange={(e) => model.change('title', e.target.value)} onBlur={() => { void model.flush() }} autoComplete="off" />
        <p id={`${id}-title-help`} className="bot-settings-help" {...idioma}>{t('botSettings.identidade.funcaoAjuda', { limite: FUNCAO_LIMITE })}</p>
      </> : null}
      {/* D368: na Donna, as instruções base são texto de leitura; a autoridade continua no Hub. */}
      {fixed ? <div className="bot-settings-readonly" role="group" aria-labelledby={`${id}-instructions-label`} {...idioma}>
        <h3 id={`${id}-instructions-label`}>{t('botSettings.donnaFixa.instrucoesBase')}</h3>
        <p dir="auto">{draft.description}</p>
        <p className="bot-settings-help">{t('botSettings.donnaFixa.instrucoesBaseAjuda')}</p>
      </div> : <>
        <label htmlFor={`${id}-instructions`} {...idioma}>{t('botSettings.identidade.instrucoes')}</label>
        <p id={`${id}-instructions-help`} className="bot-settings-help" {...idioma}>{t('botSettings.identidade.instrucoesAjuda')}</p>
        <textarea id={`${id}-instructions`} className="field-box" rows={5} value={draft.description} aria-describedby={`${id}-instructions-help ${id}-application`}
          disabled={disabled || !editor.capabilities.editableFields.includes('description')} onChange={(e) => model.change('description', e.target.value)} onBlur={() => { void model.flush() }} />
        <p id={`${id}-application`} className="bot-settings-help" {...idioma}>{t('botSettings.identidade.aplicacao')}</p>
      </>}
      {/* Auditoria D249, F4: o "Salvo" fica só no cabeçalho; aqui só o que pede atenção junto do campo. */}
      {state.error || state.dirty ? <div className="bot-settings-field-status">{state.error ? textoDaMensagem(t, { chave: state.error }) : t('bots.ajustes.naoSalvas')}</div> : null}
    </div>
    {preferences}
    <div className="bot-settings-card">
      <h3 className="avatar-profile-heading" {...idioma}>{t('bots.ajustes.aparencia')}</h3>
      {/* D169 (components/AvatarEditor): the reference's profile column beside the editor (below it when
          the card is narrow): big face, name and role, mood preview, sizes and the real save note. */}
      <div className="avatar-profile">
        <div className="avatar-profile-column">
          <div className="avatar-profile-hero">
            <DonnabotAvatar config={avatar} size={168} estado={estado} label={t('bots.ajustes.aparenciaDe', { bot: draft.name, humor: nomeEstado.toLocaleLowerCase() })} />
          </div>
          <div className="avatar-profile-name"><b dir="auto">{draft.name}</b><span>{draft.title?.trim() || (fixed ? t('botSettings.pagina.principal') : t('botSettings.identidade.semFuncao'))}</span></div>
          <div role="group" aria-label={t('bots.ajustes.previsualizar')} className="avatar-profile-moods">
            {ESTADOS.map((e) => <button key={e.id} type="button" className="avatar-profile-state" aria-pressed={estado === e.id} onClick={() => setEstado(e.id)}>{e.nome}</button>)}
          </div>
          <div className="avatar-profile-sizes" aria-label={t('bots.ajustes.tamanhos')} role="img">
            {SIZES.map((size) => <span key={size} className="avatar-profile-size" aria-hidden><DonnabotAvatar config={avatar} size={size} label="" animated={false} /><span>{size}</span></span>)}
          </div>
          <p className="avatar-profile-note" {...idioma}>{t('bots.ajustes.salvaNaHora')}</p>
        </div>
        <fieldset disabled={disabled || !!avisoHub || !editor.capabilities.editableFields.includes('avatarConfig') || !editor.capabilities.editableFields.includes('mexeriquinha')} aria-label={t('bots.ajustes.aparenciaBot')}>
          {/* Fase 2a: as Mexeriquinhas (Simples e Pelúcia 3D). Só a aparência; nome e instruções ficam nos campos acima. */}
          {avisoHub ? <p className="bot-settings-help" role="status">{avisoHub}</p> : null}
          <SeletorMexeriquinha value={avatar} disabled={disabled || !!avisoHub} onChange={(config) => model.changeAvatar(config)} />
        </fieldset>
      </div>
    </div>
  </section>
}
