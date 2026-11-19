// DON-907 C9: os favoritos do seletor de modelo, deste aparelho. Um lugar só, para o compositor e o criar bot:
// os três primeiros são ⌘1–⌘3 nos dois.
import { pref } from '../../../lib/prefs'

const isStringList = (value: unknown): value is string[] => Array.isArray(value) && value.every((v) => typeof v === 'string')

export const seletorPrefs = {
  favoritos: pref<string[]>('modelPickerFavorites', [], isStringList),
}
