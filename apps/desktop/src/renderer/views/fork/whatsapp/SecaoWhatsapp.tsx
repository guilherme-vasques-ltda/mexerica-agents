import { useEffect, useState, type ReactNode } from 'react'
import { Button, CardSection, ChoicePicker, Pill, SegmentedChoice, Slider, Switch, ValueRow } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { comNos, no, semParametros, Superficie } from '../../settings/rotulo'
import { SwitchRow } from '../../bots/parts'
import { settingsScreen } from '../ajustes/tela'
import { extensoesScreen } from '../extensoes/tela'
import { AvisoNaoOficial, ErroWhatsapp, MarcaW, QuatroEstados, TextoDaMensagem, useWhatsapp } from './partes'
import { ConectarNumero, DesconectarNumero, EstadoDoNumero } from './Numero'
import { caixaDe } from './caixa-estado'
import { NomeConversa } from './contato'
import { NumeroBot } from './NumeroBot'
import { horaMensagem, type ConversaWa } from './caixa'
import { decisoesDoItem, motivoFila, resumoFila, estadoFila, type ItemFila } from './fila'
import { AVISO_14_DIAS, computadorPessoal, ESPERA, LIMITE, LIMITE_DIA, limiteDiaDe, horaDeMinutos, minutosDeHora, PRAZOS, bloqueioDoBot, clamp, erroDoHub, patchVer, podeResponder, quatroEstados, silencioDe, transcricaoTexto, type AjustesBot, type BotWhatsapp, type PatchBot } from './regras'
import type { Mensagem } from '../bot-settings/mensagem'

// whatsapp R5/MX-18: a seção "WhatsApp" do editor do bot. Dois modos de resposta ("peço antes" com cartão
// ou "sozinho" dentro dos limites, só em conversa marcada), limites por hora e por dia, horário de
// silêncio e a fila de saída com os itens incertos para o dono decidir.

/** R1-02 do #587: todos os retornos (carregando, erro e a seção) ficam na superfície do idioma do app. */
export function SecaoWhatsapp(props: { store: BotStore; botId: string; nome: string; disabled: boolean }) {
  return <Superficie><ConteudoWhatsapp {...props} /></Superficie>
}

