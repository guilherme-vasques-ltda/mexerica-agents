// O reconhecedor devolve sons que não são fala: "[BLANK_AUDIO]", "[música]", "*Loud noise*". Saem do texto antes de
// ele virar mensagem. Parênteses e contas como "2 * 3 * 4" ficam: só vale o que cola nas marcas, sem espaço por dentro.
const COLCHETES = /\[[^\][\n]{1,40}\]/g
const ASTERISCOS = /\*(?=\S)[^*\n]{1,40}?(?<=\S)\*/g

export function limparRuidoDeDitado(texto: string): string {
  return texto.replace(COLCHETES, ' ').replace(ASTERISCOS, ' ').replace(/[ \t]{2,}/g, ' ').trim()
}
