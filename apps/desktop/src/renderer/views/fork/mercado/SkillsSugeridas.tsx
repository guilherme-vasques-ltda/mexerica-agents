// As skills que a ficha sugere para o serviço: só o nome e o que ela faz. Skill é texto: não dá acesso a nada e não liga
// sozinha; quem instala e liga é a pessoa, em Extensões → Skills.
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import { extensoesScreen } from '../extensoes/lugar-atual'
import { skillsDaFicha } from './regras.ts'
import type { Servico } from './tipos.ts'

/** `emBreve`: o serviço ainda não conecta; a sugestão fica só como texto, sem o atalho para instalar. */
export function SkillsSugeridas({ servico, emBreve = false }: { servico: Pick<Servico, 'nome' | 'skills'>; emBreve?: boolean }) {
  const t = useT()
  const skills = skillsDaFicha(servico)
  if (skills.length === 0) return null
  return (
    <section className="mercado-secao mercado-skills-sugeridas" aria-label={t('mercado.skills.titulo', { servico: servico.nome })}>
      <Text as="h3" size="callout" weight="semibold" tone="secondary">{t('mercado.skills.titulo', { servico: servico.nome })}</Text>
      <div className="mercado-lista-fina">
        {skills.map((s) => (
          <div key={s.id} className="mercado-linha">
            <Icon name="book.pages" size={12} />
            <VStack gap={0} shrink>
              <Text size="body" weight="medium" tone="text">{s.titulo}</Text>
              <Text size="caption" tone="secondary">{s.resumo}</Text>
            </VStack>
          </div>
        ))}
      </div>
      <Text size="caption" tone="secondary">{emBreve ? t('mercado.skills.notaEmBreve', { servico: servico.nome }) : t('mercado.skills.nota')}</Text>
      {emBreve ? null : (
        <button className="mercado-link" onClick={() => extensoesScreen.setPage('skills')}>
          <HStack gap={4}><Text size="callout" weight="semibold">{t('mercado.skills.ver')}</Text><Icon name="chevron.right" size={10} /></HStack>
        </button>
      )}
    </section>
  )
}