function ConteudoWhatsapp({ store, botId, nome, disabled }: { store: BotStore; botId: string; nome: string; disabled: boolean }) {
  const estado = useWhatsapp(store)
  const t = useT()
  const { leitura } = estado
  // MX-18: a fila de saída deste bot e a caixa (nomes das marcadas e do destino na fila),
  // relidas a cada aviso do Hub.
  useModel(estado)
  const caixa = caixaDe(store, () => store.connectedClient)
  useModel(caixa)
  useEffect(() => { void estado.lerFila(botId) }, [estado, botId, store.whatsappEpoch, leitura])
  useEffect(() => { void caixa.lerConversas(botId) }, [caixa, botId, store.whatsappEpoch])
  if (!leitura) {
    return estado.erroLeitura
      ? <ErroWhatsapp mensagem={estado.erroLeitura} tentar={() => void estado.ler()} />
      : <p className="wa-carregando" role="status">{t('whatsapp.ajustes.carregando')}</p>
  }
  const bot = leitura.bots.find((b) => b.botId === botId)
  if (!bot) return <ErroWhatsapp mensagem={{ chave: 'whatsapp.ajustes.semBot' }} tentar={() => void estado.ler()} />
  const chaveBloqueio = bloqueioDoBot(leitura, bot)
  const bloqueio = chaveBloqueio ? semParametros(t)(chaveBloqueio) : null
  const travado = disabled || estado.salvando || bloqueio !== null
  const salvar = (patch: PatchBot) => void estado.salvarBot(botId, patch)
  return (
    <div className="wa-secao">
      <div className="wa-numero">
        <MarcaW />
        <div className="wa-plugin-texto">
          <EstadoDoNumero numero={bot.number} nome={nome} computador={store.hostName || undefined} pessoal={computadorPessoal(store.hello?.os)} />
          {bot.number.state === 'none' ? <span className="wa-nota">{comNos(t('whatsapp.ajustes.conecteUm', { bot: no('bot') }), { bot: <bdi dir="auto">{nome}</bdi> })}</span> : null}
        </div>
        <span className="wa-bot-acoes">
          <ConectarNumero store={store} estado={estado} leitura={leitura} bot={bot} nome={nome} disabled={disabled} />
        </span>
      </div>
      <p className="wa-nota wa-14"><Icon name="clock" size={12} />{t(AVISO_14_DIAS)}</p>
      <QuatroEstados estados={quatroEstados(leitura, bot)} nome={nome} />
      <AvisoNaoOficial />
      {bloqueio ? (
        <div className="wa-bloqueio" role="note">
          <span>{bloqueio}</span>
          {!leitura.plugin.enabled || bot.paused ? <button className="wa-link" onClick={() => extensoesScreen.show('plugins', store.computer.id, 'whatsapp')}>{t('whatsapp.ajustes.abrirPlugins')}</button> : null}
        </div>
      ) : null}
      {estado.erroPedido ? <ErroWhatsapp mensagem={estado.erroPedido} /> : null}
      <fieldset disabled={travado} className={`wa-sem-borda wa-opcoes${bloqueio ? ' esmaecido' : ''}`}>
        <Opcoes s={bot.settings} nome={nome} salvar={salvar} />
        {bot.settings.replyMode === 'autonomous' ? <Marcadas caixa={caixa} bot={bot} nome={nome} travado={travado} /> : null}
        <CardSection title={t('whatsapp.ajustes.voz')}>
          <SwitchRow title={t('whatsapp.voz.transcrever')} detail={t('whatsapp.voz.transcreverDetalhe')} on={bot.settings.transcribeAudio} onChange={(on) => salvar({ transcribeAudio: on })} />
          <ValueRow label={t('whatsapp.ajustes.modelo')} detail={transcricaoTexto(t, leitura.transcription)}>
            <button type="button" className="wa-link" onClick={() => settingsScreen.show('voice', 'ouvir')}>{t('whatsapp.ajustes.vozParaTexto')} <Icon name="chevron.right" size={10} /></button>
          </ValueRow>
        </CardSection>
        <Prazo dias={bot.settings.retentionDays ?? 30} salvar={(d) => salvar({ retentionDays: d })} />
      </fieldset>
      <FilaEnvio estado={estado} botId={botId} conversas={caixa.lista(botId)?.conversas ?? []} travado={travado} />
      <DesconectarNumero estado={estado} bot={bot} nome={nome} disabled={disabled} />
    </div>
  )
}

