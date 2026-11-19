import { useState } from 'react'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Dialog } from '../../../components/Overlay'
import type { BotStore } from '../../../store/bot-store'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { comNos, no, semParametros } from '../../settings/rotulo'
import { textoDaMensagem } from '../bot-settings/mensagem'
import type { EstadoWhatsapp } from './estado'
import { Pareamento } from './Pareamento'
import { acoesDoNumero, bloqueioConectar, numeroAjuda, numeroNoComputador, numeroTexto, numeroTom, type BotWhatsapp, type LeituraWhatsapp, type Numero } from './regras'

// whatsapp R5b: o número de cada bot nas duas telas (estado, Conectar, Desconectar), com os RPCs do R2.

/** `computador`: o nome do Hub do bot (o app já conhece); `pessoal`: Mac ou PC, que desligam. O telefone vem no R3. */
export function EstadoDoNumero({ numero, nome, ajuda = true, computador, pessoal = false }: { numero: Numero; nome: string; ajuda?: boolean; computador?: string; pessoal?: boolean }) {
  const t = useT()
  const ajudaChave = ajuda ? numeroAjuda(numero) : null
  const aqui = computador && numeroNoComputador(numero)
  return (
    <>
      <span className={`wa-bot-linha tom-${numeroTom(numero)}`}><span className="wa-ponto" aria-hidden="true" />{semParametros(t)(numeroTexto(numero))}
        {aqui ? <span className="wa-nota"><Icon name="desktopcomputer" size={11} /> {comNos(t('whatsapp.estados.numeroAqui', { computador: no('computador') }), { computador: <b><bdi dir="auto">{computador}</bdi></b> })}</span> : null}
      </span>
      {ajudaChave ? <span className="wa-nota">{comNos(t(ajudaChave as 'whatsapp.estados.ajuda.caiu', { bot: no('bot') }), { bot: <bdi dir="auto">{nome}</bdi> })}</span> : null}
      {aqui && pessoal ? <span className="wa-nota">{t('whatsapp.estados.soComLigado')}</span> : null}
    </>
  )
}

/** "Conectar número" e a folha do pareamento. Some quando o número já está conectado. */
export function ConectarNumero({ store, estado, leitura, bot, nome, disabled = false }: { store: BotStore; estado: EstadoWhatsapp; leitura: LeituraWhatsapp; bot: BotWhatsapp; nome: string; disabled?: boolean }) {
  const t = useT()
  const [aberto, setAberto] = useState(false)
  const acoes = acoesDoNumero(bot.number)
  if (!acoes.conectar && !aberto) return null
  const chaveBloqueio = bloqueioConectar(leitura, bot)
  const bloqueio = chaveBloqueio ? semParametros(t)(chaveBloqueio) : null
  return (
    <>
      {acoes.conectar ? <Button kind={bot.number.state === 'none' ? 'primary' : 'secondary'} disabled={disabled || bloqueio !== null} title={bloqueio ?? undefined} onClick={() => setAberto(true)}>{semParametros(t)(acoes.rotuloConectar)}</Button> : null}
      {bloqueio && bot.number.state === 'not_installed' ? <span className="wa-nota">{bloqueio}</span> : null}
      {aberto ? <Pareamento store={store} estado={estado} bot={bot} nome={nome} aberto={aberto} aoFechar={() => setAberto(false)} /> : null}
    </>
  )
}

/** "Desconectar" e "Desconectar e apagar", com confirmação. Se o aviso ao WhatsApp falhar, diz o que fazer no celular. */
export function DesconectarNumero({ estado, bot, nome, disabled }: { estado: EstadoWhatsapp; bot: BotWhatsapp; nome: string; disabled: boolean }) {
  const t = useT()
  const language = superficieIdioma(useIdioma())
  const [confirmar, setConfirmar] = useState<'sair' | 'apagar' | null>(null)
  const pode = acoesDoNumero(bot.number).desconectar && !disabled && !estado.salvando
  const aviso = estado.avisoLogout?.botId === bot.botId ? estado.avisoLogout.mensagem : null
  return (
    <div className="wa-rodape">
      <Button kind="secondary" disabled={!pode} onClick={() => setConfirmar('sair')}>{t('whatsapp.estados.desconectar.botao')}</Button>
      <Button kind="secondary" disabled={disabled || estado.salvando || bot.number.state === 'pairing'} className="wa-perigo" onClick={() => setConfirmar('apagar')}>{t('whatsapp.estados.apagar.botao')}</Button>
      {aviso ? <p className="wa-aviso-logout" role="alert" dir={'texto' in aviso ? 'auto' : undefined}>{textoDaMensagem(t, aviso)}</p> : null}
      <Dialog open={confirmar === 'sair'} language={language} title={t('whatsapp.estados.desconectar.titulo', { bot: nome })}
        message={t('whatsapp.estados.desconectar.mensagem')} cancel={t('whatsapp.estados.cancelar')}
        onClose={() => setConfirmar(null)} actions={[{ title: t('whatsapp.estados.desconectar.acao'), action: () => { void estado.desconectar(bot.botId, false) } }]} />
      <Dialog open={confirmar === 'apagar'} language={language} title={t('whatsapp.estados.apagar.titulo')}
        message={t('whatsapp.estados.apagar.mensagem', { bot: nome })} cancel={t('whatsapp.estados.cancelar')}
        onClose={() => setConfirmar(null)} actions={[{ title: t('whatsapp.estados.apagar.acao'), destructive: true, action: () => { void estado.desconectar(bot.botId, true) } }]} />
    </div>
  )
}
