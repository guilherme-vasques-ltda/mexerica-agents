// Aviso de instalação só neste computador. Chaves completas; docs/guides/idiomas.md.
import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/avisoLocal.ts'

export const textos: Traducao<typeof fonte> = {
  'avisoLocal.rc2Titulo': 'Mexerica RC 2',
  'avisoLocal.rc2Inativa': 'هذا إصدار تجريبي. لم يتم تفعيل Hub المحلي بعد.',
  'avisoLocal.rc2Verificacao': 'تعذّر التحقق من هذا التثبيت. لم يتم تشغيل Hub المحلي.',
  'avisoLocal.texto': 'مثبّت على هذا الكمبيوتر فقط: إذا دخل في وضع السكون أو أُغلق، تتوقف البوتات والمهام الروتينية والمتصفح. للبقاء متاحًا دائمًا، اربط خادم VPS خاصًا بك، أو فكّر في Cloud عندما تصبح متاحة.',
  'avisoLocal.rc2HubLocal': 'المركز المحلي مُفعّل: بدون زوج مُصادَق بين التطبيق والمركز، لا ضمان أن يحدث أي شيء فقط بعد نقرتك.',
}
