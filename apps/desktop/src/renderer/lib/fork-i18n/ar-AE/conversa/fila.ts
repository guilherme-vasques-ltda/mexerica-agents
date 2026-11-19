import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/fila.ts'

// DON-903: conversa/fila (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 (D431): a bandeja da fila em cima do campo (views/fork/conversa/BandejaFila.tsx).
  'conversa.fila.titulo': 'في الانتظار',
  'conversa.fila.lista': 'الرسائل المنتظرة',
  'conversa.fila.respondendo': '{nome} يرد الآن…',
  'conversa.fila.atalhos': '⇧Enter يرسل الأولى · ↑ يعدّل الأخيرة',
  'conversa.fila.recolher': 'طي قائمة الانتظار',
  'conversa.fila.expandir': 'إظهار قائمة الانتظار',
  'conversa.fila.mandarAgora': 'أرسل الآن',
  'conversa.fila.editar': 'تعديل',
  'conversa.fila.remover': 'إزالة',
  'conversa.fila.arrastar': 'اسحب لتغيير الترتيب (Alt+↑/↓)',
  'conversa.fila.soAnexos': 'مرفقات فقط',
  'conversa.fila.de': 'من {origem}',
  'conversa.fila.editado': 'معدَّل',
  'conversa.fila.editando': 'تعديل الرسالة رقم {n} في الانتظار',
  'conversa.fila.cancelar': 'إلغاء',
  'conversa.fila.salvar': 'حفظ',
  'conversa.fila.erro.saiu': 'غادرت هذه الرسالة قائمة الانتظار بالفعل.',
  'conversa.fila.erro.mudou': 'تغيّرت قائمة الانتظار في الأثناء. راجعها وحاول مجددًا.',
  'conversa.fila.erro.permissao': 'لم يعد بإمكان هذا الجهاز تغيير قائمة الانتظار.',
  'conversa.fila.erro.pendente': 'لم يؤكد الكمبيوتر في الوقت المناسب. راجع قائمة الانتظار.',
  'conversa.fila.erro.falhou': 'تعذّر تغيير قائمة الانتظار.',
}
