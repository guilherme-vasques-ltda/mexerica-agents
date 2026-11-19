// Skills: acabamento (proposta visual de 10/11/2026). Três peças pequenas sobre o que já existe:
// (a) a confirmação de remover diz QUAIS bots perdem a skill, antes do gesto (o Hub já desliga na mesma operação);
// (b) "Verificar agora" na ficha: leitura de agora, parcial, com origem, versão (digest) e estado;
// (c) último uso (data e contagem), só quando o Hub mandar `usage` — hoje não manda (precisa do Hub).
import { useState } from 'react'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import type { SkillReceipt, SkillUsage } from '@shared/fork-skills'
import { isGroup } from '@shared/models'
import { Button, CardSection, Spinner, ValueRow } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { Selo } from './partes'
import type { EstadoConferencia } from './resumo-informativo'

/** Os bots com a skill ligada agora, como o app os vê (a remoção no Hub desliga de todos, vistos aqui ou não). */
export function botsQueUsam(store: BotStore, skillId: string) {
  return [...store.bots.values()].filter((b) => !isGroup(b) && b.skills.includes(skillId))
}

/** O corpo da confirmação de remover: quem perde a skill e o que acontece com o texto. */
export function AvisoDeRemocao({ store, skillId, doCatalogo }: { store: BotStore; skillId: string; /** Com recibo do catálogo: dá para instalar de novo. */ doCatalogo: boolean }) {
  useModel(store)
  const t = useT()
  const bots = botsQueUsam(store, skillId)
  return (
    <div className="cs-remocao">
      {bots.length ? (
        <>
          <span>{t.plural('skills.acab.removerUsam', bots.length)}</span>
          <ul className="cs-remocao-bots">
            {bots.map((b) => (
              <li key={b.id}>
                <DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(b), b.name, b.description)} size={22} label="" animated={false} />
                <bdi dir="auto">{b.name}</bdi>
              </li>
            ))}
          </ul>
        </>
      ) : <span>{t('skills.acab.removerNinguem')}</span>}
      <span>{t(doCatalogo ? 'skills.acab.removerRestoCatalogo' : 'skills.acab.removerRestoManual')}</span>
    </div>
  )
}

const curto = (hex: string) => (hex.length > 16 ? `${hex.slice(0, 12)}…` : hex)

/**
 * "Verificar agora": relê a instalada no Hub e mostra o que ele respondeu. Só leitura e parcial: compara o conteúdo
 * guardado com o recibo; não analisa segurança e não aprova o que já foi usado.
 */
export function ConferenciaSkill({ estado, recibo, onVerificar, livre }: {
  /** MX-37: o estado já vem da folha com as guardas de vigência (`estadoDaConferencia`); aqui não se presume positivo. */
  estado: EstadoConferencia
  recibo: SkillReceipt | null
  /** Relê a lista e os detalhes (o "Ler de novo" da ficha); `true` só com a releitura válida na origem da folha. */
  onVerificar: () => Promise<boolean>
  livre: boolean
}) {
  const t = useT()
  const idioma = useIdioma()
  const [lendo, setLendo] = useState(false)
  const [conferidoEm, setConferidoEm] = useState<number | null>(null)
  const verificar = async () => {
    setLendo(true)
    // Recusa, falha ou troca de conexão não ganham hora de conferência.
    try { setConferidoEm(await onVerificar() ? Date.now() : null) } catch { setConferidoEm(null) } finally { setLendo(false) }
  }
  // A hora é da releitura pedida aqui, não do conteúdo; uma leitura anterior não a exibe.
  const hora = conferidoEm !== null && estado !== 'anterior' ? conferidoEm : null
  return (
    <CardSection title={t('skills.acab.conferencia')} footer={t('skills.acab.rodape')}
      accessory={<Button kind="secondary" disabled={lendo || !livre} onClick={() => { void verificar() }}>{lendo ? <Spinner /> : null}{t(lendo ? 'skills.acab.verificando' : 'skills.acab.verificar')}</Button>}>
      {[
        <ValueRow key="estado" label={t('skills.acab.estado')}>
          <Selo tom={estado === 'verified' ? 'ok' : estado === 'semRecibo' || estado === 'anterior' ? 'neutro' : 'aviso'}>{t(`skills.acab.estado.${estado}`)}</Selo>
        </ValueRow>,
        <ValueRow key="origem" label={t('skills.acab.origem')}>
          {recibo ? <Text tone="secondary" truncate className="selectable bidi-isolado" dir="ltr" title={recibo.origin.repo}>{recibo.origin.repo}</Text> : <Text tone="secondary">{t('skills.acab.origemManual')}</Text>}
        </ValueRow>,
        recibo ? <ValueRow key="versao" label={t('skills.detalhes.versaoDigest')}><Text tone="secondary" mono className="selectable bidi-isolado" dir="ltr" title={recibo.digest}>{curto(recibo.digest)}</Text></ValueRow> : null,
        <ValueRow key="quando" label={t('skills.acab.conferido')}>
          <Text tone="secondary" role="status">{hora !== null ? t('skills.acab.conferidoAs', { hora: new Intl.DateTimeFormat(idioma, { timeStyle: 'short' }).format(hora) }) : t('skills.acab.naoConferido')}</Text>
        </ValueRow>,
      ]}
    </CardSection>
  )
}

/** A data do último uso, curta: hoje, ontem ou o dia. */
function quando(idioma: string, em: number, agora = Date.now()) {
  const dias = Math.round((new Date(agora).setHours(0, 0, 0, 0) - new Date(em).setHours(0, 0, 0, 0)) / 86_400_000)
  if (dias <= 1) return new Intl.RelativeTimeFormat(idioma, { numeric: 'auto' }).format(-dias, 'day')
  return new Intl.DateTimeFormat(idioma, { day: 'numeric', month: 'short' }).format(em)
}

/** A terceira linha de uma instalada: último uso e contagem. Sem o campo do Hub, nada aparece. */
export function LinhaDeUso({ uso }: { uso: SkillUsage | null | undefined }) {
  const t = useT()
  const idioma = useIdioma()
  if (!uso) return null
  return (
    <Text size="footnote" tone="tertiary" className="cs-uso">
      {uso.lastUsedAt === null ? t('skills.acab.nuncaUsada') : `${t('skills.acab.ultimoUsoQuando', { quando: quando(idioma, uso.lastUsedAt) })} · ${t.plural('skills.acab.vezes', uso.count)}`}
    </Text>
  )
}

/** O uso na ficha: data com hora e contagem; o rodapé diz o que NÃO é guardado. */
export function UsoDaSkill({ uso }: { uso: SkillUsage | null | undefined }) {
  const t = useT()
  const idioma = useIdioma()
  if (!uso) return null
  return (
    <CardSection title={t('skills.acab.uso')} footer={t('skills.acab.usoRodape')}>
      {[
        <ValueRow key="ultimo" label={t('skills.acab.ultimoUso')}>
          <Text tone="secondary">{uso.lastUsedAt === null ? t('skills.acab.nuncaUsada') : new Intl.DateTimeFormat(idioma, { dateStyle: 'medium', timeStyle: 'short' }).format(uso.lastUsedAt)}</Text>
        </ValueRow>,
        <ValueRow key="vezes" label={t('skills.acab.contagem')}><Text tone="secondary" tabular>{t.plural('skills.acab.vezes', uso.count)}</Text></ValueRow>,
      ]}
    </CardSection>
  )
}