function Opcoes({ s, nome, salvar }: { s: AjustesBot; nome: string; salvar: (patch: PatchBot) => void }) {
  const t = useT()
  const sozinho = s.replyMode === 'autonomous'
  // Os `detail` são texto puro: o nome entra como parâmetro (isolado em RTL pelo catálogo).
  return (
    <>
      <CardSection title={t('whatsapp.ajustes.conversas')}>
        <SwitchRow title={t('whatsapp.estados.opcao.verConversas')} detail={t('whatsapp.ajustes.verConversasDetalhe', { bot: nome })} on={s.readChats} onChange={(on) => salvar(patchVer('chats', on))} />
        <SwitchRow title={t('whatsapp.estados.opcao.responderConversas')} detail={podeResponder(s, 'chats') ? t('whatsapp.ajustes.responderConversasDetalhe') : t('whatsapp.ajustes.ligueVerConversas', { bot: nome })}
          on={s.replyChats} disabled={!podeResponder(s, 'chats')} onChange={(on) => salvar({ replyChats: on })} />
      </CardSection>
      <CardSection title={t('whatsapp.ajustes.grupos')}>
        <SwitchRow title={t('whatsapp.estados.opcao.verGrupos')} detail={t('whatsapp.ajustes.verGruposDetalhe', { bot: nome })} on={s.readGroups} onChange={(on) => salvar(patchVer('groups', on))} />
        <SwitchRow title={t('whatsapp.estados.opcao.responderGrupos')} detail={podeResponder(s, 'groups') ? t('whatsapp.ajustes.responderGruposDetalhe', { bot: nome }) : t('whatsapp.ajustes.ligueVerGrupos', { bot: nome })}
          on={s.replyGroups} disabled={!podeResponder(s, 'groups')} onChange={(on) => salvar({ replyGroups: on })} />
      </CardSection>
      <CardSection title={t('whatsapp.ajustes.comoResponde')} footer={<><b>{t('whatsapp.ajustes.nuncaOrdens')}</b> {comNos(t('whatsapp.ajustes.quemManda', { bot: no('bot') }), { bot: <bdi dir="auto">{nome}</bdi> })}</>}>
        <ValueRow label={t('whatsapp.ajustes.respostas')} detail={t(sozinho ? 'whatsapp.ajustes.respostasDetalheSozinho' : 'whatsapp.ajustes.respostasDetalhe')}>
          <SegmentedChoice
            selection={s.replyMode}
            options={[
              { id: 'approval' as const, label: t('whatsapp.ajustes.aguardaAprovacao') },
              { id: 'autonomous' as const, label: t('whatsapp.ajustes.respondeSozinho') },
            ]}
            onChange={(modo) => salvar({ replyMode: modo })}
          />
        </ValueRow>
      </CardSection>
      <CardSection title={t('whatsapp.ajustes.cuidados')}>
        <Espera valor={s.delaySeconds} salvar={(v) => salvar({ delaySeconds: v })} />
        <ValueRow label={t('whatsapp.ajustes.limite')} detail={t('whatsapp.ajustes.limiteDetalhe', { max: LIMITE.max, bot: nome })}>
          <span className="wa-passo">
            <button type="button" aria-label={t('whatsapp.ajustes.menos')} disabled={s.hourlyLimit <= LIMITE.min} onClick={() => salvar({ hourlyLimit: clamp(s.hourlyLimit - 1, LIMITE) })}>−</button>
            <b aria-live="polite">{t('whatsapp.ajustes.porHora', { n: s.hourlyLimit })}</b>
            <button type="button" aria-label={t('whatsapp.ajustes.mais')} disabled={s.hourlyLimit >= LIMITE.max} onClick={() => salvar({ hourlyLimit: clamp(s.hourlyLimit + 1, LIMITE) })}>+</button>
          </span>
        </ValueRow>
        <ValueRow label={t('whatsapp.ajustes.limiteDia')} detail={t('whatsapp.ajustes.limiteDiaDetalhe', { max: LIMITE_DIA.max })}>
          <span className="wa-passo">
            <button type="button" aria-label={t('whatsapp.ajustes.menos')} disabled={limiteDiaDe(s) <= LIMITE_DIA.min} onClick={() => salvar({ dailyLimit: clamp(limiteDiaDe(s) - 1, LIMITE_DIA) })}>−</button>
            <b aria-live="polite">{t('whatsapp.ajustes.porDia', { n: limiteDiaDe(s) })}</b>
            <button type="button" aria-label={t('whatsapp.ajustes.mais')} disabled={limiteDiaDe(s) >= LIMITE_DIA.max} onClick={() => salvar({ dailyLimit: clamp(limiteDiaDe(s) + 1, LIMITE_DIA) })}>+</button>
          </span>
        </ValueRow>
        <Silencio s={s} salvar={salvar} />
        <ValueRow label={t('whatsapp.ajustes.nuncaComecar')} detail={t('whatsapp.ajustes.nuncaComecarDetalhe', { bot: nome })}><Pill outlined><Icon name="lock.shield" size={11} /> {t('whatsapp.ajustes.sempre')}</Pill></ValueRow>
        <ValueRow label={t('whatsapp.ajustes.semListas')} detail={t('whatsapp.ajustes.semListasDetalhe')}><Pill outlined><Icon name="lock.shield" size={11} /> {t('whatsapp.ajustes.sempre')}</Pill></ValueRow>
      </CardSection>
    </>
  )
}

