import { useState, type RefObject } from 'react'
import { Button, Disclosure } from '../../../components/Controls'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import type { PropostaSkill } from './rascunho'
import { superficieIdioma, useIdioma, tradutor } from '../../../lib/fork-i18n/estado'

/** Só texto externo passivo; a identidade chega do autor real resolvido no chat. */
export function RecomendacaoSkill({ proposta, nomeAutor, pessoal, ancoraExame, examinar, dispensar }: {
  proposta: Extract<PropostaSkill, { tipo: 'recomendacao' }>
  nomeAutor?: string
  pessoal: boolean
  ancoraExame: RefObject<HTMLDivElement | null>
  examinar: () => void
  dispensar: () => void
}) {
  const [expandida, setExpandida] = useState(false)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const instrucoes = <Text as="div" size="callout" translate="no" className="skill-recommendation-text">{proposta.instructions}</Text>
  return <VStack gap={12} className="skill-recommendation idioma-superficie" {...superficieIdioma(idioma)} role="region" aria-label={t('conversa.skills.recomendacao.aria')}>
    <Text size="footnote" weight="semibold">{nomeAutor?.trim() ? t('conversa.skills.recomendacao.de', { bot: nomeAutor }) : t('conversa.skills.recomendacao.aria')}</Text>
    <VStack gap={4}>
      <Text size="footnote" tone="secondary" weight="semibold">{t('conversa.skills.motivo')}</Text>
      <Text as="div" size="callout" translate="no" className="skill-recommendation-text">{proposta.reason}</Text>
    </VStack>
    <VStack gap={12} className="skill-recommendation-body">
      <Text as="h3" size="callout" weight="semibold" translate="no">{proposta.name}</Text>
      <VStack gap={4}>
        <Text size="footnote" tone="secondary" weight="semibold">{t('conversa.skills.recomendacao.quando')}</Text>
        <Text as="div" size="callout" translate="no" className="skill-recommendation-text">{proposta.description}</Text>
      </VStack>
      {proposta.instructions.length > 800 || proposta.instructions.split('\n').length > 8
        ? <Disclosure expanded={expandida} onToggle={() => setExpandida(!expandida)} label={<Text size="footnote" weight="semibold">{t('conversa.skills.recomendacao.instrucoes')}</Text>}>{instrucoes}</Disclosure>
        : <VStack gap={4}><Text size="footnote" tone="secondary" weight="semibold">{t('conversa.skills.recomendacao.instrucoes')}</Text>{instrucoes}</VStack>}
    </VStack>
    <Text size="footnote" tone="secondary">{t('conversa.skills.recomendacao.nada')}</Text>
    {pessoal ? null : <Text size="footnote" tone="secondary">{t('conversa.skills.soPessoal')}</Text>}
    <HStack gap={8} wrap>
      <div ref={ancoraExame}><Button kind="primary" disabled={!pessoal} onClick={examinar}>{t('conversa.skills.examinarRascunho')}</Button></div>
      <Button kind="secondary" onClick={dispensar}>{t('conversa.skills.agoraNao')}</Button>
    </HStack>
  </VStack>
}
