import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/limites.ts'

// DON-903: conversa/limites (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'conversa.limites.titulo': '使用限额',
  'conversa.limites.retomar': '限额恢复后继续',
  'conversa.limites.retomarDetalhe': '账户限额恢复后，机器人会继续处理中断的请求。',
  'conversa.limites.soneca': '休眠至限额恢复',
  'conversa.limites.sonecaDetalhe': '对话会等到恢复时间，期间不会提前尝试，也不会发送提醒。',
  'conversa.limites.rodape': '适用于此 Hub 上的所有机器人。在每个对话中，“不再继续”只会取消该次等待。',
  'conversa.limites.carregando': '正在读取设置…',
  'conversa.limites.semConexao': '未连接到 Hub。Hub 恢复连接后会显示设置。',
  'conversa.limites.hubAntigo': '请更新 Hub 以更改使用限额。',
  'conversa.limites.falhaLer': '暂时无法读取设置。',
  'conversa.limites.falhaGravar': '无法确认是否已保存。请检查设置。',
  'conversa.limites.tentarDeNovo': '重试',
  'conversa.limites.naoRetomar': '不再继续',
  'conversa.limites.retomarAgora': '立即继续',
  'conversa.limites.falhaCancelar': '无法确认是否已取消。请先查看对话，再重试。',
  'conversa.limites.falhaRetomar': '暂时无法继续。请重试。',
}
