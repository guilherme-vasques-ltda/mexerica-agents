// Aviso de instalação só neste computador. Chaves completas; docs/guides/idiomas.md.
import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/avisoLocal.ts'

export const textos: Traducao<typeof fonte> = {
  'avisoLocal.texto': 'مثبّت على هذا الكمبيوتر فقط: إذا دخل في وضع السكون أو أُغلق، تتوقف البوتات والمهام الروتينية والمتصفح. للبقاء متاحًا دائمًا، اربط خادم VPS خاصًا بك، أو فكّر في Cloud عندما تصبح متاحة.',
}
