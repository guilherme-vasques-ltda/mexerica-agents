// Fase 2b: gerador "Crie o seu" 3D. Toda execução é do Hub; aqui vão só intenções e estado.
// Sem chave no cofre, o fluxo não roda: o formulário próprio grava a chave no cofre do Hub
// (mesmo padrão da chave de contas — some do campo ao enviar, nunca volta).
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { assetIdValido, type MexeriquinhaConfig } from '@shared/fork-mexeriquinha-config'
import { useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { ATALHOS, briefingValido, cancelar, chaveErro, codigoDoErro, configGerada, estadoGerador, gerar, job, salvarChave, salvarConjunto, type Briefing, type JobGerador } from './gerador'
import { srcDoQuadroPelucia } from '../../../fork/avatars/mexeriquinhas-assets.ts'
import { QUADROS } from '../../../lib/mexeriquinhas/index.ts'
import variantes from '../../../lib/mexeriquinhas/prompts/variantes.json' with { type: 'json' }
import type { QuadroPelucia } from '../../../lib/mexeriquinhas-runtime.ts'

type Fase =
  | { k: 'carregando' }
  | { k: 'sem-cofre' }
  | { k: 'sem-processador' }
  | { k: 'sem-chave' }
  | { k: 'pronto' }
  | { k: 'gerando'; jobId: string; ultimo?: JobGerador }
  | { k: 'revisar'; jobId: string; assetId: string; nome: string }
  | { k: 'salvo'; config: MexeriquinhaConfig; nome: string }

const POLL_MS = 1500

export function GeradorMexeriquinha({ onPick, onClose, disabled }: {
  onPick: (config: MexeriquinhaConfig) => void
  onClose: () => void
  disabled?: boolean
}) {
  const t = useT()
  const store = useStore()
  const client = store.client
  const [fase, setFase] = useState<Fase>({ k: 'carregando' })
  const [erro, setErro] = useState('')
  const [chave, setChave] = useState('')
  const [briefing, setBriefing] = useState<Briefing>({ nome: '', descricao: '', atalhos: {} })
  const poll = useRef<ReturnType<typeof setTimeout> | null>(null)
  const geracao = useRef(0)

  useEffect(() => () => { if (poll.current) clearTimeout(poll.current) }, [])

  useEffect(() => {
    if (!client) { setFase({ k: 'sem-cofre' }); return }
    const era = ++geracao.current
    estadoGerador(client).then((e) => {
      if (geracao.current !== era) return
      setFase(!e.cofre ? { k: 'sem-cofre' } : !e.processador ? { k: 'sem-processador' } : e.gerador ? { k: 'pronto' } : { k: 'sem-chave' })
    }).catch(() => { if (geracao.current === era) setFase({ k: 'sem-cofre' }) })
  }, [client])

  const guardarChave = async (e?: FormEvent) => {
    e?.preventDefault()
    const valor = chave.trim()
    setChave('')
    if (!client || !valor) return
    setErro('')
    try {
      const estado = await salvarChave(client, valor)
      setFase(!estado.processador ? { k: 'sem-processador' } : estado.gerador ? { k: 'pronto' } : { k: 'sem-chave' })
      if (!estado.gerador) setErro(t('botRoles.mexeriquinha.gerador.chaveRecusada'))
    } catch (err) {
      setErro(t(chaveErro(codigoDoErro(err))))
    }
  }

  const acompanhar = (id: string) => {
    const era = geracao.current
    const tick = async () => {
      if (!client || geracao.current !== era) return
      try {
        const j = await job(client, id)
        if (geracao.current !== era) return
        if (j.status === 'ready' && j.conjunto && assetIdValido(j.conjunto.id)) {
          setFase({ k: 'revisar', jobId: id, assetId: j.conjunto.id, nome: j.conjunto.nome })
          return
        }
        if (j.status === 'error' || j.status === 'cancelled') {
          setErro(t(chaveErro(j.codigo ?? 'GERACAO_FALHOU')))
          setFase({ k: 'pronto' })
          return
        }
        setFase((f) => f.k === 'gerando' ? { ...f, ultimo: j } : f)
        poll.current = setTimeout(tick, POLL_MS)
      } catch {
        if (geracao.current === era) poll.current = setTimeout(tick, POLL_MS * 2)
      }
    }
    void tick()
  }

  const iniciar = async (e?: FormEvent) => {
    e?.preventDefault()
    if (!client || !briefingValido(briefing)) return
    setErro('')
    try {
      const { id } = await gerar(client, { ...briefing, nome: briefing.nome.trim(), descricao: briefing.descricao.trim() })
      setFase({ k: 'gerando', jobId: id })
      acompanhar(id)
    } catch (err) {
      setErro(t(chaveErro(codigoDoErro(err))))
    }
  }

  const salvar = async () => {
    if (!client || fase.k !== 'revisar') return
    setErro('')
    try {
      const { conjunto } = await salvarConjunto(client, fase.jobId)
      const config = configGerada(conjunto.id, 'base')
      if (!config) throw new Error('CONJUNTO_INDISPONIVEL')
      onPick(config)
    } catch (err) {
      setErro(t(chaveErro(codigoDoErro(err))))
    }
  }

  const interromper = () => {
    if (client && fase.k === 'gerando') void cancelar(client, fase.jobId).catch(() => {})
    setFase({ k: 'pronto' })
  }

  return <div className="mxq-gerador" role="group" aria-label={t('botRoles.mexeriquinha.gerador.titulo')}>
    <div className="mxq-seletor-barra">
      <span className="mxq-seletor-rotulo">{t('botRoles.mexeriquinha.gerador.titulo')}</span>
      <button type="button" className="mxq-seletor-chip" onClick={onClose}>{t('botRoles.mexeriquinha.gerador.fechar')}</button>
    </div>
    {fase.k === 'carregando' ? <p className="mxq-seletor-nota" role="status">{t('botRoles.mexeriquinha.gerador.carregando')}</p> : null}
    {fase.k === 'sem-cofre' ? <p className="mxq-seletor-nota" role="status">{t('botRoles.mexeriquinha.gerador.semCofre')}</p> : null}
    {fase.k === 'sem-processador' ? <p className="mxq-seletor-nota" role="status">{t('botRoles.mexeriquinha.gerador.semProcessador')}</p> : null}
    {fase.k === 'sem-chave' ? <form className="mxq-gerador-chave" onSubmit={guardarChave}>
      <p className="mxq-seletor-nota">{t('botRoles.mexeriquinha.gerador.semChave')}</p>
      <input className="field-box" type="password" autoComplete="off" spellCheck={false} value={chave}
        placeholder={t('botRoles.mexeriquinha.gerador.chavePlaceholder')} aria-label={t('botRoles.mexeriquinha.gerador.chavePlaceholder')}
        onChange={(e) => setChave(e.target.value)} />
      <button type="submit" className="mxq-seletor-chip" disabled={!chave.trim() || disabled}>{t('botRoles.mexeriquinha.gerador.guardarChave')}</button>
      <p className="mxq-seletor-nota">{t('botRoles.mexeriquinha.gerador.chaveNota')}</p>
    </form> : null}
    {fase.k === 'pronto' ? <form className="mxq-gerador-briefing" onSubmit={iniciar}>
      <input className="field-box" value={briefing.nome} maxLength={80} placeholder={t('botRoles.mexeriquinha.gerador.nome')}
        aria-label={t('botRoles.mexeriquinha.gerador.nome')} onChange={(e) => setBriefing({ ...briefing, nome: e.target.value })} />
      <textarea className="field-box" value={briefing.descricao} maxLength={2000} rows={3} placeholder={t('botRoles.mexeriquinha.gerador.descricao')}
        aria-label={t('botRoles.mexeriquinha.gerador.descricao')} onChange={(e) => setBriefing({ ...briefing, descricao: e.target.value })} />
      <div className="mxq-gerador-atalhos">
        {ATALHOS.map((campo) => <label key={campo} className="mxq-gerador-atalho">
          <span className="mxq-seletor-rotulo">{t(`botRoles.mexeriquinha.parte.${campo === 'cor' ? 'cor' : campo}`)}</span>
          <select className="field-box" value={briefing.atalhos[campo] ?? ''} onChange={(e) => setBriefing({
            ...briefing, atalhos: { ...briefing.atalhos, ...(e.target.value ? { [campo]: e.target.value } : {}) },
          })}>
            <option value="">{t('botRoles.mexeriquinha.gerador.atalho.nenhum')}</option>
            {Object.keys(variantes[campo] ?? {}).map((id) => <option key={id} value={id}>{id}</option>)}
          </select>
        </label>)}
      </div>
      <button type="submit" className="mxq-seletor-chip" disabled={!briefingValido(briefing) || disabled}>{t('botRoles.mexeriquinha.gerador.gerar')}</button>
      <p className="mxq-seletor-nota">{t('botRoles.mexeriquinha.gerador.custo')}</p>
    </form> : null}
    {fase.k === 'gerando' ? <div className="mxq-gerador-andamento" role="status">
      <p className="mxq-seletor-nota">{t('botRoles.mexeriquinha.gerador.gerando')}</p>
      <button type="button" className="mxq-seletor-chip" onClick={interromper}>{t('botRoles.mexeriquinha.gerador.cancelar')}</button>
    </div> : null}
    {fase.k === 'revisar' ? <div className="mxq-gerador-revisar">
      <p className="mxq-seletor-nota">{t('botRoles.mexeriquinha.gerador.revisar', { nome: fase.nome })}</p>
      <div className="mxq-gerador-grade">
        {QUADROS.map((q) => <img key={q} className="mxq-gerador-quadro" src={srcDoQuadroPelucia(fase.assetId, 96, q as QuadroPelucia)} alt={q} width={96} height={96} />)}
      </div>
      <div className="mxq-seletor-barra">
        <button type="button" className="mxq-seletor-chip" onClick={salvar} disabled={disabled}>{t('botRoles.mexeriquinha.gerador.salvar')}</button>
        <button type="button" className="mxq-seletor-chip" onClick={() => setFase({ k: 'pronto' })}>{t('botRoles.mexeriquinha.gerador.voltar')}</button>
      </div>
    </div> : null}
    {erro ? <p className="mxq-seletor-nota" role="alert">{erro}</p> : null}
  </div>
}
