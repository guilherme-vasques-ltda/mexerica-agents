import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/mailInbox.ts'

export const textos: Traducao<typeof fonte> = {
  "mailInbox.title": "صندوق وارد للقراءة فقط",
  "mailInbox.fixtureNotice": "عرض توضيحي برسائل خيالية؛ لا يتصل بحساب حقيقي.",
  "mailInbox.refresh": "تحديث",
  "mailInbox.loading": "جارٍ التحميل…",
  "mailInbox.notConnected": "لا يوجد صندوق بريد متصل.",
  "mailInbox.reconnect": "يجب التصريح بالاتصال مجددًا.",
  "mailInbox.unavailable": "الصندوق غير متاح الآن.",
  "mailInbox.mailboxes": "صناديق البريد",
  "mailInbox.noMailboxes": "لا يوجد صندوق متاح للقراءة.",
  "mailInbox.empty": "هذا الصندوق فارغ.",
  "mailInbox.noSubject": "بلا موضوع",
  "mailInbox.thread": "المحادثة",
  "mailInbox.chooseThread": "اختر محادثة لقراءتها.",
  "mailInbox.unknownSender": "مرسل غير معروف",
  "mailInbox.declaredDomain": "النطاق المُعلن",
  "mailInbox.truncated": "تم اختصار المحتوى.",
  "mailInbox.loadMore": "تحميل المزيد",
  "mailInbox.error": "تعذر تحميل الصندوق.",
  "mailInbox.contextUnavailable": "اختر مساحة متصلة لفتح صندوق القراءة فقط.",
  "mailInbox.fixtureMailbox": "دعم خيالي",
  "mailInbox.fixtureSubject": "طلب تجريبي",
  "mailInbox.fixtureSender": "شخص تجريبي",
  "mailInbox.fixtureBody": "رسالة خيالية. يجب أن يبقى https://example.invalid/link نصًا غير تفاعلي.",
}
