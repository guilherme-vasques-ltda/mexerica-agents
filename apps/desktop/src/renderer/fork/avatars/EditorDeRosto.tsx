// D478: o editor de rosto com as frutinhas primeiro. Escolher uma aplica o preset da função dela; o editor de robô da
// biblioteca (D115, intacto) fica inteiro em "Montar robô (avançado)", aberto de início só quando o rosto é um robô.
import { useState, type ReactNode } from 'react'
import type { AvatarConfig } from '@shared/fork-avatar-config'
import { arteDoAvatar, configDaFrutinha, FRUTINHAS } from '@shared/fork-frutinhas'
import { useT } from '../../lib/fork-i18n/estado'
import { RostoParado } from './Avatar'
import './avatars.css'

export function EditorDeRosto({ value, onChange, children }: { value: AvatarConfig; onChange: (config: AvatarConfig) => void; children: ReactNode }) {
  const t = useT()
  const atual = arteDoAvatar(value)
  const [robo, setRobo] = useState(() => atual === null)
  return <div className="editor-rosto">
    <div className="editor-rosto-titulo">{t('botRoles.frutinha.titulo')}</div>
    <div className="editor-rosto-grade" role="group" aria-label={t('botRoles.frutinha.titulo')}>
      {FRUTINHAS.map((f) => {
        const config = configDaFrutinha(f)
        const nome = t(`botRoles.frutinha.${f}`)
        return <button key={f} type="button" className="editor-rosto-fruta" aria-pressed={atual === f} aria-label={t('botRoles.frutinha.usar', { nome })}
          onClick={() => onChange(config)}>
          <RostoParado config={config} size={44} />
          <span>{nome}</span>
        </button>
      })}
    </div>
    <details className="editor-rosto-robo" open={robo} onToggle={(e) => setRobo(e.currentTarget.open)}>
      <summary>{t('botRoles.frutinha.montarRobo')}</summary>
      <p className="editor-rosto-nota">{t('botRoles.frutinha.notaSelo')}</p>
      {robo ? children : null}
    </details>
  </div>
}
