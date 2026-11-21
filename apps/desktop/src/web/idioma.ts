// Idioma da web local: a escolha salva da pessoa (`appLanguage`, gravada pelo seletor do
// portão) e, sem escolha, o idioma do navegador que o app tem (`suggestedLanguage` da ponte);
// sem nenhum, o padrão do app (inglês). A sugestão nunca vira escolha gravada.
import type { CodyncBridge } from '../shared/ipc.ts'
import { definirIdiomaDePartida } from '../renderer/lib/fork-i18n/estado.ts'

/** Chamar logo depois de instalar a ponte e antes do portão e do renderer (`main.ts`). */
export function instalarIdiomaWeb(ponte: Pick<CodyncBridge, 'suggestedLanguage'>): void {
  definirIdiomaDePartida(() => ponte.suggestedLanguage)
}
