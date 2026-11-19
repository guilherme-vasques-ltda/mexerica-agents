import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import { semParametros, Superficie } from '../../settings/rotulo'
import { Button, CardForm, CardSection, Pill, SegmentedChoice, Switch } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import type { BotStore } from '../../../store/bot-store'
import { AvisoNaoOficial, ErroWhatsapp, MarcaW, QuatroEstados, useWhatsapp } from './partes'
import type { EstadoWhatsapp } from './estado'
import { ConectarNumero, EstadoDoNumero } from './Numero'
import { avisoWacli, computadorPessoal, MODO_TEXTO, oQueOBotPode, quatroEstados, transcricaoTexto, type BotWhatsapp, type LeituraWhatsapp, type Modo } from './regras'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'

// D544: Agenda é trabalho interno dos bots em Rotinas; Plugins conserva o WhatsApp.
// Revisão do app (Entrega 2): esta é a tela do plugin WhatsApp ABERTO. A lista de plugins, fechada, é
// fork/extensoes/PaginaPlugins.tsx (Extensões → Plugins), que só mostra esta tela depois do clique na linha.

export function PluginsPage({ store, openVoice }: { store: BotStore | null; openVoice: () => void }) {
  // R1-02 do #587: todos os retornos (sem computador, carregando, erro e a tela) ficam na superfície do idioma do app.
  return (
    <CardForm>
      <Superficie>
      {store
        ? <WhatsappPlugin store={store} openVoice={openVoice} />
        : <ErroWhatsapp mensagem={{ chave: 'whatsapp.ajustes.semComputador' }} />}
      </Superficie>
    </CardForm>
  )
}

function WhatsappPlugin({ store, openVoice }: { store: BotStore; openVoice: () => void }) {
  const estado = useWhatsapp(store)
  const t = useT()
  const idioma = useIdioma()
  const { leitura } = estado
  if (!leitura) {
    return estado.erroLeitura
      ? <ErroWhatsapp mensagem={estado.erroLeitura} tentar={() => void estado.ler()} />
      : <p className="wa-carregando idioma-superficie" role="status" {...superficieIdioma(idioma)}>{t('whatsapp.ajustes.carregando')}</p>
  }
  const { enabled, mode } = leitura.plugin
  const nome = (id: string) => store.bots.get(id)?.name ?? id
  // Auditoria da proposta (2ª rodada): a tela foi refeita, sem mudar regra nem contrato. Os bots que o modo atual
  // pausa saem da lista e ficam recolhidos em "Outros bots (N)", com a frase do motivo uma vez só.
  const ativos = leitura.bots.filter((b) => !b.paused)
  const pausados = leitura.bots.filter((b) => b.paused)
  const linha = (b: BotWhatsapp) => <LinhaBot key={b.botId} bot={b} store={store} nome={nome(b.botId)} ligado={enabled} estado={estado} leitura={leitura} />
  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <section className="wa-plugin" aria-label={t('whatsapp.ajustes.nomePlugin')}>
        <div className="wa-plugin-cabeca">
          <MarcaW />
          <div className="wa-plugin-texto">
            <h3>{t('whatsapp.ajustes.nomePlugin')}</h3>
            <p>{t('whatsapp.ajustes.descricao')}</p>
          </div>
          <Pill className={enabled ? 'wa-selo on' : 'wa-selo'}>{t(enabled ? 'whatsapp.estados.ligado' : 'whatsapp.estados.desligado')}</Pill>
          <Switch label={t('whatsapp.ajustes.ligar')} on={enabled} disabled={estado.salvando} onChange={(on) => void estado.salvarHub({ enabled: on })} />
        </div>
        <AvisoNaoOficial />
        {estado.erroPedido ? <ErroWhatsapp mensagem={estado.erroPedido} /> : null}
        <div className="wa-modo">
          <b>{t('whatsapp.ajustes.comoUsar')}</b>
          <fieldset disabled={estado.salvando} className="wa-sem-borda">
            <SegmentedChoice<Modo> selection={mode} options={[{ id: 'donna_only', label: t('marca.whatsapp.soBerga') }, { id: 'per_bot', label: t('whatsapp.ajustes.umPorBot') }]} onChange={(m) => { if (m !== mode) void estado.salvarHub({ mode: m }) }} />
          </fieldset>
          <span>{semParametros(t)(MODO_TEXTO[mode])}</span>
        </div>
        <QuatroEstados estados={quatroEstados(leitura)} fino />
        {enabled ? <LinhaWacli estado={estado} /> : null}
        <CardSection title={t('whatsapp.ajustes.bots')} accessory={<span className="wa-nota">{t('whatsapp.ajustes.botsDetalhe')}</span>} footer={t('whatsapp.ajustes.botsRodape')}>
          {leitura.bots.length === 0 ? <p className="wa-nota">{t('whatsapp.ajustes.nenhumBot')}</p> : null}
          {ativos.map(linha)}
          {pausados.length > 0 ? (
            <details key="outros" className="wa-outros">
              <summary>
                <Icon name="chevron.right" size={10} weight="semibold" className="wa-outros-seta" />
                <b>{t('whatsapp.ajustes.outrosBots', { n: pausados.length })}</b>
                <span className="wa-nota">{t('whatsapp.ajustes.pausadoModo')}</span>
              </summary>
              <div className="wa-outros-lista">{pausados.map(linha)}</div>
            </details>
          ) : null}
        </CardSection>
        <p className="wa-voz">
          <Icon name="mic" size={13} />
          <span>{t('whatsapp.ajustes.vozLinha', { transcricao: transcricaoTexto(t, leitura.transcription) })}</span>
          <button className="wa-link" onClick={openVoice}>{t('whatsapp.ajustes.abrirVoz')} <Icon name="chevron.right" size={10} /></button>
        </p>
        <div className="wa-desligar">
          <p><b>{t('whatsapp.ajustes.desligarTitulo')}</b> {t('whatsapp.ajustes.desligarTexto')}</p>
          <p><b>{t('whatsapp.ajustes.religarTitulo')}</b> {t('whatsapp.ajustes.religarTexto')}</p>
        </div>
      </section>
    </div>
  )
}

