// Ajustes → Instruções: «Para todos os meus bots» e «Neste Space», somados às instruções de cada bot. Só o Hub do
// computador escolhido; o Hub decide quem é o dono e recusa os outros. Contrato: docs/features/manual-do-harness.md.
import { useEffect, useId } from 'react'
import { Button, CardForm, CardSection, ChoicePicker } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { formatarNumero } from '../../../lib/fork-i18n/index'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import { computadorDaPagina, spaceDaPagina } from '../nunca-fazer/modelo'
import { CampoInstrucao, contar, LIMITE, type Escopo, type InstrucoesLink } from './campo'
import './instrucoes.css'

/** `guardarComo`: a chave do rascunho em Ajustes; o modelo guarda o texto editado e a página volta com ele (estado-pagina.ts). */
function Campo({ link, escopo, guardarComo }: { link: InstrucoesLink; escopo: Escopo; guardarComo: string }) {
  const [modelo] = useEstadoDaPagina(`instrucoes/${guardarComo}`, () => new CampoInstrucao(link, escopo))
  const campo = useModel(modelo)
  const id = useId()
  const t = useT()
  const idioma = useIdioma()
  const chave = escopo.tipo === 'space' ? 'space' : 'geral'
  // Com rascunho sujo do modelo que já leu, a releitura do Hub não apaga o texto digitado.
  useEffect(() => { void modelo.ler(modelo.revisao > 0 && modelo.sujo) }, [modelo])
  const alerta = campo.fase === 'conflito' || campo.fase === 'falha' || campo.excede
  return (
    <div className="instrucoes-campo">
      <label htmlFor={id} className="instrucoes-titulo">{t(`instrucoes.${chave}.titulo`)}</label>
      <p className="instrucoes-ajuda">{t(`instrucoes.${chave}.ajuda`)}</p>
      <textarea id={id} className="instrucoes-texto" dir="auto" value={campo.texto} placeholder={t(`instrucoes.${chave}.exemplo`)} rows={6}
        disabled={campo.fase === 'lendo'} onChange={(e) => modelo.editar(e.target.value)} aria-describedby={`${id}-estado`} />
      <div className="instrucoes-rodape">
        <span id={`${id}-estado`} className={alerta ? 'instrucoes-alerta' : undefined} role={alerta ? 'alert' : undefined}>{t(campo.estado)}</span>
        <span className={`instrucoes-contador${campo.excede ? ' instrucoes-alerta' : ''}`}>
          {t('instrucoes.contador', { n: formatarNumero(idioma, contar(campo.texto)), max: formatarNumero(idioma, LIMITE) })}
        </span>
      </div>
      <div className="instrucoes-acoes">
        {campo.podeRepetir ? <Button kind="secondary" onClick={() => void modelo.tentarDeNovo()}>{t('instrucoes.acao.tentarDeNovo')}</Button> : null}
        <Button disabled={!campo.sujo || campo.excede || campo.fase !== 'pronto'} onClick={() => void modelo.salvar()}>{t('instrucoes.acao.salvar')}</Button>
      </div>
    </div>
  )
}

function PaginaDoHub({ store: fonte }: { store: BotStore }) {
  const t = useT()
  const app = useApp()
  const store = useModel(fonte)
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  const spaceId = spaceDaPagina(app.spaceMirrors.active, store.computer.id, store.hello?.hostId, store.spaces?.personalSpaceId)
  const nome = store.spaces?.spaces.find((s) => s.id === spaceId)?.name ?? ''
  if (!client) return <Text tone="secondary">{t('instrucoes.semConexao')}</Text>
  const hub = `${store.computer.id}|${store.hello?.hostId ?? ''}|${store.principalEpoch}`
  return <>
    <CardSection>
      <Campo key={`${hub}|geral`} guardarComo={`${hub}|geral`} link={client} escopo={{ tipo: 'geral' }} />
    </CardSection>
    {spaceId ? <CardSection title={nome || null}>
      <Campo key={`${hub}|${spaceId}`} guardarComo={`${hub}|${spaceId}`} link={client} escopo={{ tipo: 'space', spaceId }} />
    </CardSection> : null}
  </>
}

export function InstrucoesPage() {
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const ativo = app.spaceMirrors.active
  const [escolhido, setComputador] = useEstadoDaPagina('instrucoes/computador', () => app.selectedStore?.computer.id ?? (app.computers.length === 1 ? app.computers[0]!.id : ''))
  // Com Space ativo o computador é o dele; só com o Pessoal aberto a pessoa escolhe o computador (como em Nunca fazer).
  const computador = computadorDaPagina(ativo, escolhido)
  const store = computador ? app.spaceMirrors.root(computador) : null
  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)}>
      <CardForm>
        <Text tone="secondary">{t('instrucoes.pagina.lead')}</Text>
        {!ativo && app.computers.length > 1 ? <label className="instrucoes-escolha"><span>{t('memoria.computador')}</span>
          <ChoicePicker language={superficieIdioma(idioma)} selection={computador} options={[{ id: '', label: t('memoria.computador.escolha') }, ...app.computers.map((c) => ({ id: c.id, label: c.name }))]} onChange={setComputador} /></label> : null}
        {store ? <PaginaDoHub key={`${computador}/${ativo?.spaceId ?? ''}`} store={store} /> : <Text tone="secondary">{t('memoria.computador.escolha')}</Text>}
        <div className="instrucoes-ordem"><b>{t('instrucoes.ordem.titulo')}</b> {t('instrucoes.ordem.texto')}</div>
      </CardForm>
    </div>
  )
}
