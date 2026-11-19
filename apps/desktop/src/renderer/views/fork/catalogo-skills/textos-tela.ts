// D454 (fatia 8b): os textos da biblioteca de skills no idioma da tela, para os componentes.
import { useMemo } from 'react'
import { useIdioma } from '../../../lib/fork-i18n/estado'
import { textosEm } from './textos'

export function useTextosSkills() {
  const idioma = useIdioma()
  return useMemo(() => textosEm(idioma), [idioma])
}