/** Uma linha por bot: rosto, nome, o estado do número e o que ele pode; a frase de "pausado" fica no grupo recolhido. */
function LinhaBot({ bot, store, nome, ligado, estado, leitura }: { bot: BotWhatsapp; store: BotStore; nome: string; ligado: boolean; estado: EstadoWhatsapp; leitura: LeituraWhatsapp }) {
  const info = store.bots.get(bot.botId)
  const t = useT()
  return (
    <div className={`wa-bot${bot.paused ? ' pausado' : ''}`}>
      <DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(info), nome, info?.description)} size={26} label={nome} animated={false} />
      <div className="wa-bot-texto">
        <span className="wa-bot-nome"><bdi dir="auto">{nome}</bdi>{bot.isDonna ? <Pill>{t('whatsapp.ajustes.principal')}</Pill> : null}</span>
        <EstadoDoNumero numero={bot.number} nome={nome} ajuda={!bot.paused && !(bot.number.state === 'not_installed' && leitura.plugin.wacli?.state)} computador={store.hostName || undefined} pessoal={computadorPessoal(store.hello?.os)} />
        {bot.paused ? null : <span className="wa-nota">{semParametros(t)(ligado ? oQueOBotPode(bot) : 'whatsapp.ajustes.pluginDesligado')}</span>}
      </div>
      <div className="wa-bot-acoes">
        {ligado && !bot.paused ? <ConectarNumero store={store} estado={estado} leitura={leitura} bot={bot} nome={nome} /> : null}
      </div>
    </div>
  )
}

/** R7: o estado do download do wacli, com "Tentar de novo" depois de uma falha. */
function LinhaWacli({ estado }: { estado: EstadoWhatsapp }) {
  const t = useT()
  const aviso = avisoWacli(t, estado.leitura?.plugin.wacli)
  if (!aviso) return null
  return (
    <div className={`wa-wacli tom-${aviso.tom}`} role={aviso.tom === 'atencao' ? 'alert' : 'status'}>
      <span className="wa-ponto" aria-hidden="true" />
      <span className="wa-wacli-texto"><b>{aviso.titulo}</b><span className="wa-nota" dir="auto">{aviso.detalhe}</span></span>
      {aviso.tentar ? <Button kind="secondary" disabled={estado.salvando} onClick={() => void estado.tentarWacli()}>{t('whatsapp.estados.tentarDeNovo')}</Button> : null}
    </div>
  )
}
