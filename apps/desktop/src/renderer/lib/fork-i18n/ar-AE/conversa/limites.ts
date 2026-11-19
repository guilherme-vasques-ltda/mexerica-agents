import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/limites.ts'

// DON-903: conversa/limites (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'conversa.limites.titulo': 'حدود الاستخدام',
  'conversa.limites.retomar': 'الاستئناف بعد الحد',
  'conversa.limites.retomarDetalhe': 'عند إعادة تعيين حد الحساب، يتابع الروبوت الطلب الذي توقف.',
  'conversa.limites.soneca': 'غفوة حتى إعادة تعيين الحد',
  'conversa.limites.sonecaDetalhe': 'تنتظر المحادثة موعد إعادة التعيين، دون محاولات مسبقة ودون تنبيهات في الأثناء.',
  'conversa.limites.rodape': 'ينطبق على جميع الروبوتات في هذا Hub. في كل محادثة، يلغي «عدم الاستئناف» هذا الانتظار فقط.',
  'conversa.limites.carregando': 'جارٍ قراءة الإعدادات…',
  'conversa.limites.semConexao': 'لا يوجد اتصال بـ Hub. ستظهر الإعدادات عند عودته.',
  'conversa.limites.hubAntigo': 'حدّث Hub لتغيير حدود الاستخدام.',
  'conversa.limites.falhaLer': 'تعذّرت قراءة الإعدادات الآن.',
  'conversa.limites.falhaGravar': 'تعذّر تأكيد الحفظ. راجع الإعدادات.',
  'conversa.limites.tentarDeNovo': 'إعادة المحاولة',
  'conversa.limites.naoRetomar': 'عدم الاستئناف',
  'conversa.limites.retomarAgora': 'الاستئناف الآن',
  'conversa.limites.falhaCancelar': 'تعذّر تأكيد الإلغاء. راجع المحادثة قبل المحاولة مرة أخرى.',
  'conversa.limites.falhaRetomar': 'تعذّر الاستئناف الآن. حاول مرة أخرى.',
}
