import type { MemoryNature as Nature, MemoryVersion } from '@shared/models'
import { ltr } from '@shared/bidi'
import type { RotuloAutor } from '../../../store/memory-store'
import { textoDaMensagem, type Mensagem } from '../bot-settings/mensagem'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { comNos, no } from '../../settings/rotulo'
import { MemoryOrigin } from './MemoryOrigin'
import { dataMemoria, natureLabel, nomeQueInferiu } from './presentation'

export function MemoryNature({ nature, bot, name }: { nature?: Nature; bot?: string | null; name: (id: string) => string | undefined }) {
  const t = useT()
  const nome = bot ? name(bot) : undefined
  // `confirmed` (2b1d-2): o selo próprio, nunca "Você disse". O nome do bot fica isolado dentro da frase.
  const label = nature === 'confirmed' ? t('memoria.natureza.confirmada')
    : nature === 'inferred' && nome ? comNos(t('memoria.auto.natureza.inferiu', { bot: no('bot') }), { bot: <bdi dir="auto">{nomeQueInferiu(t, nome)}</bdi> })
      : natureLabel(t, nature, nome)
  return <span className={`memory-nature ${nature === 'conflict' ? 'conflict' : ''}`}>{label}</span>
}

export function MemoryVersionMeta({ at, rev }: { at?: number; rev?: number }) {
  const t = useT()
  const idioma = useIdioma()
  return <small>{at && at > 0 ? <bdi>{dataMemoria(idioma, at)}</bdi> : t('memoria.auto.versao.semData')} · {rev && rev > 0 ? t('memoria.auto.versao.numero', { n: ltr(rev) }) : t('memoria.auto.versao.semNumero')}</small>
}

/** Um texto do usuário ou do bot marcado como hipótese, anterior ou alternativa: a frase inteira, o texto isolado. */
export function TextoMarcado({ chave, texto }: { chave: 'memoria.auto.hipotese' | 'memoria.auto.anterior' | 'memoria.auto.conflito'; texto: string }) {
  const t = useT()
  return <>{comNos(t(chave, { texto: no('texto') }), { texto: <bdi dir="auto">{texto}</bdi> })}</>
}

export function MemoryPrevious({ version, name }: { version?: MemoryVersion | null; name: (id: string) => string | undefined }) {
  const t = useT()
  if (!version) return null
  return <details className="memory-previous"><summary>{t('memoria.auto.versao.anterior')}</summary>
    <MemoryVersionMeta at={version.at} rev={version.rev} />
    <MemoryNature nature={version.nature} bot={version.natureBot} name={name} />
    <MemoryOrigin origin={version.origin} />
    {version.text !== undefined && <p className="selectable">{version.nature === 'inferred' ? <TextoMarcado chave="memoria.auto.hipotese" texto={version.text} /> : <bdi dir="auto">{version.text}</bdi>}</p>}
    {version.conflictText && <p className="selectable"><TextoMarcado chave="memoria.auto.conflito" texto={version.conflictText} /></p>}
  </details>
}

/** Quem escreveu: a frase inteira do catálogo, com o nome do bot isolado. */
export function Autor({ rotulo }: { rotulo: RotuloAutor }) {
  const t = useT() as unknown as (chave: RotuloAutor['chave'], params?: Record<string, string>) => string
  return <>{rotulo.bot === undefined ? t(rotulo.chave) : comNos(t(rotulo.chave, { bot: no('bot') }), { bot: <bdi dir="auto">{rotulo.bot}</bdi> })}</>
}

/** O erro da memória: a chave traduzida agora, ou o texto cru do Hub isolado na própria direção. */
export function ErroDaMemoria({ m }: { m: Mensagem }) {
  const t = useT()
  return 'texto' in m ? <bdi dir="auto">{m.texto}</bdi> : <>{textoDaMensagem(t, m)}</>
}
