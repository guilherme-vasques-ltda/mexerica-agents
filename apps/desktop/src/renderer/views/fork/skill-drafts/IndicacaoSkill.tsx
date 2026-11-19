import { useState, type RefObject } from 'react'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { Button } from '../../../components/Controls'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { PaginaSkills } from '../catalogo-skills/PaginaSkills'
import type { InspecaoSkills } from '../catalogo-skills/catalogo-store'
import type { IndicacaoSkill as Indicacao } from './rascunho'
import { superficieIdioma, useIdioma, useT, tradutor } from '../../../lib/fork-i18n/estado'

/** A referência externa é pista de busca, nunca identidade ou metadado canônico. */
export function IndicacaoSkill({ proposta, nomeAutor, pessoal, ancoraExame, examinar, dispensar }: {
  proposta: Indicacao
  nomeAutor?: string
  pessoal: boolean
  ancoraExame: RefObject<HTMLDivElement | null>
  examinar: () => void
  dispensar: () => void
}) {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  return <VStack gap={12} className="skill-recommendation idioma-superficie" {...superficieIdioma(idioma)} role="region" aria-label={t('conversa.skills.indicacao.aria')}>
    <Text size="footnote" weight="semibold">{nomeAutor?.trim() ? t('conversa.skills.indicacao.de', { bot: nomeAutor }) : t('conversa.skills.indicacao.aria')}</Text>
    <VStack gap={4}>
      <Text size="footnote" tone="secondary" weight="semibold">{t('conversa.skills.motivo')}</Text>
      <Text as="div" size="callout" translate="no" className="skill-recommendation-text">{proposta.reason}</Text>
    </VStack>
    <VStack gap={4} className="skill-recommendation-body">
      <Text size="footnote" tone="secondary" weight="semibold">{t('conversa.skills.indicacao.referencia')}</Text>
      <Text as="div" size="callout" translate="no" className="skill-recommendation-text">{proposta.reference}</Text>
    </VStack>
    <Text size="footnote" tone="secondary">{t('conversa.skills.indicacao.pista')}</Text>
    {pessoal ? null : <Text size="footnote" tone="secondary">{t('conversa.skills.indicacao.soPessoal')}</Text>}
    <HStack gap={8} wrap>
      <div ref={ancoraExame}><Button kind="primary" disabled={!pessoal} onClick={examinar}>{t('conversa.skills.indicacao.examinar')}</Button></div>
      <Button kind="secondary" onClick={dispensar}>{t('conversa.skills.agoraNao')}</Button>
    </HStack>
  </VStack>
}

/** Uma busca independente do chat; reutiliza integralmente a página e seus filtros. */
export function InspecaoDaConversa({ referencia, inspecao }: { referencia: string; inspecao: InspecaoSkills }) {
  const [escopo] = useState(inspecao)
  const [busca, setBusca] = useState(referencia)
  const t = useT()
  return <SheetPage><ModalHeader closeTitle={t('conversa.fechar')} icon="book.pages" title={t('conversa.skills.indicacao.examinar')} /><div className="skill-reference-page"><PaginaSkills busca={busca} onBusca={setBusca} inspecao={escopo} /></div></SheetPage>
}
