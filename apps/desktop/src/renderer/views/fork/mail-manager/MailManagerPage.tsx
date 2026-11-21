// Mail Manager: a tela do plugin aberto em Extensões → Plugins, no mesmo desenho limpo da do WhatsApp.
// A lista fechada é fork/extensoes/PaginaPlugins.tsx, que só mostra esta tela depois do clique na linha.
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import { useEffect, useId, useMemo, useState } from 'react'
import { Button, CardForm, CardSection, Pill, Switch } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { spaceAllows } from '../../../lib/fork-space-capabilities'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { Superficie } from '../../settings/rotulo'
import { textoDaMensagem, type Mensagem } from '../bot-settings/mensagem'
import { estadoMailDe, type EstadoMailManager } from './estado'
import { caixasPreviaFaltantes, contagemAutorizados, parseAddressesInput, permissoesDoBot, type BotMail, type CaixaMail, type LeituraMailManager, type PermissaoMail } from './regras'
import './mail-manager.css'

export function MailManagerPage({ store }: { store: BotStore | null }) {
  return (
    <CardForm>
      <Superficie>
      {store
        ? <PluginMailManager store={store} />
        : <ErroMail mensagem={{ chave: 'mailManager.erro.semComputador' }} />}
      </Superficie>
    </CardForm>
  )
}

/** Uma leitura por Hub, compartilhada pelas telas; relê quando a conexão ou a identidade do Hub muda. */
export function useMailManager(store: BotStore, enabled = true) {
  useModel(store)
  const estado = useMemo(() => estadoMailDe(store, () => spaceAllows(store.spaceId, 'connectors') ? store.connectedClient : null), [store])
  useModel(estado)
  const client = store.connectedClient
  // Trocar de Space relê na hora: sem conectores no Space o cliente já é null, então a leitura
  // antiga sai da tela (fail-closed) em vez de ficar como se ainda valesse.
  useEffect(() => { if (enabled) void estado.ler() }, [enabled, estado, client, store.principalEpoch, store.spaceId])
  return estado
}

function PluginMailManager({ store }: { store: BotStore }) {
  const estado = useMailManager(store)
  const t = useT()
  const idioma = useIdioma()
  const { leitura } = estado
  if (!leitura) {
    return estado.erroLeitura
      ? <ErroMail mensagem={estado.erroLeitura} tentar={() => void estado.ler()} />
      : <p className="mm-carregando idioma-superficie" role="status" {...superficieIdioma(idioma)}>{t('mailManager.carregando')}</p>
  }
  const { enabled } = leitura.plugin
  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <section className="mm-plugin" aria-label={t('mailManager.nomePlugin')}>
        <div className="mm-cabeca">
          <MarcaMail />
          <div className="mm-cabeca-texto">
            <h3>{t('mailManager.nomePlugin')}</h3>
            <p>{t('mailManager.descricao')}</p>
          </div>
          <Pill className={enabled ? 'mm-selo on' : 'mm-selo'}>{t(enabled ? 'mailManager.selo.ligado' : 'mailManager.selo.desligado')}</Pill>
          <Switch label={t('mailManager.ligar')} on={enabled} disabled={estado.salvando} onChange={(on) => void estado.salvarPlugin(on)} />
        </div>
        <AvisoPrevia />
        {estado.erroPedido ? <ErroMail mensagem={estado.erroPedido} /> : null}
        <ConectarCaixa estado={estado} leitura={leitura} />
        <CardSection title={t('mailManager.caixas.titulo')}>
          {leitura.mailboxes.length === 0 ? <p className="mm-nota">{t('mailManager.caixas.vazio')}</p> : null}
          {leitura.mailboxes.map((caixa) => <Caixa key={caixa.id} caixa={caixa} leitura={leitura} estado={estado} store={store} />)}
          <p className="mm-nota">{t('mailManager.permissao.nota')}</p>
          {!leitura.personGrantsSupported ? <p className="mm-nota">{t('mailManager.caixas.pessoas')}</p> : null}
        </CardSection>
        <div className="mm-desligar">
          <p><b>{t('mailManager.desligar.titulo')}</b> {t('mailManager.desligar.texto')}</p>
          <p><b>{t('mailManager.religar.titulo')}</b> {t('mailManager.religar.texto')}</p>
        </div>
      </section>
    </div>
  )
}

/** A marca do plugin: a bandeja de e-mail, sem texto solto. */
export const MarcaMail = () => <span className="mm-marca" aria-hidden="true"><Icon name="tray" size={16} /></span>

/** O aviso fixo da prévia, sempre visível e sem botão de fechar. */
function AvisoPrevia() {
  const t = useT()
  return (
    <div className="mm-aviso" role="note">
      <Icon name="shield" size={15} />
      <div>
        <p><b>{t('mailManager.aviso.titulo')}</b> {t('mailManager.aviso.texto')}</p>
        <p>{t('mailManager.aviso.acesso')}</p>
      </div>
    </div>
  )
}

/** Erro do Hub, claro e com nova tentativa; nunca vira lista vazia. A mensagem é dado, traduzida aqui. */
export function ErroMail({ mensagem, tentar }: { mensagem: Mensagem; tentar?: () => void }) {
  const t = useT()
  return (
    <div className="mm-erro" role="alert">
      <Icon name="exclamationmark.triangle" size={14} />
      <span dir={'texto' in mensagem ? 'auto' : undefined}>{textoDaMensagem(t, mensagem)}</span>
      {tentar ? <button onClick={tentar}>{t('mailManager.tentarDeNovo')}</button> : null}
    </div>
  )
}

