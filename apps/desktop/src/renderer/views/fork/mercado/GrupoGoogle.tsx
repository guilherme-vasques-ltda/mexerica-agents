// O grupo "Google" do catálogo (tela 01): um cartão só para Gmail, Agenda, Drive, Docs e Planilhas, que entram com o app
// OAuth da própria pessoa. Só renderiza o que o Hub mandou; abrir o fluxo é com a página.
import { Button } from '../../../components/Controls'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import type { AppGoogle, Conta, Servico } from './tipos.ts'
import { LogoServico, PontoDeEstado } from './pecas'

export function GrupoGoogle({ servicos, app, contas, cofreTrancado, aoAbrir }: {
  servicos: readonly Servico[]
  /** O app Google do Space (só o ID); `null` = ainda sem app ou cofre trancado. */
  app: AppGoogle | null
  contas: readonly Conta[]
  cofreTrancado: boolean
  aoAbrir: () => void
}) {
  const t = useT()
  const ids = new Set(servicos.map((s) => s.id))
  const conectadas = contas.filter((c) => c.servico !== null && ids.has(c.servico))
  const expirada = conectadas.some((c) => c.auth === 'signedOut')
  return (
    <section className="mercado-destaque mercado-google" aria-label={t('google.grupo.titulo')}>
      <div className="mercado-google-logos" aria-hidden>
        {servicos.map((s) => <LogoServico key={s.id} nome={s.nome} marca={s.marca} size={34} />)}
      </div>
      <VStack gap={4} grow shrink>
        <HStack gap={8}>
          <Text as="h3" size="title3" weight="semibold" tone="text">{t('google.grupo.titulo')}</Text>
          <Text size="caption" weight="semibold" className="mercado-etiqueta neutra">{servicos.map((s) => s.nome.replace(/^Google /, '')).join(' · ')}</Text>
        </HStack>
        <Text size="body" tone="secondary" lines={2}>{t('google.grupo.sub')}</Text>
        <Text size="callout" tone="tertiary">{t('google.grupo.contas')}</Text>
      </VStack>
      {conectadas.length > 0 ? (
        <span className="mercado-card-fim">
          <PontoDeEstado alerta={expirada} />
          <Text size="callout" tone={expirada ? 'attention' : 'secondary'}>{t.plural('mercado.contas.n', conectadas.length)}</Text>
        </span>
      ) : null}
      <Button onClick={aoAbrir} disabled={cofreTrancado}>{t(app?.configurado ? 'google.grupo.escolher' : 'google.grupo.conectar')}</Button>
    </section>
  )
}
