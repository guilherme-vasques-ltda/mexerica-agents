import { useEffect, useRef, useState } from 'react'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import { Button, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { ModalHeader, Sheet } from '../../../components/Overlay'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import type { BotStore } from '../../../store/bot-store'
import type { EstadoWhatsapp } from './estado'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { comNos, no, semParametros } from '../../settings/rotulo'
import { AvisoNaoOficial, ErroWhatsapp } from './partes'
import { codigoFormatado, RESULTADO_TEXTO, telefoneParece, type BotWhatsapp } from './regras'

// whatsapp R5b: "Conectar número" (proposta whatsapp-20261105, cenas parear-*). Riscos (caixa obrigatória) →
// QR ou código de 8 letras → resultado do Hub (R2). Código e QR só aparecem na tela: nada de log,
// analytics, título ou área de transferência automática.

type Passo = 'riscos' | 'conectar'
/** O WhatsApp troca o QR a cada ~20 s; o Hub manda o novo e o app relê. */
const QR_MUDA_S = 20

export function Pareamento({ store, estado, bot, nome, aberto, aoFechar }: { store: BotStore; estado: EstadoWhatsapp; bot: BotWhatsapp; nome: string; aberto: boolean; aoFechar: () => void }) {
  const pairing = bot.number.pairing
  const emCurso = bot.number.state === 'pairing' && pairing?.result === 'waiting'
  const [passo, setPasso] = useState<Passo>(emCurso ? 'conectar' : 'riscos')
  const [entendi, setEntendi] = useState(false)
  const [modo, setModo] = useState<'qr' | 'code'>(pairing?.method ?? 'qr')
  const [telefone, setTelefone] = useState('')
  /** Pedido feito nesta folha: só então um resultado do Hub vira a tela "Pronto". */
  const [iniciado, setIniciado] = useState(emCurso)
  const resultado = iniciado && pairing && pairing.result !== 'waiting' ? pairing.result : null
  const esperando = iniciado && !resultado && pairing?.result === 'waiting'

  const pedir = (m: 'qr' | 'code') => {
    setModo(m)
    setIniciado(true)
    void estado.parear(bot.botId, m, m === 'code' ? telefone : undefined)
  }
  const fechar = () => {
    // Pedido disparado e sem resultado: cancela também antes do primeiro QR chegar (R363-1).
    if (iniciado && !resultado) void estado.cancelarPareamento(bot.botId)
    aoFechar()
  }
  const info = store.bots.get(bot.botId)
  const passoAtual = resultado ? 2 : passo === 'conectar' ? 1 : 0
  const t = useT()
  const idioma = useIdioma()
  const passos = [t('whatsapp.pareamento.passo.riscos'), t('whatsapp.pareamento.passo.conectar'), t('whatsapp.pareamento.passo.pronto')]

  return (
    <Sheet language={superficieIdioma(idioma)} open={aberto} onClose={fechar} width={640} label={t('whatsapp.pareamento.titulo', { bot: nome })}>
      <div className="wa-parear idioma-superficie" {...superficieIdioma(idioma)}>
        <ModalHeader icon={<DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(info), nome, info?.description)} size={36} label={nome} animated={false} />}
          title={comNos(t('whatsapp.pareamento.titulo', { bot: no('bot') }), { bot: <bdi dir="auto">{nome}</bdi> })} subtitle={t('whatsapp.pareamento.subtitulo')} onClose={fechar} closeTitle={t('whatsapp.pareamento.fechar')} />
        <ol className="wa-passos" aria-label={t('whatsapp.pareamento.passos')}>
          {passos.map((p, i) => <li key={i} className={i <= passoAtual ? 'feito' : ''} aria-current={i === passoAtual ? 'step' : undefined}>{p}</li>)}
        </ol>
        <div className="wa-parear-corpo">
          {estado.erroPedido ? <ErroWhatsapp mensagem={estado.erroPedido} /> : null}
          {resultado ? <Resultado bot={bot} nome={nome} resultado={resultado} />
            : passo === 'riscos' ? <Riscos nome={nome} entendi={entendi} setEntendi={setEntendi} />
            : modo === 'qr' ? <Qr nome={nome} texto={iniciado ? pairing?.qr : undefined} pedindo={estado.salvando} computador={store.hostName || undefined} />
            : <Codigo nome={nome} telefone={telefone} setTelefone={setTelefone} codigo={iniciado && pairing?.method === 'code' ? pairing.code : undefined} pedindo={estado.salvando} pedir={() => pedir('code')} />}
        </div>
        <footer className="wa-parear-rodape">
          {resultado === 'connected' ? <><span /><Button onClick={aoFechar}>{comNos(t('whatsapp.pareamento.escolher', { bot: no('bot') }), { bot: <bdi dir="auto">{nome}</bdi> })}</Button></>
            : resultado ? <><Button kind="secondary" onClick={aoFechar}>{t('whatsapp.pareamento.fechar')}</Button><Button onClick={() => pedir(modo)} disabled={modo === 'code' && !telefoneParece(telefone)}>{t('whatsapp.estados.tentarDeNovo')}</Button></>
            : passo === 'riscos' ? <>
              <span className="wa-nota">{entendi ? '' : t('whatsapp.pareamento.marque')}</span>
              <span className="wa-parear-botoes"><Button kind="secondary" onClick={fechar}>{t('whatsapp.estados.cancelar')}</Button><Button disabled={!entendi} onClick={() => { setPasso('conectar'); pedir('qr') }}>{t('whatsapp.pareamento.continuar')}</Button></span>
            </> : <>
              {modo === 'qr'
                ? <Button kind="secondary" onClick={() => setModo('code')}>{t('whatsapp.pareamento.comTelefone')}</Button>
                : <Button kind="secondary" onClick={() => pedir('qr')}>{t('whatsapp.pareamento.usarQr')}</Button>}
              <span className="wa-parear-botoes">{esperando ? <span className="wa-nota">{t('whatsapp.pareamento.esperandoCelular')}</span> : null}<Button kind="secondary" onClick={fechar}>{t('whatsapp.estados.cancelar')}</Button></span>
            </>}
        </footer>
      </div>
    </Sheet>
  )
}

