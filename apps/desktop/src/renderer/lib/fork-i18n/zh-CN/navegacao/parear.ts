import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/parear.ts'

// Fatia 1 (D454): navegacao/parear (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.parear.qr": "{computador} 的配对码",
  "navegacao.parear.semRede": "未找到网络地址，且“随处访问”已关闭。请连接 Wi-Fi 或开启该选项。",
  "navegacao.parear.mesmaRede": "“随处访问”已关闭，因此 iPhone 只能在同一网络中访问 {computador}。",
  "navegacao.parear.valeAte": "仅可使用一次，有效期至 {hora}",
  "navegacao.parear.novo": "新代码",
  "navegacao.parear.copiar": "复制配对链接",
  "navegacao.parear.semHost": "这台电脑的主机未连接。",
  "navegacao.parear.esteComputador": "这台电脑",
}
