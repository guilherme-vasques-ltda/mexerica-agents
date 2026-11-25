// Aviso de instalação só neste computador. Chaves completas; docs/guides/idiomas.md.
import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/avisoLocal.ts'

export const textos: Traducao<typeof fonte> = {
  'avisoLocal.texto': '仅安装在这台电脑上：电脑休眠或关机时，机器人、例行任务和浏览器都会停止。要一直保持运行，请连接你自己的 VPS，或在 Cloud 可用后考虑使用。',
  'avisoLocal.rc2HubLocal': '本地 Hub 已启动：若应用与 Hub 之间没有经过身份验证的配对，就无法保证操作只在你点击之后才会发生。',
}
