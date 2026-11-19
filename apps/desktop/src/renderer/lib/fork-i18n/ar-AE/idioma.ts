import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/idioma.ts'

// DON-903: idioma (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'idioma.nome': 'العربية (الإمارات)',
  'idioma.passo.titulo1': 'كل روبوتاتك،',
  'idioma.passo.titulo2': 'في مكان واحد.',
  'idioma.passo.subtitulo': 'يدير Berga فريق الروبوتات لديك في Mexerica، وينبهك عندما يحتاج أمر ما إلى انتباهك.',
  'idioma.passo.escolha': 'اختر لغتك',
  'idioma.passo.sugerido': 'مقترح بناءً على لغة جهاز Mac',
  'idioma.passo.continuar': 'متابعة',
  'idioma.passo.rodape': 'يمكنك تغيير اللغة لاحقًا من الإعدادات → عام.',
  'idioma.passo.etapa': 'الخطوة {atual} من {total}',
  'idioma.aviso.migracao': 'الترجمة قيد الإنجاز: ما يزال تسجيل الدخول إلى الحساب ومعظم التطبيق باللغة البرتغالية.',
  'idioma.aviso.bots': 'ما تزال الروبوتات تجيب باللغة البرتغالية.',
  'idioma.ajuste.secao': 'اللغة',
  'idioma.ajuste.rotulo': 'لغة التطبيق',
}
