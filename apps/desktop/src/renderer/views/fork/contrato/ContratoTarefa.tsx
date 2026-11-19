// T2a (DON-912): o contrato que o bot propôs ao delegar, no cartão do pedido entre bots. Só leitura: o dono vê,
// não aprova. O texto é do bot e entra como texto (nunca HTML); os rótulos vêm do catálogo de idiomas.
import type { TaskContract } from '../../../../shared/models.ts'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { camposDoContrato } from './campos'
import './contrato.css'

export function ContratoTarefa({ contrato, aberto = false }: { contrato: TaskContract; aberto?: boolean }) {
  const t = useT()
  const idioma = useIdioma()
  return (
    <details className="contrato-tarefa idioma-superficie" open={aberto} {...superficieIdioma(idioma)}>
      <summary className="contrato-titulo">{t('contrato.titulo')}</summary>
      <dl className="contrato-campos">
        {camposDoContrato(contrato).map(({ chave, rotulo, texto }) => (
          <div key={chave} className="contrato-campo">
            <dt>{t(rotulo)}</dt>
            <dd className="selectable" dir="auto">{texto}</dd>
          </div>
        ))}
      </dl>
      <p className="contrato-nota">{t('contrato.nota')}</p>
    </details>
  )
}
