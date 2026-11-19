// voz D416 A6 (D71): o texto que vai para a voz perde o que só serve na tela. Só a cópia falada muda;
// a mensagem do chat, a Entry e o store ficam intactos. Gancho em `call/spoken-text.ts`.

/** Uma palavra (`main`) ou um nome próprio (`Mexerica`) entre crases fica; comando e código inline saem. */
const PALAVRA_SIMPLES = /^(?:\p{L}[\p{L}'-]{0,30}|\p{Lu}\p{Ll}+(?: \p{Lu}\p{Ll}+){1,3})$/u

/** Antes do Markdown: blocos de código e código inline técnico saem. */
export function antesDaFala(markdown: string): string {
  return markdown
    .replace(/```[\s\S]*?```/g, '')
    .replace(/[ \t]+/g, ' ') // D416-R2: sem sequências longas de espaços, nenhuma regra seguinte fica quadrática
    .replace(/`([^`\n]*)`/g, (_, c: string) => (PALAVRA_SIMPLES.test(c.trim()) ? c.trim() : ''))
}

/** Caractere de um componente de caminho, inclusive com acento (relatório.pdf). */
const C = String.raw`[\p{L}\p{N}_.@+-]`
/** Nunca começa no meio de um nome: evita reescanear cada posição de um texto longo (D416-R2). */
const INICIO = String.raw`(?<![\p{L}\p{N}_.@+\-/:~])`
/** Pontos finais do trecho, contados de trás para a frente (`/\.+$/` seria quadrático com muitos pontos). */
function pontosFinais(m: string) {
  let i = m.length
  while (i > 0 && m[i - 1] === '.') i--
  return i
}
/** Caminho apagado sem levar junto o ponto final da frase. */
const caminho = (m: string) => m.slice(pontosFinais(m))
/** Relativo só sai se terminar em extensão de arquivo (docs/a.md): preço, agenda e datas com barras ficam. */
const relativo = (m: string) => (/\.[a-z][a-z0-9]{0,4}$/.test(m.slice(0, pontosFinais(m)).slice(-6)) ? caminho(m) : m)

const REGRAS: [RegExp, string | ((m: string) => string)][] = [
  [/<(?:https?|ftp|file):\/\/[^>\s]*>/gi, ''], // autolinks
  [/\b(?:https?|ftp|file):\/\/[^\s<>]*[^\s<>.,;:!?)\]'"]/gi, ''], // URLs soltas
  [/\bwww\.[^\s<>]*[^\s<>.,;:!?)\]'"]/gi, ''],
  [/\b[A-Za-z]:\\[^\s]*[^\s.,;:!?)]/g, ''], // C:\pasta\arquivo
  [/\\\\[^\s\\]+\\[^\s]*[^\s.,;:!?)]/g, ''], // \\servidor\pasta
  // /a/b, ~/a, ./a, ../a e /tmp (um componente começando por letra); não pega e/ou nem 24/7.
  [new RegExp(String.raw`${INICIO}(?:(?:~|\.{1,2})\/${C}*|\/\p{L}${C}*)(?:\/${C}+)*\/?`, 'gu'), caminho],
  [new RegExp(String.raw`${INICIO}${C}+(?:\/${C}+)+`, 'gu'), relativo],
  [/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi, ''], // UUID
  [/\b(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{7,}\b/gi, ''], // hash (tem letra a-f e dígito)
  [/\b[a-z]{2,12}[_-](?=[A-Za-z0-9]*\d)[A-Za-z0-9]{6,}\b/g, ''], // thr_abc123, msg-01H9…
  [/(?<![\w-])(?=[\w-]*\d)(?=[\w-]*[A-Za-z])[\w-]{20,}(?![\w-])/g, ''], // tokens longos (só no início: linear)
  [/\b(\d{4}-\d{2}-\d{2})T(\d{1,2}:\d{2}):\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?/g, '$1 $2'], // ISO
  [/\b(\d{1,2}:\d{2}):\d{2}(?:[.,]\d+)?\b/g, '$1'], // 14:32:05 → 14:32
]

/** Depois do Markdown: URLs, caminhos, IDs, hashes e segundos saem; arruma espaços e pontuação. */
export function depoisDaFala(texto: string): string {
  let t = texto
  for (const [padrao, troca] of REGRAS) t = t.replace(padrao, troca as string)
  return t
    .replace(/\((?:[\s,;:]|sha\b|id\b)*\)/gi, '') // parênteses vazios (D416-R2: sem quantificadores sobrepostos)
    .replace(/[ \t]+([.,;:!?])/g, '$1')
    .replace(/([,;:])(?:[ \t]*[,;:])+/g, '$1')
    .replace(/[ \t]{2,}/g, ' ')
    .replace(/[ \t]+$/gm, '')
    .trim()
}