/** O nome do bot dentro de uma frase do catálogo, isolado (D454). */
const comNome = (texto: string, nome: string) => comNos(texto, { bot: <bdi dir="auto">{nome}</bdi> })

function Riscos({ nome, entendi, setEntendi }: { nome: string; entendi: boolean; setEntendi: (v: boolean) => void }) {
  const t = useT()
  return (
    <>
      <AvisoNaoOficial />
      <ul className="wa-riscos">
        <li><Icon name="person.crop.circle" size={14} /><span><b>{comNome(t('whatsapp.pareamento.riscos.chip', { bot: no('bot') }), nome)}</b> {t('whatsapp.pareamento.riscos.banido')}</span></li>
        <li><Icon name="desktopcomputer" size={14} /><span>{t('whatsapp.pareamento.riscos.aparelho')} {comNome(t('whatsapp.pareamento.riscos.soLigado', { bot: no('bot') }), nome)}</span></li>
        <li><Icon name="shield" size={14} /><span>{comNos(t('whatsapp.pareamento.riscos.nada', { bot: no('bot'), naoPode: no('naoPode') }), { bot: <bdi dir="auto">{nome}</bdi>, naoPode: <b>{t('whatsapp.pareamento.riscos.naoPode')}</b> })}</span></li>
        <li><Icon name="iphone" size={14} /><span>{t('whatsapp.pareamento.riscos.numeroSo')}</span></li>
      </ul>
      <label className="wa-entendi">
        <input type="checkbox" checked={entendi} onChange={(e) => setEntendi(e.currentTarget.checked)} />
        <b>{t('whatsapp.pareamento.entendi')}</b>
      </label>
    </>
  )
}

/** Os rótulos do app WhatsApp no celular, em negrito dentro da instrução. */
function rotulosWhatsapp(t: ReturnType<typeof useT>) {
  const b = (chave: 'whatsapp.pareamento.wa.whatsapp' | 'whatsapp.pareamento.wa.configuracoes' | 'whatsapp.pareamento.wa.aparelhos' | 'whatsapp.pareamento.wa.conectarAparelho' | 'whatsapp.pareamento.wa.comTelefone') => <b>{t(chave)}</b>
  return {
    marcas: { whatsapp: no('whatsapp'), configuracoes: no('configuracoes'), aparelhos: no('aparelhos'), conectarAparelho: no('conectarAparelho'), comTelefone: no('comTelefone') },
    nos: { whatsapp: b('whatsapp.pareamento.wa.whatsapp'), configuracoes: b('whatsapp.pareamento.wa.configuracoes'), aparelhos: b('whatsapp.pareamento.wa.aparelhos'), conectarAparelho: b('whatsapp.pareamento.wa.conectarAparelho'), comTelefone: b('whatsapp.pareamento.wa.comTelefone') },
  }
}

