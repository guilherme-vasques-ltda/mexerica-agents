import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/troca.ts'

// DON-903: conversa/troca (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 C9a: a linha de troca de modelo e o balão técnico (views/fork/troca-modelo).
  'conversa.troca.linha': 'تم تغيير النموذج من {de} إلى {para}. انتقلت المحادثة إلى النموذج الجديد.',
  'conversa.troca.balao.sessao': 'استأنف النموذج الجديد المحادثة كاملة.',
  'conversa.troca.balao.passagem': 'تلقّى النموذج الجديد الملخص وآخر الطلبات، وليس المحادثة كاملة.',
  'conversa.troca.balao.enviando': 'جارٍ الإرسال…',
  'conversa.troca.balao.reenviados': 'أُعيد إرسال {n} رمز دون الاستفادة مما كان جاهزًا.',
  'conversa.troca.balao.enviados': 'أُرسل {n} رمز دون الاستفادة مما كان جاهزًا.',
}
