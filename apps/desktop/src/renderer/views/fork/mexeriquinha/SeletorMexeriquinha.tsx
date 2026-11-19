// Fase 2a das Mexeriquinhas: escolha da aparência do bot em dois estilos (Simples e Pelúcia 3D).
// Só muda a aparência (avatarConfig legado + mexeriquinha v2); nome, função e instruções do bot não passam por aqui. Sem gerador nem chave (fase 2b).
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import type { MexeriquinhaConfig } from '@shared/fork-avatar-config'
import { ACESSORIOS, CATEGORIAS, CORES, ESTADOS, MODELOS, OBJETOS, OLHOS, ROUPAS, conjuntoDoModelo, modeloPorId,
  type CategoriaId, type EstadoId, type Opcao } from '../../../lib/mexeriquinhas/index.ts'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { useT } from '../../../lib/fork-i18n/estado'
import { comParte, configCriada, configDoPronto, gruposVisiveis, prontoMarcado, simplesDe, type Estilo, type Parte } from './seletor'
import { GeradorMexeriquinha } from './Gerador.tsx'
import './mexeriquinha.css'

const PARTES: { parte: Parte; opcoes: readonly Opcao<string>[] }[] = [
  { parte: 'olhos', opcoes: OLHOS },
  { parte: 'cor', opcoes: CORES },
  { parte: 'roupa', opcoes: ROUPAS },
  { parte: 'acessorio', opcoes: ACESSORIOS },
  { parte: 'objeto', opcoes: OBJETOS },
]
const ESTILOS: Estilo[] = ['simples', '3d']
/** Quantas aparências 3D os nomes prontos dividem nesta fase (texto honesto na galeria). */
const APARENCIAS_3D = new Set(MODELOS.map((m) => conjuntoDoModelo(m).id)).size

/** Monta o conteúdo só quando chega perto da área visível; depois fica montado (sem piscar ao rolar). */
function Visivel({ size, children }: { size: number; children: ReactNode }) {
  const ref = useRef<HTMLSpanElement>(null)
  const [visto, setVisto] = useState(false)
  useEffect(() => {
    const el = ref.current
    if (!el || visto) return
    if (typeof IntersectionObserver === 'undefined') { setVisto(true); return }
    const io = new IntersectionObserver((entradas) => { if (entradas.some((e) => e.isIntersecting)) setVisto(true) }, { rootMargin: '160px' })
    io.observe(el)
    return () => io.disconnect()
  }, [visto])
  return <span ref={ref} className="mxq-seletor-visivel" style={{ width: size, height: size }}>{visto ? children : null}</span>
}

/** Nome pronto ou "Criado por você" para a aparência em uso. */
function nomeEmUso(value: MexeriquinhaConfig, criado: string): string {
  return ('modeloId' in value && value.modeloId && modeloPorId(value.modeloId)?.nome) || criado
}

