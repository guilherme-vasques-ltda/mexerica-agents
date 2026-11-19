import { useId, useRef, useState, type ReactNode } from 'react'
import type { MexeriquinhaConfig } from '@shared/fork-avatar-config'
import { FUNCAO_LIMITE, type Funcao } from '@shared/fork-bot-funcao'
import { FUNCOES_DA_CRIACAO } from '@shared/fork-criar-bot'
import { DonnabotAvatar } from '../../fork/avatars/Avatar'
import { Icon } from '../../components/Icon'
import { useT } from '../../lib/fork-i18n/estado'
import { rotuloDaFuncao } from './rotulos'
import { SeletorMexeriquinha } from '../fork/mexeriquinha/SeletorMexeriquinha'
import '../../fork/avatars/avatars.css'

// Fase 2a: Função e Aparência (as Mexeriquinhas, dois estilos). O editor de robô da biblioteca fica intacto, fora desta tela.
const ABAS = ['role', 'aparencia'] as const
export type Aba = typeof ABAS[number]

/** O id da função escolhida; `nova` para uma função do dono que ainda não existe em nenhum bot. */
export type Escolha = string | null

/**
 * As abas do Estúdio, por cima dos passos ("Ver as 30 funções" abre em Função; "Personalizar", em Aparência).
 * A aba Função é desta tela (exemplos, funções do dono, nome editável e "Nova função"); Aparência é o seletor
 * das Mexeriquinhas, que muda só o avatar.
 */
export function FuncaoPainel({ inicial = 'role', config, avisoAparencia = null, escolha, funcao, erro, proprias, onConfig, onEscolher, onNova, onFuncao }: {
  inicial?: Aba
  config: MexeriquinhaConfig
  /** Hub sem a v2: o seletor aparece desabilitado com este aviso; a criação continua. */
  avisoAparencia?: string | null
  escolha: Escolha
  funcao: string
  erro: string | null
  proprias: Funcao[]
  onConfig: (config: MexeriquinhaConfig) => void
  onEscolher: (funcao: Funcao) => void
  onNova: () => void
  onFuncao: (nome: string) => void
}) {
  const t = useT()
  const id = useId()
  const [aba, setAba] = useState<Aba>(inicial)
  const campo = useRef<HTMLInputElement>(null)
  const tamanho = [...funcao].length

  return <section className="nb-cartao nb-painel" aria-label={t('botCreate.abas')}>
    <div role="tablist" aria-label={t('botCreate.abas')} className="nb-abas">
      {ABAS.map((aba_) => <button key={aba_} type="button" role="tab" id={`${id}-${aba_}`} aria-selected={aba === aba_}
        aria-controls={`${id}-painel`} className="nb-aba" onClick={() => setAba(aba_)}>{aba_ === 'role' ? t('botCreate.aba.role') : t('bots.ajustes.aparencia')}</button>)}
    </div>
    <div role="tabpanel" id={`${id}-painel`} aria-labelledby={`${id}-${aba}`} className="nb-aba-corpo">
      {aba === 'role' ? <>
        <p className="nb-dica">{escolha === 'nova' ? t('botCreate.dicaNova') : t('botCreate.dica')}</p>
        <div className="nb-funcao">
          <label className="nb-rotulo" htmlFor={`${id}-funcao`}>{t('botCreate.funcao')}</label>
          <div className="nb-funcao-linha">
            <input ref={campo} id={`${id}-funcao`} className="field-box nb-funcao-campo" value={funcao} maxLength={FUNCAO_LIMITE}
              placeholder={t('botCreate.funcaoExemplo')} autoComplete="off" spellCheck={false} aria-invalid={!!erro}
              aria-describedby={`${id}-funcao-ajuda`} onChange={(e) => onFuncao(e.target.value)} />
            <span className={`nb-contador ${tamanho > FUNCAO_LIMITE ? 'nb-erro' : ''}`} aria-hidden="true">{tamanho}/{FUNCAO_LIMITE}</span>
          </div>
          <p id={`${id}-funcao-ajuda`} className={erro ? 'nb-nota nb-erro' : 'nb-nota'} role={erro ? 'alert' : undefined}>{erro ?? t('botCreate.funcaoAjuda')}</p>
        </div>
        <Grupo titulo={t('botCreate.suasFuncoes')}>
          <button type="button" className="nb-tile nb-tile-nova" aria-pressed={escolha === 'nova'}
            onClick={() => { onNova(); campo.current?.focus() }}>
            <span className="nb-tile-mais" aria-hidden="true"><Icon name="plus" size={18} weight="semibold" /></span>
            <span className="nb-tile-nome">{t('botCreate.novaFuncao')}</span>
            <span className="nb-tile-linha">{t('botCreate.novaFuncaoLinha')}</span>
          </button>
          {proprias.map((f) => <Tile key={f.id} funcao={f} ativo={escolha === f.id} onClick={() => onEscolher(f)} />)}
        </Grupo>
        <Grupo titulo={t('botCreate.exemplos')} detalhe={`${FUNCOES_DA_CRIACAO.length}`}>
          {FUNCOES_DA_CRIACAO.map((f) => <Tile key={f.id} funcao={f} ativo={escolha === f.id} onClick={() => onEscolher(f)} />)}
        </Grupo>
      </> : <>{avisoAparencia ? <p className="nb-nota" role="status">{avisoAparencia}</p> : null}
        <SeletorMexeriquinha value={config} disabled={!!avisoAparencia} onChange={onConfig} /></>}
    </div>
  </section>
}

function Grupo({ titulo, detalhe, children }: { titulo: string; detalhe?: string; children: ReactNode }) {
  return <div className="nb-grupo">
    <div className="nb-grupo-titulo"><span className="nb-rotulo">{titulo}</span>{detalhe ? <span className="nb-grupo-detalhe">{detalhe}</span> : null}</div>
    <div className="nb-grade">{children}</div>
  </div>
}

function Tile({ funcao, ativo, onClick }: { funcao: Funcao; ativo: boolean; onClick: () => void }) {
  const t = useT()
  return <button type="button" className="nb-tile" aria-pressed={ativo} onClick={onClick}
    aria-label={funcao.propria ? t('botCreate.rostoDe', { nome: funcao.nome, funcao: t('botCreate.criadaPorVoce') }) : t('botCreate.rostoDe', { nome: funcao.nomeDoBot, funcao: rotuloDaFuncao(t, funcao) })}>
    <span className="nb-tile-rosto" aria-hidden="true"><DonnabotAvatar config={funcao.config} size={44} label="" animated={false} /></span>
    <span className="nb-tile-nome">{funcao.propria ? funcao.nome : funcao.nomeDoBot}</span>
    <span className="nb-tile-linha">{funcao.propria ? t('botCreate.criadaPorVoce') : rotuloDaFuncao(t, funcao)}</span>
  </button>
}
