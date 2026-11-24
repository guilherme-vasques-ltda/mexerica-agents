import { Icon } from '../../../components/Icon'
import { HStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'

export function AvisoInstalacaoLocal() {
  const t = useT()
  return (
    <HStack align="flex-start" gap={6} className="aviso-instalacao-local">
      <Icon name="desktopcomputer" size={12} color="var(--ink-3)" />
      <Text as="div" size="caption" tone="secondary">{t('avisoLocal.texto')}</Text>
    </HStack>
  )
}
