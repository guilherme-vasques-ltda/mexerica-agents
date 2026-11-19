import type { ReactNode } from 'react'
import type { Funcao } from '@shared/fork-bot-funcao'
import { Icon } from '../../components/Icon'
import { DonnabotAvatar } from '../../fork/avatars/Avatar'
import { useT } from '../../lib/fork-i18n/estado'
import { rotuloDaFuncao } from './rotulos'

const FRASES_IDS = ['qa', 'dev', 'research', 'escritor', 'social', 'seo', 'suporte'] as const
type Frase = (typeof FRASES_IDS)[number]
const FRASES = new Set<string>(FRASES_IDS)

/** Passo 1: os sete papéis comuns, "Ver todas as funções", onde ele trabalha, equipe e chefe, e o lugar das pastas. */
export function PassoPapel({ papeis, escolha, onde, equipe, onEscolher, onVerTodas }: {
  papeis: Funcao[]
  /** Fatia 3: a equipe e o chefe (`EscolhaDaEquipe`). */
  equipe: ReactNode
  /** Fatia 2: a escolha de Pessoal ou Space (`OndeTrabalha`). */
  onde: ReactNode
  escolha: string | null
  onEscolher: (funcao: Funcao) => void
  onVerTodas: () => void
}) {
  const t = useT()
  const comum = papeis.some((f) => f.id === escolha)
  return <>
    <span className="nb-rotulo">{t('botCreate.papelPrincipal')}</span>
    <div className="nb-papeis">
      {papeis.map((f) => {
        const id = f.id.replace(/^exemplo:/, '')
        return <button key={f.id} type="button" className="nb-papel" aria-pressed={escolha === f.id} onClick={() => onEscolher(f)}>
          <span className="nb-papel-rosto" aria-hidden="true"><DonnabotAvatar config={f.config} size={30} label="" animated={false} /></span>
          <span className="nb-papel-texto">
            <span className="nb-papel-nome">{rotuloDaFuncao(t, f)}</span>
            <span className="nb-papel-linha">{FRASES.has(id) ? t(`botCreate.papelFrase.${id as Frase}`) : null}</span>
          </span>
        </button>
      })}
      <button type="button" className="nb-papel nb-papel-todas" aria-pressed={!!escolha && !comum} onClick={onVerTodas}>
        <span className="nb-papel-nome">{t('botCreate.verTodas')}</span>
        <span className="nb-papel-linha">{t('botCreate.verTodasLinha')}</span>
      </button>
    </div>
    <p className="nb-nota">{t('botCreate.papelNota')}</p>

    <div className="nb-duas">
      <div className="nb-bloco">
        <span className="nb-rotulo">{t('botCreate.equipe')}</span>
        {equipe}
      </div>
      <div className="nb-bloco">
        <span className="nb-rotulo">{t('botCreate.onde')}</span>
        {onde}
      </div>
    </div>

    <span className="nb-rotulo">{t('botCreate.pastas')}</span>
    <div className="nb-caixa">
      <div className="nb-caixa-linha">
        <Icon name="doc.text" size={14} />
        <span className="nb-lista-texto">
          <span className="nb-lista-nome">{t('botCreate.pastasLinha')}</span>
          <span className="nb-lista-linha">{t('botCreate.pastasDetalhe')}</span>
        </span>
      </div>
    </div>
    <p className="nb-nota">{t('botCreate.pastasAinda')}</p>
  </>
}

/** Passo 3: Pedir (padrão), Liberar com confirmação e Acesso total desligado ("ainda não existe"). */
export function PassoPermissoes({ permissao, disabled, onPedir }: {
  permissao: string
  disabled: boolean
  onPedir: (permissao: 'ask' | 'auto') => void
}) {
  const t = useT()
  const opcoes = [
    { id: 'ask' as const, nome: t('botCreate.pedir'), linha: t('botCreate.pedirLinha'), extra: t('botCreate.recomendado') },
    { id: 'auto' as const, nome: t('botCreate.liberar'), linha: t('botCreate.liberarLinha'), extra: null },
  ]
  return <>
    <span className="nb-rotulo">{t('botCreate.quantoPergunta')}</span>
    <div role="radiogroup" aria-label={t('botCreate.quantoPergunta')} className="nb-modos">
      {opcoes.map((o) => <button key={o.id} type="button" role="radio" aria-checked={permissao === o.id} disabled={disabled}
        className="nb-modo" onClick={() => onPedir(o.id)}>
        <span className="nb-radio" aria-hidden="true" />
        <span className="nb-lista-texto">
          <span className="nb-lista-nome">{o.nome}{o.extra ? <span className="nb-selo">{o.extra}</span> : null}</span>
          <span className="nb-lista-linha">{o.linha}</span>
        </span>
      </button>)}
      <div className="nb-modo nb-modo-futuro" role="radio" aria-checked={false} aria-disabled="true">
        <span className="nb-radio" aria-hidden="true" />
        <span className="nb-lista-texto">
          <span className="nb-lista-nome">{t('botCreate.acessoTotal')}<span className="nb-depende">{t('botCreate.depende')}</span></span>
          <span className="nb-lista-linha">{t('botCreate.acessoTotalLinha')}</span>
        </span>
      </div>
    </div>
    <span className="nb-rotulo">{t('botCreate.valeSempre')}</span>
    <ul className="nb-regras">
      {(['recusar', 'semPedir', 'voz', 'hub'] as const).map((r) => <li key={r}><Icon name="checkmark" size={12} />{t(`botCreate.regras.${r}`)}</li>)}
    </ul>
  </>
}

/** Passo 4: tudo nasce desligado (D91/D166); ligar fica para os ajustes do bot até a fatia própria. */
export function PassoFerramentas() {
  const t = useT()
  const itens: [string, string][] = [
    ['puzzlepiece.extension', t('botCreate.ferramentasItens.skills')],
    ['point.3.connected.trianglepath.dotted', t('botCreate.ferramentasItens.conectores')],
    ['key.fill', t('botCreate.ferramentasItens.logins')],
    ['desktopcomputer', t('botCreate.ferramentasItens.computador')],
  ]
  return <>
    <div className="nb-caixa">
      {itens.map(([icone, nome]) => <div key={nome} className="nb-caixa-linha">
        <Icon name={icone} size={14} />
        <span className="nb-lista-nome nb-cresce">{nome}</span>
        <span className="nb-desligado">{t('botCreate.desligado')}</span>
      </div>)}
    </div>
    <p className="nb-nota">{t('botCreate.ferramentasLinha')}</p>
    <p className="nb-nota nb-aviso-calmo"><Icon name="lock.shield" size={13} />{t('botCreate.semSegredo')}</p>
  </>
}
