import type { BrowserLiveSession } from '../../../../shared/fork-browser-live.ts'
import type { BrowserFrame } from '../../../../shared/relay-protocol/browser-frames.ts'
import { desenhar } from './desenhar.ts'

type Criar = Parameters<typeof desenhar>[3]
type Tela = Parameters<typeof desenhar>[0]

/**
 * Liga a sessão ao canvas: cada quadro é desenhado e só então confirmado à sessão, com o resultado real
 * (a entrada e o relógio de quadro velho valem para o que foi pintado). Quando a sessão fecha (revogação,
 * queda, erro) o canvas é limpo na hora e qualquer desenho em andamento é descartado sem tocar nele.
 * Desligar (painel fechado) pausa a sessão: o consumidor local sai e libera o slot de quadros da conexão.
 */
export function ligarPintor(sessao: BrowserLiveSession, canvas: () => Tela | null, aoPintar: () => void, criar?: Criar): () => void {
  let vivo = true
  const encerrada = () => !vivo || sessao.state.status === 'closed'
  const limpar = () => {
    const alvo = canvas()
    if (!alvo) return
    alvo.width = 0
    alvo.height = 0
  }
  const pintar = async (frame: BrowserFrame) => {
    const alvo = canvas()
    if (!alvo) return sessao.frameShown(frame, false)
    const ok = await desenhar(alvo, frame, () => !encerrada(), criar)
    if (encerrada()) return
    if (!ok) limpar()
    sessao.frameShown(frame, ok)
    aoPintar()
  }
  const parar = sessao.subscribe({
    onState: (next) => {
      if (next.status === 'closed') limpar()
    },
    onFrame: (frame) => void pintar(frame),
  })
  void sessao.resume()
  void sessao.start()
  return () => {
    vivo = false
    parar()
    sessao.pause()
  }
}
