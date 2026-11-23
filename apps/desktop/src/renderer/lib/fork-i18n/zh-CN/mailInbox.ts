import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/mailInbox.ts'

export const textos: Traducao<typeof fonte> = {
  "mailInbox.title": "只读收件箱",
  "mailInbox.fixtureNotice": "使用虚构邮件演示；不会连接真实账户。",
  "mailInbox.refresh": "刷新",
  "mailInbox.loading": "正在加载…",
  "mailInbox.notConnected": "没有已连接的邮箱。",
  "mailInbox.reconnect": "需要重新授权连接。",
  "mailInbox.unavailable": "邮箱暂时不可用。",
  "mailInbox.mailboxes": "邮箱",
  "mailInbox.noMailboxes": "没有可读取的邮箱。",
  "mailInbox.empty": "此邮箱为空。",
  "mailInbox.noSubject": "无主题",
  "mailInbox.thread": "邮件对话",
  "mailInbox.chooseThread": "选择一封邮件对话进行阅读。",
  "mailInbox.unknownSender": "未知发件人",
  "mailInbox.declaredDomain": "声明的域名",
  "mailInbox.truncated": "内容已截短。",
  "mailInbox.loadMore": "加载更多",
  "mailInbox.error": "无法加载邮箱。",
  "mailInbox.contextUnavailable": "选择已连接的 Space 以打开只读收件箱。",
  "mailInbox.fixtureMailbox": "虚构支持邮箱",
  "mailInbox.fixtureSubject": "示例请求",
  "mailInbox.fixtureSender": "示例联系人",
  "mailInbox.fixtureBody": "虚构邮件。https://example.invalid/link 必须保持为不可交互文本。",
}
