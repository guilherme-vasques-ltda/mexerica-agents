import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import { BrowserLiveSession, type BrowserLiveBridge, type LiveState } from '@shared/fork-browser-live'
import type { BrowserScope } from '@shared/fork-browser'
import { ligarPintor } from './pintor'
import { obterSessaoAoVivo } from './sessoes'

export interface AoVivoParams {
  bridge: BrowserLiveBridge
  scope: BrowserScope
  session: string
  target: string
  /** Identifica a conexão: sessões da mesma conexão dividem a inscrição única do Hub; a mesma conexão e sessão retomam a já inscrita. */
  conexao: string
}

export type AvisoAoVivo = 'assumir' | 'clique' | null

/** Liga a sessão ao vivo à tela: estado, desenho do quadro, relógio do quadro velho, assumir e clicar. */
export function useAoVivo({ bridge, scope, session, target, conexao }: AoVivoParams) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const [state, setState] = useState<LiveState | null>(null)
  const [agora, setAgora] = useState(() => Date.now())
  const [aviso, setAviso] = useState<AvisoAoVivo>(null)
  const [, redesenhar] = useReducer((n: number) => n + 1, 0)
  const dados = useMemo(() => ({ bridge, scope, session, target }), [bridge, scope.hubId, scope.spaceId, scope.botId, session, target]) // eslint-disable-line react-hooks/exhaustive-deps
  const sessao = useMemo(() => obterSessaoAoVivo(conexao, dados.session, (ficha) => new BrowserLiveSession(dados.bridge, dados.scope, dados.session, dados.target, undefined, ficha)), [conexao, dados])

  useEffect(() => {
    let vivo = true
    setState(sessao.state)
    const paraEstado = sessao.subscribe({
      onState: (next) => {
        if (vivo) setState(next)
      },
      onFrame: () => {},
    })
    const desligar = ligarPintor(sessao, () => canvas.current, redesenhar)
    return () => {
      vivo = false
      paraEstado()
      desligar()
    }
  }, [sessao])

  const aoVivo = state?.status === 'live'
  useEffect(() => {
    if (!aoVivo) return
    const tick = setInterval(() => setAgora(Date.now()), 1000)
    return () => clearInterval(tick)
  }, [aoVivo])

  const assumir = useCallback(
    async (quem: 'person' | 'agent') => {
      setAviso(null)
      if (!(await sessao.setController(quem)) && sessao.state.status === 'live') setAviso('assumir')
    },
    [sessao],
  )

  const clicar = useCallback(
    async (x: number, y: number, largura: number, altura: number) => {
      setAviso(null)
      const resultado = await sessao.click(x, y, largura, altura)
      if (!resultado.ok && resultado.reason !== 'stale' && resultado.reason !== 'notPerson' && resultado.reason !== 'noFrame') setAviso('clique')
    },
    [sessao],
  )

  return { canvas, state: state ?? sessao.state, idadeMs: aoVivo || state?.status === 'closed' ? sessao.frameAgeMs() : null, agora, aviso, temQuadro: sessao.frame !== null, assumir, clicar }
}