/** MX-18: o horário de silêncio — segurado até a abertura; o que o dono escreve sai na hora. */
function Silencio({ s, salvar }: { s: AjustesBot; salvar: (patch: PatchBot) => void }) {
  const t = useT()
  const q = silencioDe(s)
  return (
    <ValueRow label={t('whatsapp.ajustes.silencio')} detail={t('whatsapp.ajustes.silencioDetalhe')}>
      <span className="wa-silencio">
        <Switch on={q.on} label={t('whatsapp.ajustes.silencio')} onChange={(on) => salvar({ quietEnabled: on })} />
        <input type="time" dir="ltr" aria-label={t('whatsapp.ajustes.silencioInicio')} disabled={!q.on}
          value={horaDeMinutos(q.start)} onChange={(e) => { const m = minutosDeHora(e.currentTarget.value); if (m !== null) salvar({ quietStart: m }) }} />
        <span aria-hidden="true">→</span>
        <input type="time" dir="ltr" aria-label={t('whatsapp.ajustes.silencioFim')} disabled={!q.on}
          value={horaDeMinutos(q.end)} onChange={(e) => { const m = minutosDeHora(e.currentTarget.value); if (m !== null) salvar({ quietEnd: m }) }} />
      </span>
    </ValueRow>
  )
}

/** MX-18: no modo "sozinho", as conversas que o dono marcou — a fila só responde a elas. */
function Marcadas({ caixa, bot, nome, travado }: { caixa: ReturnType<typeof caixaDe>; bot: BotWhatsapp; nome: string; travado: boolean }) {
  const t = useT()
  const [erro, setErro] = useState<Mensagem | null>(null)
  const marcadas = (caixa.lista(bot.botId)?.conversas ?? []).filter((c) => c.marked)
  const tirar = (c: ConversaWa) => {
    setErro(null)
    caixa.marcarConversa(bot.botId, c.whatsappChatId, false).catch((e: unknown) => setErro(erroDoHub(e)))
  }
  return (
    <CardSection title={t('whatsapp.ajustes.marcadas', { bot: nome })} footer={t('whatsapp.ajustes.marcadasDetalhe')}>
      {erro ? <p className="wa-erro-curto" role="alert"><TextoDaMensagem m={erro} /></p> : null}
      {marcadas.length ? (
        <div className="wa-marcas">
          {marcadas.map((c) => (
            <span key={c.whatsappChatId} className="wa-marca-chip">
              <NomeConversa c={c} olho={false} />
              <button type="button" aria-label={t('whatsapp.ajustes.removerMarca', { nome: rotuloMarca(t, c) })} disabled={travado} onClick={() => tirar(c)}>
                <Icon name="xmark" size={11} />
              </button>
            </span>
          ))}
        </div>
      ) : <p className="wa-nota">{t('whatsapp.ajustes.marcadasVazio')}</p>}
    </CardSection>
  )
}

/** O nome da conversa para o aria do botão de tirar a marca, sem o número de quem não tem nome. */
function rotuloMarca(t: Tradutor, c: ConversaWa): string {
  return c.name?.trim() || (c.phone ? t('whatsapp.numero.escondido', { fim: c.phone.replace(/\D/g, '').slice(-2) }) : t(c.kind === 'group' ? 'whatsapp.estados.semNome.grupo' : 'whatsapp.estados.semNome.contato'))
}

