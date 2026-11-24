import { CardSection } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import { WebLink } from '../../marketplace/pieces'

export function AntigravitySetup({ computador }: { computador: string }) {
  const t = useT()
  return <>
    <CardSection title={t('extensoes.motor.antigravityComputador', { computador })}>
      <Text size="subheadline" tone="secondary">{t('extensoes.motor.antigravityInstalacao')}</Text>
      <WebLink title={t('extensoes.motor.antigravityVersao')} url="https://github.com/google-antigravity/antigravity-cli/releases/tag/1.5.15" />
      <WebLink title={t('extensoes.motor.antigravityGuia')} url="https://antigravity.google/download/#antigravity-cli" />
    </CardSection>
    <CardSection title={t('extensoes.motor.entrar')}>
      <Text size="subheadline" tone="secondary">{t('extensoes.motor.antigravityLoginLocal')}</Text>
      <Text size="subheadline" tone="secondary">{t('extensoes.motor.antigravityPermissoes')}</Text>
    </CardSection>
    <CardSection title={t('extensoes.motor.antigravityGemini')}>
      <Text size="subheadline" tone="secondary">{t('extensoes.motor.antigravityGeminiDetalhe')}</Text>
      <WebLink title={t('extensoes.motor.obterChave')} url="https://aistudio.google.com/app/api-keys" />
    </CardSection>
  </>
}
