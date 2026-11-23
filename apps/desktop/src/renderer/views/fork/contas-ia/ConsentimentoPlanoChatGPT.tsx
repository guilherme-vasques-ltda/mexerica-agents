import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { traduzir } from '../../../lib/fork-i18n/index'
import { direcaoDe, type AppLanguage } from '../../../../shared/fork-idioma'

const AJUSTES_DE_USO = 'https://chatgpt.com/settings/usage'

export function ConsentimentoPlanoChatGPT({ idioma }: { idioma: AppLanguage }) {
  const rotulo = traduzir(idioma, 'contas.chatgpt.consentimento.rotulo')
  return (
    <VStack gap={8} lang={idioma} dir={direcaoDe(idioma)}>
      <HStack gap={8} wrap>
        <input type="checkbox" role="switch" aria-label={rotulo} aria-checked={false} checked={false} readOnly disabled />
        <Text tone="text">{rotulo}</Text>
      </HStack>
      <Text tone="text">{traduzir(idioma, 'contas.chatgpt.consentimento.indisponivel')}</Text>
      <Text tone="text">{traduzir(idioma, 'contas.chatgpt.consentimento.creditos')}</Text>
      <Text tone="text">{traduzir(idioma, 'contas.chatgpt.consentimento.limite')}</Text>
      <Text tone="text">{traduzir(idioma, 'contas.chatgpt.consentimento.uso')}{' · '}<bdi dir="ltr">{AJUSTES_DE_USO}</bdi></Text>
    </VStack>
  )
}
