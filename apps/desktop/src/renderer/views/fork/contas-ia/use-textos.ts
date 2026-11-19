// DON-917 (fatia 9a): os textos das contas de IA no idioma do app; a tela re-renderiza quando ele muda.
import { useIdioma } from '../../../lib/fork-i18n/estado'
import { textosEm, type TextosContas } from './textos'

export function useTextosContas(): TextosContas {
  return textosEm(useIdioma())
}
