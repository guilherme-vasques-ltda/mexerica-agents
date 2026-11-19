import { useMemo, useState } from 'react'
import type { TimesBot, TimesGraph } from '@shared/fork-times'
import { Button } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { configFromRole } from '../../../lib/donnabot-avatars/src'
import { linhasMarketing, MARKETING, REGRAS_MARKETING, type ExecucaoMarketing, type Linha, type PlanoMarketing } from './modelo-marketing'
import { areaPath, Note, TimesFace, useTimesTextos } from './parts'
import { frase, juntas, plural } from '../../../lib/fork-i18n/frase'
import { useT } from '../../../lib/fork-i18n/estado'
import { rotuloDoPapel } from '../../criar-bot/rotulos'

// Modelo Equipe Marketing (D353): lista quem já existe e quem nasce. Nada é criado nem conferido antes de Confirmar;
// Cancelar só fecha. Os bots novos nascem pelo createBot normal e a equipe vai como uma mudança comum (prévia).
// D454 (fatia 9c): no idioma da tela. As regras da equipe são o texto que vai às instruções dos bots criados e ficam
// em pt-BR (como os bots respondem hoje): mostradas aqui com `lang="pt-BR"`, até a decisão do dono.

export function TimesModeloSheet({ plano, graph, bots, execucao, executar, onDone, onCancel }: {
  plano: PlanoMarketing
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  /** O andamento vive na sessão: reabrir a janela durante a criação mostra "Criando…", sem outra execução (R1). */
  execucao: ExecucaoMarketing
  executar: (linhas: Linha[]) => Promise<{ ok: boolean }>
  onDone: () => void
  onCancel: () => void
}) {
  const { tx, nameOf } = useTimesTextos()
  const t = useT()
  // R1-02 do #594 (P135): o nome da função na tela vem do catálogo pelo id estável do preset; o preset e as regras que
  // vão às instruções do bot não mudam.
  const funcao = (role: { id: string; role: string }) => rotuloDoPapel(t, role.id) ?? role.role
  const [escolhas, setEscolhas] = useState<Record<string, string>>({})
  const { rodando, falha } = execucao
  const linhas = useMemo(() => linhasMarketing(plano, { ...execucao.escolhas, ...escolhas }), [plano, escolhas, execucao.escolhas])
  const novos = linhas.filter((l) => l.estado === 'criar').length
  const chefe = linhas[0]!
  const chefeNome = chefe.estado === 'existe' ? nameOf(bots, chefe.botId) : chefe.role.name

  const confirmar = async () => {
    if ((await executar(linhas)).ok) onDone()
  }

  return (
    <SheetPage className="times-sheet">
      <ModalHeader closeTitle={tx(frase('times.quadro.fechar'))} title={tx(frase('times.quadro.mkt.equipe', { area: MARKETING.area }))} subtitle={tx(frase('times.quadro.mkt.sub'))} />
      <fieldset className="times-fieldset">
        <legend>{tx(frase('times.quadro.mkt.quemEntra'))}</legend>
        {plano.vagas.map((v, i) => {
          const l = linhas[i]!
          return (
            <div key={v.role.id} className="times-modelo-row">
              {l.estado === 'existe' ? <TimesFace bot={bots.get(l.botId) ?? null} size={22} /> : <DonnabotAvatar config={configFromRole(v.role)} size={22} animated={false} label={v.role.name} />}
              <span className="times-modelo-text">
                <span><b><bdi dir="auto">{l.estado === 'existe' ? nameOf(bots, l.botId) : v.role.name}</bdi></b>{v.chefe ? ` · ${tx(frase('times.quadro.mkt.chefe'))}` : ''} <span className="times-choice-detail">{tx(v.chefe ? frase('times.quadro.mkt.gerenteDaArea', { funcao: funcao(v.role) }) : frase('times.quadro.mkt.reportaA', { funcao: funcao(v.role), nome: chefeNome }))}</span></span>
                {l.estado === 'criar' && v.fora.length ? <span className="times-choice-detail">{v.fora.map((f) => tx(f.subbots ? frase('times.quadro.mkt.temSubbots', { nome: f.bot.name }) : frase('times.quadro.mkt.estaEm', { nome: f.bot.name, lugar: f.teamId ? areaPath(graph, f.teamId) : frase('times.quadro.mkt.outroLugar') }))).join('; ')}</span> : null}
              </span>
              {v.candidatos.length > 1 ? (
                <select className="times-input times-modelo-pick" aria-label={tx(frase('times.quadro.mkt.botDe', { funcao: funcao(v.role) }))} value={l.estado === 'existe' ? l.botId : ''} disabled={rodando}
                  onChange={(e) => setEscolhas((s) => ({ ...s, [v.role.id]: e.target.value }))}>
                  {v.candidatos.map((b) => <option key={b.id} value={b.id} dir="auto">{b.name}</option>)}
                </select>
              ) : <span className="times-choice-detail">{tx(frase(l.estado === 'existe' ? 'times.quadro.mkt.jaExisteBot' : 'times.quadro.mkt.criarAgora'))}</span>}
            </div>
          )
        })}
      </fieldset>
      <fieldset className="times-fieldset">
        <legend>{tx(frase('times.quadro.mkt.regras'))}</legend>
        <ul className="times-modelo-rules" lang="pt-BR" dir="ltr">{REGRAS_MARKETING.map((r) => <li key={r}>{r}</li>)}</ul>
        <span className="times-choice-detail">{tx(novos ? plural('times.quadro.mkt.vaoNasInstrucoes', novos) : frase('times.quadro.mkt.naoMudam'))}</span>
      </fieldset>
      <Note>{tx(juntas(novos ? frase('times.quadro.mkt.botsNovos') : null, frase('times.quadro.mkt.viraGerente', { nome: chefeNome })))}</Note>
      {plano.bloqueio ? <p className="times-inline-error" role="alert">{tx(plano.bloqueio)}</p> : null}
      {falha ? (
        <p className="times-inline-error" role="alert">
          {tx(juntas(falha.erro, frase('times.quadro.mkt.nadaGravado'), falha.criados.length ? frase('times.quadro.mkt.jaCriados', { nomes: falha.criados.join(', ') }) : null))}
        </p>
      ) : null}
      <div className="times-sheet-foot">
        <Button kind="secondary" disabled={rodando} onClick={onCancel}>{tx(frase('times.quadro.mkt.cancelar'))}</Button>
        <Button disabled={rodando || !!plano.bloqueio} onClick={() => void confirmar()}>
          {tx(rodando ? frase('times.quadro.mkt.criando') : falha ? frase('times.quadro.tentar') : novos ? plural('times.quadro.mkt.criarEConferir', novos) : frase('times.quadro.mkt.conferirEquipe'))}
        </Button>
      </div>
    </SheetPage>
  )
}
