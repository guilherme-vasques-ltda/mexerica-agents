import { useId, useState } from 'react'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'

/**
 * Auditoria D249, F10: o rodapé de um cartão de Ajustes em uma frase; o texto inteiro, que continua
 * valendo, fica atrás de "Saiba mais" (o mesmo par "Saiba mais"/"Mostrar menos" de Rotinas, E4).
 */
export function RodapeCurto({ resumo, completo }: { resumo: string; completo: string }) {
  const t = useT()
  const idioma = useIdioma()
  const [aberto, setAberto] = useState(false)
  const id = useId()
  return <span className="rodape-curto">
    <span id={id}>{aberto ? completo : resumo}</span>{' '}
    <button type="button" className="rodape-curto-mais idioma-superficie" {...superficieIdioma(idioma)} aria-expanded={aberto} aria-controls={id} onClick={() => setAberto((a) => !a)}>
      {aberto ? t('ajustes.rodape.menos') : t('ajustes.rodape.mais')}
    </button>
  </span>
}
