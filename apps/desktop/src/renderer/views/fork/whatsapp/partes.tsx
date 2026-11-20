import { spaceAllows } from '../../../lib/fork-space-capabilities'
import { useEffect, useMemo, type ReactNode } from 'react'
import { Icon } from '../../../components/Icon'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { useT } from '../../../lib/fork-i18n/estado'
import { comNos, no, semParametros } from '../../settings/rotulo'
import { textoDaMensagem, type Mensagem } from '../bot-settings/mensagem'
import { estadoDe } from './estado'
import { AVISO_FIXO, FRASE_ESTADOS, relerEmMs, type Estados } from './regras'
import './whatsapp.css'

/** Uma leitura por Hub, compartilhada pelas telas (D350); relê quando a conexão ou a identidade do Hub muda. */
export function useWhatsapp(store: BotStore, enabled = true) {
  useModel(store)
  const estado = useMemo(() => estadoDe(store, () => spaceAllows(store.spaceId, 'connectors') ? store.connectedClient : null), [store])
  useModel(estado)
  const client = store.connectedClient
  useEffect(() => { if (enabled && spaceAllows(store.spaceId, 'connectors')) void estado.ler() }, [enabled, estado, client, store.principalEpoch, store.whatsappEpoch])
  // R7: o fim do download não gera aviso do Hub; enquanto baixa, relê.
  const espera = relerEmMs(estado.leitura)
  useEffect(() => {
    if (!enabled || espera === null || !spaceAllows(store.spaceId, 'connectors')) return
    const t = setTimeout(() => void estado.ler(), espera)
    return () => clearTimeout(t)
  }, [enabled, estado, espera, estado.leitura])
  return estado
}

/** O aviso fixo, sempre visível e sem botão de fechar. */
export function AvisoNaoOficial() {
  const t = useT()
  return (
    <div className="wa-aviso" role="note">
      <Icon name="shield" size={15} />
      <p><b>{t('whatsapp.estados.naoOficial')}</b> {t(AVISO_FIXO)}</p>
    </div>
  )
}

/** O "W" neutro, até a revisão do uso da marca (decisão de UI 8). */
const MONOGRAMA = 'W' // textos-ok: monograma neutro da marca (decisão de UI 8), igual em todo idioma
export const MarcaW = () => <span className="wa-marca" aria-hidden="true">{MONOGRAMA}</span>

/** Os 4 estados do contrato de plugins, separados. */
/** `fino` (plugin aberto em Extensões): os quatro numa linha só, em vez de quatro caixas. */
export function QuatroEstados({ estados, nome, fino = false }: { estados: Estados; nome?: string; fino?: boolean }) {
  const t = useT()
  // Um só nó: o título fica num flex com espaçamento, e o nome isolado não pode virar outro item.
  const pode = nome ? <span>{comNos(t('whatsapp.estados.titulo.podeNome', { bot: no('bot') }), { bot: <bdi dir="auto">{nome}</bdi> })}</span> : t('whatsapp.estados.titulo.pode')
  const itens: [string, ReactNode, string, string][] = [
    ['plugin', t('whatsapp.estados.titulo.plugin'), semParametros(t)(estados.plugin), estados.ligado ? 'checkmark' : 'xmark'],
    ['numero', t('whatsapp.estados.titulo.numero'), semParametros(t)(estados.numero), 'phone'],
    ['pode', pode, semParametros(t)(estados.pode), 'lock.shield'],
    ['envio', t('whatsapp.estados.titulo.envio'), semParametros(t)(estados.envio), 'hand.raised'],
  ]
  return (
    <div className="wa-estados-bloco">
      <ol className={fino ? 'wa-estados fino' : 'wa-estados'} aria-label={t('whatsapp.estados.quatroAria')}>
        {itens.map(([id, titulo, valor, icone], i) => (
          <li key={id}>
            <span className="wa-estado-titulo"><span className="wa-estado-n">{i + 1}</span>{titulo}</span>
            <span className="wa-estado-valor"><Icon name={icone} size={12} />{valor}</span>
          </li>
        ))}
      </ol>
      <p className="wa-frase"><Icon name="info.circle" size={13} />{t(FRASE_ESTADOS)}</p>
    </div>
  )
}

/** R1-01 do #587: uma mensagem guardada (D507) na tela; a frase crua do Hub vai isolada, na direção do próprio texto. */
export function TextoDaMensagem({ m }: { m: Mensagem }) {
  const t = useT()
  return 'texto' in m ? <bdi dir="auto">{m.texto}</bdi> : <>{textoDaMensagem(t, m)}</>
}

/** Erro do Hub, claro e com nova tentativa; nunca vira lista vazia. A mensagem é dado (D507), traduzida aqui. */
export function ErroWhatsapp({ mensagem, tentar }: { mensagem: Mensagem; tentar?: () => void }) {
  const t = useT()
  return (
    <div className="wa-erro" role="alert">
      <Icon name="exclamationmark.triangle" size={14} />
      <span dir={'texto' in mensagem ? 'auto' : undefined}>{textoDaMensagem(t, mensagem)}</span>
      {tentar ? <button onClick={tentar}>{t('whatsapp.estados.tentarDeNovo')}</button> : null}
    </div>
  )
}
