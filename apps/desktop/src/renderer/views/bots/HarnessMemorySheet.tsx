import { CardForm, CardSection } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ModalHeader, SheetPage } from '../../components/Overlay'
import { HStack, IconSlot, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useT } from '../../lib/fork-i18n/estado'

/**
 * The memory screen's info button beside the harness's own memory switch: what turning it on means. The same text
 * as the kit's; the terminal UI says it short in a confirmation before turning it on. `readable`: Codync shows its
 * notes (Claude Code's are a file).
 */
export function HarnessMemorySheet({ harness, readable }: { harness: string; readable: boolean }) {
  const t = useT()
  const details = [
    { icon: 'brain', title: t('memoria.painel.motor.uma'), detail: t('marca.memoriaMotor.separadas', { motor: harness }) },
    { icon: 'square.on.square', title: t('memoria.painel.motor.duasVezes'), detail: t('memoria.painel.motor.duasVezesDetalhe') },
    { icon: 'trash', title: t('memoria.painel.esquecer'), detail: t('marca.memoriaMotor.esquecer', { motor: harness }) },
    { icon: 'eye', title: t('memoria.painel.motor.ondeVer'), detail: readable ? t('memoria.painel.motor.ondeVerAba', { motor: harness }) : t('marca.memoriaMotor.ondeVer', { motor: harness }) },
  ]
  return (
    <SheetPage>
      <ModalHeader closeTitle={t('memoria.painel.fechar')} icon="brain" title={t('memoria.painel.motor.propria', { motor: harness })} />
      <CardForm>
        <CardSection>
          {details.map((d) => (
            <HStack key={d.title} align="flex-start" gap={12}>
              <IconSlot width={20} tone="secondary" className="memory-info-icon">
                <Icon name={d.icon} size={15} />
              </IconSlot>
              <VStack gap={2} grow>
                <span>{d.title}</span>
                <Text size="footnote" tone="secondary">{d.detail}</Text>
              </VStack>
            </HStack>
          ))}
        </CardSection>
      </CardForm>
    </SheetPage>
  )
}
