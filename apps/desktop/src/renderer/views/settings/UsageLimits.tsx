// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { Usage, UsageProvider } from '@shared/models'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { prefs, usePref } from '../../lib/prefs'
import { useIdioma, useT } from '../../lib/fork-i18n/estado'
import { formatarNumero } from '../../lib/fork-i18n/index'
import { ProviderMascot } from '../usage/ProviderMascot'
import { UsageBar } from '../usage/UsageBar'
import { providerTintCSS, resetDescription, windowTitle } from '../usage/usage-model'
import './settings.css'

/** Every limit as a bar under its provider, the way the menu bar lists them. */
export function UsageLimits({ usage }: { usage: Usage }) {
  const [style] = usePref(prefs.usageIconStyle)
  return (
    <VStack gap={18}>
      {usage.providers.map((provider) => (
        <VStack key={provider.id} gap={10}>
          <HStack gap={8}>
            <ProviderMascot provider={provider} size={18} style={style} />
            <Text size="body" weight="semibold" tone="text">{provider.name}</Text>
          </HStack>
          <UsageWindows provider={provider} />
        </VStack>
      ))}
    </VStack>
  )
}

/** One provider's windows (5 h, week…) as bars; also under an account's row in Ajustes → Uso (MX-145 E2). */
export function UsageWindows({ provider }: { provider: UsageProvider }) {
  const t = useT()
  const idioma = useIdioma()
  // Percentual no formato do idioma (dígitos e sinal) e nome acessível da barra traduzido.
  const pct = (p: number) => formatarNumero(idioma, Math.round(p) / 100, { style: 'percent', maximumFractionDigits: 0 })
  return (
    <>
      {provider.windows.map((w) => {
        const volta = resetDescription(t, idioma, w)
        return (
          <HStack key={w.id} gap={10} wrap className="settings-usage-row">
            <Text tone="text" truncate className="settings-usage-title">{windowTitle(t, w)}</Text>
            <UsageBar percent={w.percent} tint={providerTintCSS(provider.id)} label={t('ajustes.uso.usado', { percent: pct(w.percent) })} />
            <Text tone="text" tabular className="settings-usage-percent">{pct(w.percent)}</Text>
            <Text tone="secondary" className="settings-usage-reset" title={volta}>{volta}</Text>
          </HStack>
        )
      })}
    </>
  )
}