/** MX-18: a fila de saída — pendente, segurada, enviada e a incerta que só o dono decide. */
function FilaEnvio({ estado, botId, conversas, travado }: { estado: ReturnType<typeof useWhatsapp>; botId: string; conversas: readonly ConversaWa[]; travado: boolean }) {
  const t = useT()
  const idioma = useIdioma()
  const fila = estado.filas.get(botId)
  const itens = fila?.items ?? null
  const nomeDe = (i: ItemFila): ReactNode => {
    const c = conversas.find((x) => x.whatsappChatId === i.chatId)
    if (c) return <NomeConversa c={c} olho={false} />
    if (!i.isGroup) {
      const fone = i.chatId.split('@')[0] ?? ''
      if (/^\d{8,}$/.test(fone)) return <NumeroBot phone={fone} olho />
    }
    return <span>{t(i.isGroup ? 'whatsapp.fila.grupo' : 'whatsapp.fila.conversa')}</span>
  }
  const resumo = itens ? resumoFila(itens) : null
  return (
    <CardSection
      title={t('whatsapp.fila.titulo')}
      footer={resumo && resumo.total ? t('whatsapp.fila.resumo', { n: resumo.total, atencao: resumo.atencao }) : t('whatsapp.fila.rodape')}
    >
      {fila?.erro ? <ErroWhatsapp mensagem={fila.erro} tentar={() => void estado.lerFila(botId)} /> : null}
      {!itens ? (fila?.carregando ? <p className="wa-carregando" role="status">{t('whatsapp.caixa.carregando')}</p> : null)
        : !itens.length ? <p className="wa-nota">{t('whatsapp.fila.vazia')}</p>
        : (
          <ul className="wa-fila">
            {itens.map((i) => {
              const motivo = motivoFila(i)
              const decisoes = decisoesDoItem(i)
              return (
                <li key={i.id} className={`wa-fila-item${i.state === 'uncertain' ? ' atencao' : ''}`}>
                  <span className={`wa-fila-icone ${i.state}`} aria-hidden="true">
                    <Icon name={i.state === 'sent' ? 'checkmark' : i.state === 'uncertain' ? 'exclamationmark.triangle' : i.state === 'held' || i.state === 'pending' ? 'clock' : 'xmark'} size={13} />
                  </span>
                  <div className="wa-fila-texto">
                    <b>{comNos(t('whatsapp.fila.para', { nome: no('nome') }), { nome: <span className="wa-fila-destino">{nomeDe(i)}</span> })}</b>
                    <small>
                      {semParametros(t)(estadoFila(i))}
                      {motivo ? <> · {semParametros(t)(motivo)}</> : null}
                      {i.state === 'sent' && i.sentAt ? <> · {horaMensagem(i.sentAt, idioma)}</> : null}
                    </small>
                    <span className="wa-fila-corpo" dir="auto">{i.text}</span>
                    {decisoes.length ? (
                      <span className="wa-fila-acoes">
                        {decisoes.includes('sent') ? <Button onClick={() => void estado.decidirFila(botId, i.id, 'sent')} disabled={travado}>{t('whatsapp.fila.enviou')}</Button> : null}
                        <Button kind="secondary" onClick={() => void estado.decidirFila(botId, i.id, 'cancelled')} disabled={travado}>{t(decisoes.includes('sent') ? 'whatsapp.fila.naoSaiu' : 'whatsapp.fila.cancelar')}</Button>
                      </span>
                    ) : null}
                  </div>
                </li>
              )
            })}
          </ul>
        )}
    </CardSection>
  )
}

/** Arrastar só mexe no número; grava ao soltar (ou ao sair do controle), um pedido por gesto. */
function Espera({ valor, salvar }: { valor: number; salvar: (v: number) => void }) {
  const t = useT()
  const [rascunho, setRascunho] = useState(valor)
  useEffect(() => setRascunho(valor), [valor])
  const gravar = () => { if (rascunho !== valor) salvar(clamp(rascunho, ESPERA)) }
  return (
    <div className="wa-espera" onPointerUp={gravar} onKeyUp={gravar} onBlur={gravar}>
      <Slider label={t('whatsapp.ajustes.intervalo')} value={rascunho} min={ESPERA.min} max={ESPERA.max} shown={t('whatsapp.ajustes.segundos', { n: rascunho })} onChange={setRascunho}
        note={t('whatsapp.ajustes.intervaloNota', { min: ESPERA.min, max: ESPERA.max })} />
    </div>
  )
}

/** R8: por quantos dias as mensagens ficam neste computador (7, 30 ou 90). */
function Prazo({ dias, salvar }: { dias: number; salvar: (d: number) => void }) {
  const t = useT()
  const language = superficieIdioma(useIdioma())
  return (
    <CardSection title={t('whatsapp.prazo.secao')}>
      <ValueRow label={t('whatsapp.prazo.rotulo')} detail={t('whatsapp.prazo.detalhe')}>
        <ChoicePicker language={language} selection={dias} options={PRAZOS.map((d) => ({ id: d, label: t.plural('whatsapp.prazo.dias', d) }))} onChange={salvar} />
      </ValueRow>
    </CardSection>
  )
}