function Qr({ nome, texto, pedindo, computador }: { nome: string; texto?: string; pedindo: boolean; computador?: string }) {
  const t = useT()
  const wa = rotulosWhatsapp(t)
  const [imagem, setImagem] = useState<string | null>(null)
  const [restam, setRestam] = useState(QR_MUDA_S)
  useEffect(() => {
    setImagem(null)
    if (!texto) return
    let vivo = true
    // A biblioteca só carrega quando a folha precisa dela (D249 E2).
    void import('qrcode').then(({ default: QRCode }) => QRCode.toDataURL(texto, { errorCorrectionLevel: 'M', margin: 1, width: 400, color: { dark: '#000000', light: '#ffffff' } }))
      .then((url) => { if (vivo) setImagem(url) }).catch(() => {})
    setRestam(QR_MUDA_S)
    const relogio = setInterval(() => setRestam((s) => Math.max(0, s - 1)), 1000)
    return () => { vivo = false; clearInterval(relogio) }
  }, [texto])
  return (
    <div className="wa-qr-bloco">
      <div className="wa-qr-coluna">
        <div className="wa-qr">{imagem ? <img src={imagem} alt={t('whatsapp.pareamento.qr.alt', { bot: nome })} width={200} height={200} /> : <Spinner size={22} />}</div>
        <span className="wa-selo-tempo" aria-live="polite"><Icon name="clock" size={12} />{imagem ? (restam > 0 ? t('whatsapp.pareamento.qr.muda', { s: restam }) : t('whatsapp.pareamento.qr.trocando')) : pedindo ? t('whatsapp.pareamento.qr.pedindo') : t('whatsapp.pareamento.qr.esperando')}</span>
      </div>
      <div>
        <h3 className="wa-parear-titulo">{comNome(t('whatsapp.pareamento.qr.titulo', { bot: no('bot') }), nome)}</h3>
        <ol className="wa-instrucoes">
          <li>{comNos(t('whatsapp.pareamento.qr.passo1', wa.marcas), wa.nos)}</li>
          <li>{comNos(t('whatsapp.pareamento.qr.passo2', wa.marcas), wa.nos)}</li>
          <li>{t('whatsapp.pareamento.qr.passo3')}</li>
        </ol>
        <p className="wa-nota"><Icon name="desktopcomputer" size={12} /> {computador ? comNos(t('whatsapp.pareamento.qr.vaiEm', { computador: no('computador') }), { computador: <b><bdi dir="auto">{computador}</bdi></b> }) : t('whatsapp.pareamento.qr.vaiAqui')}</p>
      </div>
    </div>
  )
}

function Codigo({ nome, telefone, setTelefone, codigo, pedindo, pedir }: { nome: string; telefone: string; setTelefone: (v: string) => void; codigo?: string; pedindo: boolean; pedir: () => void }) {
  const t = useT()
  const wa = rotulosWhatsapp(t)
  const campo = useRef<HTMLInputElement>(null)
  useEffect(() => { if (!codigo) campo.current?.focus() }, [codigo])
  const ok = telefoneParece(telefone)
  return (
    <div className="wa-codigo-bloco">
      <label className="wa-campo">
        <b>{comNome(t('whatsapp.pareamento.codigo.rotulo', { bot: no('bot') }), nome)}</b>
        <input ref={campo} className="field-box" dir="ltr" type="tel" inputMode="tel" autoComplete="off" placeholder="+55 11 90000-0000" value={telefone}
          onChange={(e) => setTelefone(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === 'Enter' && ok && !pedindo) pedir() }} />
        <span className="wa-nota">{t('whatsapp.pareamento.codigo.dica')}</span>
      </label>
      {codigo ? (
        <>
          <div className="wa-codigo" dir="ltr" aria-label={t('whatsapp.pareamento.codigo.aria', { codigo: codigoFormatado(codigo).split('').join(' ') })}>{codigoFormatado(codigo)}</div>
          <ol className="wa-instrucoes">
            <li>{comNos(t('whatsapp.pareamento.codigo.passo1', wa.marcas), wa.nos)}</li>
            <li>{comNos(t('whatsapp.pareamento.codigo.passo2', wa.marcas), wa.nos)}</li>
            <li>{t('whatsapp.pareamento.codigo.passo3')}</li>
          </ol>
          <span className="wa-parear-botoes"><span className="wa-selo-tempo"><Icon name="clock" size={12} />{t('whatsapp.pareamento.codigo.vale')}</span><Button kind="secondary" disabled={!ok || pedindo} onClick={pedir}>{t('whatsapp.pareamento.codigo.outro')}</Button></span>
        </>
      ) : <Button disabled={!ok || pedindo} onClick={pedir}>{pedindo ? t('whatsapp.pareamento.codigo.pedindo') : t('whatsapp.pareamento.codigo.receber')}</Button>}
    </div>
  )
}

function Resultado({ bot, nome, resultado }: { bot: BotWhatsapp; nome: string; resultado: keyof typeof RESULTADO_TEXTO }) {
  const t = useT()
  const texto = RESULTADO_TEXTO[resultado]
  const ok = resultado === 'connected'
  const opcoes: [string, boolean][] = [[t('whatsapp.estados.opcao.verConversas'), bot.botCan.readChats], [t('whatsapp.estados.opcao.responderConversas'), bot.botCan.replyChats], [t('whatsapp.estados.opcao.verGrupos'), bot.botCan.readGroups], [t('whatsapp.estados.opcao.responderGrupos'), bot.botCan.replyGroups]]
  return (
    <>
      <div className={`wa-resultado${ok ? ' ok' : ''}`} role="status">
        <span className="wa-resultado-icone"><Icon name={ok ? 'checkmark' : 'exclamationmark.triangle'} size={18} /></span>
        <div><b>{semParametros(t)(texto.titulo)}</b><span className="wa-nota">{semParametros(t)(texto.detalhe)}</span></div>
      </div>
      {ok ? (
        <>
          <p className="wa-parear-titulo">{comNome(t('whatsapp.pareamento.agora', { bot: no('bot') }), nome)}</p>
          <ul className="wa-desligadas">{opcoes.map(([rotulo, v]) => <li key={rotulo}><span>{rotulo}</span><span className="wa-nota">{t(v ? 'whatsapp.estados.ligado' : 'whatsapp.estados.desligado')}</span></li>)}</ul>
        </>
      ) : null}
    </>
  )
}