export function SeletorMexeriquinha({ value, onChange, disabled = false, previa = false }: {
  value: MexeriquinhaConfig
  onChange: (config: MexeriquinhaConfig) => void
  disabled?: boolean
  /** Palco com os cinco estados; desligado onde a tela já tem a própria prévia (Ajustes, Novo bot). */
  previa?: boolean
}) {
  const t = useT()
  const id = useId()
  const [estilo, setEstilo] = useState<Estilo>(value.estilo)
  const [busca, setBusca] = useState('')
  const [categoria, setCategoria] = useState<CategoriaId | ''>('')
  const [pecasAbertas, setPecasAbertas] = useState(value.estilo === 'simples' && !('modeloId' in value && value.modeloId))
  const [geradorAberto, setGeradorAberto] = useState(false)
  const pecas = useRef<HTMLDetailsElement>(null)
  const grupos = gruposVisiveis(busca, categoria)
  const marcado = prontoMarcado(value, estilo)
  const criado = t('botRoles.mexeriquinha.criado')

  const escolherEstilo = (novo: Estilo) => setEstilo(novo)
  const setas = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return
    e.preventDefault()
    const outro = ESTILOS.find((s) => s !== estilo)!
    escolherEstilo(outro)
    e.currentTarget.parentElement?.querySelector<HTMLButtonElement>(`[data-estilo="${outro}"]`)?.focus()
  }
  const crie = () => {
    onChange(configCriada())
    setPecasAbertas(true)
    requestAnimationFrame(() => pecas.current?.querySelector('summary')?.focus())
  }
  const simples = simplesDe(value)
  const amostra = (s: Estilo) => configDoPronto(modeloPorId(('modeloId' in value && value.modeloId) || '') ?? modeloPorId('berga')!, s)

  return <div className="mxq-seletor" aria-disabled={disabled || undefined}>
    {previa ? <Palco value={value} /> : null}
    <div className="mxq-seletor-estilos" role="radiogroup" aria-label={t('botRoles.mexeriquinha.estilo')}>
      {ESTILOS.map((s) => <button key={s} type="button" role="radio" data-estilo={s} aria-checked={estilo === s} tabIndex={estilo === s ? 0 : -1}
        className="mxq-seletor-estilo" disabled={disabled} onClick={() => escolherEstilo(s)} onKeyDown={setas}>
        <DonnabotAvatar config={amostra(s)} size={40} label="" animated={false} />
        <span className="mxq-seletor-estilo-texto">
          <b>{t(s === 'simples' ? 'botRoles.mexeriquinha.simples' : 'botRoles.mexeriquinha.pelucia')}</b>
          <span>{t(s === 'simples' ? 'botRoles.mexeriquinha.simplesLinha' : 'botRoles.mexeriquinha.peluciaLinha')}</span>
        </span>
      </button>)}
    </div>
    <p className="mxq-seletor-nota" role="status">
      {t('botRoles.mexeriquinha.emUso', { nome: `${nomeEmUso(value, criado)} · ${t(value.estilo === 'simples' ? 'botRoles.mexeriquinha.simples' : 'botRoles.mexeriquinha.pelucia')}` })}
      {' '}{t('botRoles.mexeriquinha.soAparencia')}
    </p>

    {estilo === 'simples' ? <details ref={pecas} className="mxq-seletor-pecas" open={pecasAbertas} onToggle={(e) => setPecasAbertas(e.currentTarget.open)}>
      <summary>{t('botRoles.mexeriquinha.pecas')}</summary>
      {pecasAbertas ? PARTES.map(({ parte, opcoes }) => <div key={parte} className="mxq-seletor-parte" role="group" aria-label={t(`botRoles.mexeriquinha.parte.${parte}`)}>
        <span className="mxq-seletor-rotulo">{t(`botRoles.mexeriquinha.parte.${parte}`)}</span>
        <div className="mxq-seletor-opcoes">
          {opcoes.map((o) => {
            const config = comParte(simples, parte, o.id)
            const ativo = value.estilo === 'simples' && value.aparencia[parte] === o.id
            return <button key={o.id} type="button" className="mxq-seletor-opcao" aria-pressed={ativo} disabled={disabled}
              data-parte={parte} data-opcao={o.id} onClick={() => onChange(config)}>
              <DonnabotAvatar config={config} size={44} label="" animated={false} />
              <span>{o.nome}</span>
            </button>
          })}
        </div>
      </div>) : null}
    </details> : null}

    <div className="mxq-seletor-barra">
      <span className="mxq-seletor-rotulo" id={`${id}-prontos`}>{t('botRoles.mexeriquinha.prontos')}</span>
      <input type="search" className="field-box mxq-seletor-busca" value={busca} placeholder={t('botRoles.mexeriquinha.buscar')}
        aria-label={t('botRoles.mexeriquinha.buscar')} autoComplete="off" spellCheck={false} onChange={(e) => setBusca(e.target.value)} />
      <select className="field-box mxq-seletor-categoria" value={categoria} aria-label={t('botRoles.mexeriquinha.categoria')}
        onChange={(e) => setCategoria(e.target.value as CategoriaId | '')}>
        <option value="">{t('botRoles.mexeriquinha.todas', { total: MODELOS.length })}</option>
        {CATEGORIAS.map((c) => <option key={c.id} value={c.id}>{c.nome}</option>)}
      </select>
    </div>
    {estilo === '3d' ? <p className="mxq-seletor-nota">{t('botRoles.mexeriquinha.compartilhadas', { total: APARENCIAS_3D })}</p> : null}

    {estilo === '3d' && geradorAberto ? <GeradorMexeriquinha disabled={disabled} onClose={() => setGeradorAberto(false)}
      onPick={(config) => { onChange(config); setGeradorAberto(false) }} /> : null}
    <div className="mxq-seletor-galeria" role="group" aria-labelledby={`${id}-prontos`}>
      {estilo === '3d' && !busca && !categoria ? <button type="button" className="mxq-seletor-tile mxq-seletor-crie" disabled={disabled}
        aria-pressed={value.v === 3} onClick={() => setGeradorAberto(true)}>
        <span className="mxq-seletor-visivel" style={{ width: 56, height: 56 }}><DonnabotAvatar config={configCriada()} size={56} label="" animated={false} /></span>
        <span className="mxq-seletor-nome">{t('botRoles.mexeriquinha.crie')}</span>
        <span className="mxq-seletor-linha">{t('botRoles.mexeriquinha.crieLinha3d')}</span>
      </button> : null}
      {estilo === 'simples' && !busca && !categoria ? <button type="button" className="mxq-seletor-tile mxq-seletor-crie" disabled={disabled}
        aria-pressed={value.estilo === 'simples' && !('modeloId' in value && value.modeloId)} onClick={crie}>
        <span className="mxq-seletor-visivel" style={{ width: 56, height: 56 }}><DonnabotAvatar config={configCriada()} size={56} label="" animated={false} /></span>
        <span className="mxq-seletor-nome">{t('botRoles.mexeriquinha.crie')}</span>
        <span className="mxq-seletor-linha">{t('botRoles.mexeriquinha.crieLinha')}</span>
      </button> : null}
      {grupos.length ? grupos.map((g) => <section key={g.id} className="mxq-seletor-categoria-bloco" aria-label={g.nome}>
        <h4 className="mxq-seletor-cab">{g.nome}<span className="mxq-seletor-conta">{g.modelos.length}</span></h4>
        <div className="mxq-seletor-grade">
          {g.modelos.map((m) => <button key={m.id} type="button" className="mxq-seletor-tile" data-modelo={m.id} aria-pressed={marcado === m.id}
            aria-label={t('botRoles.mexeriquinha.usar', { nome: m.nome })} disabled={disabled} onClick={() => onChange(configDoPronto(m, estilo))}>
            <Visivel size={56}><DonnabotAvatar config={configDoPronto(m, estilo)} size={56} label="" animated={false} /></Visivel>
            <span className="mxq-seletor-nome" aria-hidden="true">{m.nome}</span>
          </button>)}
        </div>
      </section>) : <p className="mxq-seletor-nota">{t('botRoles.mexeriquinha.nenhum')}</p>}
    </div>
  </div>
}

/** Palco: a aparência em uso nos cinco estados (movimento só aqui; a galeria é parada). */
function Palco({ value }: { value: MexeriquinhaConfig }) {
  const t = useT()
  const [estado, setEstado] = useState<EstadoId>('livre')
  return <div className="mxq-seletor-palco">
    <DonnabotAvatar config={value} size={128} estado={estado} label={ESTADOS.find((e) => e.id === estado)!.nome} />
    <div className="mxq-seletor-estados" role="group" aria-label={t('botRoles.mexeriquinha.estados')}>
      {ESTADOS.map((e) => <button key={e.id} type="button" className="mxq-seletor-chip" aria-pressed={estado === e.id} onClick={() => setEstado(e.id)}>{e.nome}</button>)}
    </div>
  </div>
}
