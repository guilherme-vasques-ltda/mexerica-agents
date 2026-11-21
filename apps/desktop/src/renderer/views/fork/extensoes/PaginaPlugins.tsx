// Revisão do app (Entrega 2, itens I e K; pedido do dono em 10/11/2026): Plugins é uma página de Extensões e uma
// LISTA. Cada plugin é uma linha fechada (marca, nome, o que faz, ligado ou desligado); só o clique abre as opções
// e a visualização dele. Hoje há um plugin (WhatsApp); o próximo entra como mais uma linha e mais um detalhe.
import { useEffect, useRef, type ReactNode } from 'react'
import { Pill } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { useEscape } from '../../../components/Overlay'
import { useT } from '../../../lib/fork-i18n/estado'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import type { BotStore } from '../../../store/bot-store'
import { semParametros, Superficie } from '../../settings/rotulo'
import { PluginsPage as DetalheWhatsapp } from '../whatsapp/PluginsPage'
import { MarcaW, useWhatsapp } from '../whatsapp/partes'
import { MailManagerPage as DetalheMailManager, MarcaMail, useMailManager } from '../mail-manager/MailManagerPage'

export const PLUGIN_WHATSAPP = 'whatsapp'
export const PLUGIN_MAIL_MANAGER = 'mail-manager'

export function PaginaPlugins({ store, aberto, onAbrir, openVoice }: {
  store: BotStore
  /** O plugin aberto; `null` mostra a lista. */
  aberto: string | null
  onAbrir: (plugin: string | null) => void
  openVoice: () => void
}) {
  if (aberto === PLUGIN_WHATSAPP) {
    return <PluginAberto onVoltar={() => onAbrir(null)}><DetalheWhatsapp store={store} openVoice={openVoice} /></PluginAberto>
  }
  if (aberto === PLUGIN_MAIL_MANAGER) {
    return <PluginAberto onVoltar={() => onAbrir(null)}><DetalheMailManager store={store} /></PluginAberto>
  }
  return (
    <Superficie>
      <div className="ext-plugins" role="list">
        <LinhaWhatsapp store={store} onAbrir={() => onAbrir(PLUGIN_WHATSAPP)} />
        <LinhaMailManager store={store} onAbrir={() => onAbrir(PLUGIN_MAIL_MANAGER)} />
      </div>
      <PeDaLista />
    </Superficie>
  )
}

/** Com um plugin só, a lista diz que os próximos entram aqui (para não parecer tela pela metade). */
function PeDaLista() {
  const t = useT()
  return <p className="ext-plugins-rodape"><Icon name="plus" size={11} />{t('extensoes.plugins.rodape')}</p>
}

/** A linha fechada: nada de opção aqui; ligar e desligar ficam dentro, junto do aviso e do que cada bot pode. */
function LinhaPlugin({ marca, nome, detalhe, ligado, seloLigado = 'whatsapp.estados.ligado', seloDesligado = 'whatsapp.estados.desligado', onAbrir }: { marca: ReactNode; nome: string; detalhe: string; /** `null`: ainda lendo ou sem leitura. */ ligado: boolean | null; seloLigado?: ChaveTexto; seloDesligado?: ChaveTexto; onAbrir: () => void }) {
  const t = useT()
  return (
    <div role="listitem">
      <button className="ext-plugin-linha press" onClick={onAbrir} aria-label={t('extensoes.plugins.abrir', { nome })}>
        {marca}
        <span className="ext-plugin-texto">
          <span className="ext-plugin-nome">{nome}</span>
          <span className="ext-plugin-detalhe">{detalhe}</span>
        </span>
        {ligado === null ? null : <Pill className={ligado ? 'ext-plugin-selo on' : 'ext-plugin-selo'}><span className="ext-plugin-ponto" aria-hidden="true" />{semParametros(t)(ligado ? seloLigado : seloDesligado)}</Pill>}
        <Icon name="chevron.right" size={11} weight="semibold" className="ext-plugin-seta" />
      </button>
    </div>
  )
}

function LinhaWhatsapp({ store, onAbrir }: { store: BotStore; onAbrir: () => void }) {
  const t = useT()
  const estado = useWhatsapp(store)
  return <LinhaPlugin marca={<MarcaW />} nome={t('whatsapp.ajustes.nomePlugin')} detalhe={t('extensoes.plugins.whatsapp')} ligado={estado.leitura ? estado.leitura.plugin.enabled : null} onAbrir={onAbrir} />
}

function LinhaMailManager({ store, onAbrir }: { store: BotStore; onAbrir: () => void }) {
  const t = useT()
  const estado = useMailManager(store)
  return <LinhaPlugin marca={<MarcaMail />} nome={t('mailManager.nomePlugin')} detalhe={t('extensoes.plugins.mailManager')} ligado={estado.leitura ? estado.leitura.plugin.enabled : null} seloLigado="mailManager.selo.ligado" seloDesligado="mailManager.selo.desligado" onAbrir={onAbrir} />
}

/** O plugin aberto: uma linha de Voltar (e Esc) sobre a tela dele; o foco entra no Voltar. */
function PluginAberto({ onVoltar, children }: { onVoltar: () => void; children: ReactNode }) {
  const t = useT()
  const voltar = useRef<HTMLButtonElement>(null)
  useEffect(() => voltar.current?.focus({ preventScroll: true }), [])
  useEscape(true, onVoltar, voltar)
  return (
    <div className="ext-plugin-aberto">
      <Superficie>
        <div className="ext-plugin-volta">
          <button ref={voltar} className="ext-plugin-voltar press" onClick={onVoltar} aria-label={t('extensoes.plugins.voltar')}>
            <Icon name="chevron.left" size={11} weight="semibold" />
            {t('extensoes.pagina.plugins')}
          </button>
        </div>
      </Superficie>
      <div className="ext-plugin-corpo">{children}</div>
    </div>
  )
}
