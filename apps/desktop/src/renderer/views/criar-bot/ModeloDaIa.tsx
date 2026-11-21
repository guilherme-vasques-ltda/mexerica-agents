import { useId, type ReactNode } from 'react'
import type { ManagedAccount } from '@shared/fork-engine-accounts'
import { modelosDaConta } from '@shared/fork-criar-bot-modelo'
import type { Backend } from '@shared/models'
import { useStore } from '../../store/context'
import { AvisoDoMotor } from '../fork/aviso-motor/AvisoDoMotor'
import { ageSemAprovacao, rotuloDeMotor, textosSemAprovacao } from '../fork/aviso-motor/aviso-motor'
import { carregarModelos, provedoresDoHub, useCatalogoMudou } from '../fork/seletor-modelo/catalogo'
import { separarPadrao, type Escolha } from '../fork/seletor-modelo/regras'
import { SeletorModelo } from '../fork/seletor-modelo/SeletorModelo'
import { idiomaAtual, useIdioma, useT, type Tradutor } from '../../lib/fork-i18n/estado'

/** Os motores do Hub como o Novo bot os mostra; a biblioteca (BN-03) reusa a mesma lista e os mesmos rótulos. */
export const opcoesDeMotor = (backends: Backend[]) => backends.map((b) => ({ id: b.id, label: rotuloDeMotor(b, idiomaAtual()) }))

const estadoDoMotor = (t: Tradutor, b: Backend) => (!b.available ? t('botCreate.naoInstalado') : b.signedIn === false ? t('botCreate.faltaEntrar') : t('botCreate.pronto'))
const sigla = (nome: string) => nome.replace(/[^\p{L}\p{N}]/gu, '').slice(0, 2)

/**
 * Passo 2: os motores do Hub (`hello.backends`), a conta de IA (fatia 2) e o modelo pelo seletor único da Conversa
 * (C9, D373) no modo `padraoDoBot`: os mesmos favoritos e atalhos ⌘1–⌘3 do compositor, "Padrão do motor" dentro dele
 * e o contexto do catálogo real. O seletor só devolve a escolha; o Novo bot a aplica ao rascunho e o `createBot`
 * grava. Com conta, a aba do motor lista só os modelos que a conta comprovou, como no editor (o Hub recusa outro).
 */
export function ModeloDaIa({ backends, valor, disabled, conta, contaSlot, onBackend, onEscolha }: {
  backends: Backend[]
  /** O padrão do bot no rascunho: motor ativo, conta, modelo (`null` = padrão), opções e velocidade. */
  valor: Escolha
  disabled: boolean
  /** A conta escolhida; `null` = padrão do Hub. */
  conta: ManagedAccount | null
  contaSlot: ReactNode
  onBackend: (backend: string) => void
  onEscolha: (escolha: Escolha) => void
}) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const semAprovacaoId = useId()
  useCatalogoMudou()
  const backend = valor.backend
  // Com conta, a aba do motor lista os modelos da conta, sem o "Default" (passa por `separarPadrao`, como o catálogo da C9).
  const provedores = provedoresDoHub(store).map((p) => (conta && p.id === backend ? { ...p, ...separarPadrao(modelosDaConta(conta.models, p.models)) } : p))
  const noSeletor = provedores.some((p) => p.id === backend)

  return <div className="nb-motor">
    <div className="nb-motores">
      <span className="nb-rotulo">{t('botCreate.motoresDoHub')}</span>
      <div role="radiogroup" aria-label={t('botCreate.motoresDoHub')} className="nb-motores-lista">
        {backends.map((b) => {
          const semAprovacao = ageSemAprovacao(b)
          const descricaoId = `${semAprovacaoId}-${b.id}`
          return <button key={b.id} type="button" role="radio" aria-checked={b.id === backend} disabled={disabled}
            aria-describedby={semAprovacao ? descricaoId : undefined}
            className="nb-motor-item" onClick={() => onBackend(b.id)}>
            <span className="nb-motor-sigla" aria-hidden="true">{sigla(b.name)}</span>
            <span className="nb-motor-texto">
              <span className="nb-motor-nome">{b.name}</span>
              <span className={`nb-motor-estado ${b.available && b.signedIn !== false ? 'nb-ok' : 'nb-falta'}`}>{estadoDoMotor(t, b)}</span>
              {semAprovacao ? <span className="nb-motor-estado nb-falta" title={textosSemAprovacao(idioma).texto}>{textosSemAprovacao(idioma).selo}</span> : null}
            </span>
            {semAprovacao ? <span id={descricaoId} hidden>{textosSemAprovacao(idioma).texto}</span> : null}
          </button>
        })}
      </div>
    </div>
    <div className="nb-modelos">
      <AvisoDoMotor backend={backend} semAprovacao={ageSemAprovacao(backends.find((b) => b.id === backend))} />
      <span className="nb-rotulo">{t('botCreate.contaDeIa')}</span>
      {contaSlot}
      <span className="nb-rotulo">{t('botCreate.modelo')}</span>
      {conta && !conta.models.length ? <p className="nb-nota">{t('botCreate.contaSemModelos')}</p> : null}
      {noSeletor ? <fieldset className="nb-seletor" disabled={disabled}>
        <SeletorModelo modo="padraoDoBot" provedores={provedores} valor={valor} carregar={(b) => carregarModelos(store, b)} onEscolher={onEscolha} />
      </fieldset> : <p className="nb-nota nb-aviso">{t('botCreate.motor.motorSemSeletor')}</p>}
      <p className="nb-nota">{t('botCreate.motor.modeloNota')}</p>
    </div>
  </div>
}