/** Conectar caixa: só ativa com o plugin ligado; desligado, a nota explica o que falta. */
function ConectarCaixa({ estado, leitura }: { estado: EstadoMailManager; leitura: LeituraMailManager }) {
  const t = useT()
  const [nome, setNome] = useState('')
  const [enderecos, setEnderecos] = useState('')
  const previaFaltante = caixasPreviaFaltantes(leitura).length > 0
  if (!leitura.plugin.enabled) {
    return (
      <CardSection title={t('mailManager.conectar.titulo')}>
        <p className="mm-nota">{t('mailManager.conectar.pluginDesligado')}</p>
      </CardSection>
    )
  }
  const lista = parseAddressesInput(enderecos)
  return (
    <CardSection title={t('mailManager.conectar.titulo')}>
      <div className="mm-form">
        <label className="mm-campo">
          <span>{t('mailManager.conectar.nome')}</span>
          <input value={nome} onChange={(e) => setNome(e.target.value)} disabled={estado.salvando} />
        </label>
        <label className="mm-campo">
          <span>{t('mailManager.conectar.enderecos')} <span className="mm-nota">{t('mailManager.conectar.enderecosDica')}</span></span>
          <textarea value={enderecos} onChange={(e) => setEnderecos(e.target.value)} rows={2} disabled={estado.salvando} />
        </label>
        <div className="mm-form-botoes">
          <Button disabled={estado.salvando || !nome.trim() || lista.length === 0}
            onClick={async () => { if (await estado.conectar(nome.trim(), lista)) { setNome(''); setEnderecos('') } }}>
            {t('mailManager.conectar.botao')}
          </Button>
          <Button kind="secondary" disabled={estado.salvando || !previaFaltante} title={!previaFaltante ? t('mailManager.conectar.previaJaFeita') : undefined}
            onClick={() => void estado.carregarPrevia({ suporte: t('mailManager.previa.suporte'), operacoes: t('mailManager.previa.operacoes') })}>
            {t('mailManager.conectar.carregarPrevia')}
          </Button>
        </div>
      </div>
    </CardSection>
  )
}

/** Uma caixa conectada: nome, selo de prévia, endereços, quantos bots têm acesso e a matriz de bots. */
function Caixa({ caixa, leitura, estado, store }: { caixa: CaixaMail; leitura: LeituraMailManager; estado: EstadoMailManager; store: BotStore }) {
  const t = useT()
  const caixaNomeId = useId()
  const autorizados = contagemAutorizados(caixa)
  return (
    <div className="mm-caixa">
      <div className="mm-caixa-cabeca">
        <bdi id={caixaNomeId} dir="auto" className="mm-caixa-nome">{caixa.label}</bdi>
        <Pill className="mm-selo">{t('mailManager.caixas.previa')}</Pill>
        <span className="mm-nota">{t.plural('mailManager.caixas.autorizados', autorizados)}</span>
        <Button kind="secondary" disabled={estado.salvando} onClick={() => void estado.desconectar(caixa.id)}>{t('mailManager.caixas.desconectar')}</Button>
      </div>
      {caixa.addresses.length > 0 ? (
        <ul className="mm-enderecos">
          {caixa.addresses.map((a) => <li key={a}><bdi dir="ltr">{a}</bdi></li>)}
        </ul>
      ) : null}
      <div className="mm-acessos">
        <span className="mm-acessos-titulo">{t('mailManager.caixas.acessos')}</span>
        {leitura.bots.length === 0 ? <p className="mm-nota">{t('mailManager.caixas.semBots')}</p> : null}
        {leitura.bots.map((bot) => <LinhaBot key={bot.botId} caixa={caixa} caixaNomeId={caixaNomeId} bot={bot} estado={estado} enabled={leitura.plugin.enabled} store={store} />)}
      </div>
    </div>
  )
}

/** Uma linha por bot: rosto, nome e os três controles independentes (Ver, Enviar, Excluir). */
function LinhaBot({ caixa, caixaNomeId, bot, estado, enabled, store }: { caixa: CaixaMail; caixaNomeId: string; bot: BotMail; estado: EstadoMailManager; enabled: boolean; store: BotStore }) {
  const t = useT()
  const botNomeId = useId()
  const permissoes = permissoesDoBot(caixa, bot.botId)
  const info = store.bots.get(bot.botId)
  const controles: { id: PermissaoMail; rotulo: string }[] = [
    { id: 'view', rotulo: t('mailManager.permissao.ver') },
    { id: 'send', rotulo: t('mailManager.permissao.enviar') },
    { id: 'delete', rotulo: t('mailManager.permissao.excluir') },
  ]
  return (
    <div className="mm-bot">
      <DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(info), bot.name, info?.description)} size={26} label={bot.name} animated={false} />
      <span className="mm-bot-nome"><bdi id={botNomeId} dir="auto">{bot.name}</bdi>{bot.isPrimary ? <Pill>{t('mailManager.principal')}</Pill> : null}</span>
      <span className="mm-bot-acoes">
        {controles.map((c) => (
          <button key={c.id} type="button" className="mm-toggle" aria-pressed={permissoes[c.id]}
            aria-labelledby={`${botNomeId}-${c.id} ${botNomeId} ${caixaNomeId}`}
            disabled={!enabled || estado.salvando}
            onClick={() => void estado.alterarPermissao(caixa.id, bot.botId, c.id, !permissoes[c.id])}>
            <span id={`${botNomeId}-${c.id}`}>{c.rotulo}</span>
          </button>
        ))}
      </span>
    </div>
  )
}
