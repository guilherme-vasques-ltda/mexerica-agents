import { CardSection, ValueRow } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import type { ResumoDaInstalacao } from './resumo-informativo.ts'
import type { SufixoSemParams } from './textos.ts'

/**
 * Apenas dados permitidos; nenhum DTO bruto, callback, efeito ou consulta. A conferência do texto é da
 * `ConferenciaSkill` e a medição real de uso, da `UsoDaSkill`: aqui não há selo nem segundo resumo de uso.
 * `usoMedido`: a seção Uso já mostra a medição vigente desta instalação, então a grade omite a célula.
 */
export function ResumoInformativo({ resumo, idioma, usoMedido }: { resumo: ResumoDaInstalacao; idioma: AppLanguage; usoMedido: boolean }) {
  const L = (chave: `skills.informativo.${SufixoSemParams<'skills.informativo.'>}`) => traduzir(idioma, chave)
  const medidas: { campo: 'atualizacao' | 'anuncio' | 'entrega' | 'uso'; valor: 'desconhecida' | 'naoMedido' | 'desconhecido' }[] = [
    { campo: 'atualizacao', valor: 'desconhecida' },
    { campo: 'anuncio', valor: 'naoMedido' },
    { campo: 'entrega', valor: 'naoMedido' },
  ]
  // Sem medição só quando a leitura é desta conexão; antes disso, não se sabe.
  if (!usoMedido) medidas.push({ campo: 'uso', valor: resumo.vigente ? 'naoMedido' : 'desconhecido' })
  return (
    <>
      <CardSection title={L('skills.informativo.titulo')} footer={L('skills.informativo.enderecoNota')}>
        <ValueRow label={L('skills.informativo.gestao')}><Text tone="secondary">{L('skills.informativo.pessoal')}</Text></ValueRow>
        <ValueRow label={L('skills.informativo.origem')}><Text tone="secondary">{L(resumo.origem ? `skills.informativo.${resumo.origem}` : 'skills.informativo.indisponivel')}</Text></ValueRow>
        <ValueRow label={L('skills.informativo.endereco')}><Text mono tone="secondary" className="cs-info-endereco selectable bidi-isolado" dir="ltr">{resumo.endereco ?? L('skills.informativo.indisponivel')}</Text></ValueRow>
      </CardSection>
      <CardSection title={L('skills.informativo.datas')} footer={L('skills.informativo.rodape')}>
        <div className="cs-info-medidas">
          {medidas.map(({ campo, valor }) => (
            <div key={campo} className="cs-info-medida">
              <Text size="footnote" tone="secondary">{L(`skills.informativo.${campo}`)}</Text>
              <Text size="callout">{L(`skills.informativo.${valor}`)}</Text>
            </div>
          ))}
        </div>
        <Text size="footnote" tone="secondary">{L('skills.informativo.usoNota')}</Text>
      </CardSection>
    </>
  )
}
