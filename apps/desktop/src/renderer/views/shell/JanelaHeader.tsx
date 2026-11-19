import { useEffect, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { hexOpaco, logoComNome } from '@shared/janela-faixa'
import logoClaro from '../../../../resources/brand/mexerica/mexerica-logo-claro.svg'
import logoEscuro from '../../../../resources/brand/mexerica/mexerica-logo-escuro.svg'
import simbolo from '../../../../resources/brand/mexerica/mexerica-simbolo.svg'
import { acompanharJanelaOculta } from '../../lib/glow-visibility'
import './janela-header.css'

/**
 * MX-122 v3 (dono, 11/11): a faixa do topo da janela, de 29 px e largura inteira. À esquerda, depois dos botões
 * da janela do Mac, o seletor de Space (`children`, no-drag); no centro da janela, a logo plana com nome
 * (89 × 23 px) ou, sem espaço, só a fruta; o resto arrasta a janela, e o duplo clique fica com o sistema.
 * A logo é decoração (`aria-hidden`), nas cores oficiais: nome marrom no claro, creme no escuro.
 */
export function JanelaHeader({ children }: { children?: ReactNode }) {
  const faixa = useRef<HTMLElement>(null)
  const inicio = useRef<HTMLDivElement>(null)
  const [nome, setNome] = useState(true)
  const [telaCheia, setTelaCheia] = useState(false)

  // Nome ou fruta: mede a faixa e o fim do seletor (o nome do Space muda a largura dele).
  useEffect(() => {
    const el = faixa.current
    const esquerda = inicio.current
    if (!el || !esquerda) return
    const medir = () => {
      const caixa = el.getBoundingClientRect()
      setNome(logoComNome(caixa.width, esquerda.getBoundingClientRect().right - caixa.left))
    }
    const ro = new ResizeObserver(medir)
    ro.observe(el)
    ro.observe(esquerda)
    medir()
    return () => ro.disconnect()
  }, [])

  // Tela cheia (Mac): os botões da janela somem e o seletor encosta à esquerda.
  useEffect(() => {
    let vivo = true
    void window.codync.app.isFullScreen().then((on) => vivo && setTelaCheia(on), () => {})
    const parar = window.codync.app.onCommand((c) => c.kind === 'fullScreen' && setTelaCheia(c.on))
    return () => { vivo = false; parar() }
  }, [])

  // MX-1287: o halo sob a faixa pausa com a janela em segundo plano (atributo no root; o CSS pausa).
  useEffect(() => acompanharJanelaOculta(), [])

  // Windows/Linux: os controles do sistema ficam na faixa; pinta-os com o fundo e a tinta da faixa a cada troca
  // de tema, cor de destaque ou fundo de tela.
  useEffect(() => {
    const el = faixa.current
    if (!el || window.codync.platform === 'darwin') return
    let enviado = ''
    const pintar = () => {
      const estilo = getComputedStyle(el)
      const base = getComputedStyle(el.closest('.chat-window') ?? document.body).backgroundColor
      const color = hexOpaco(estilo.backgroundColor, base)
      const symbolColor = hexOpaco(estilo.color, base)
      if (!color || !symbolColor || `${color}${symbolColor}` === enviado) return
      enviado = `${color}${symbolColor}`
      window.codync.app.setWindowControls({ color, symbolColor })
    }
    pintar()
    const mo = new MutationObserver(() => requestAnimationFrame(pintar))
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-wallpaper', 'data-accent', 'style'] })
    return () => mo.disconnect()
  }, [])

  return (
    <>
    <header ref={faixa} className={`janela-header drag${telaCheia ? ' tela-cheia' : ''}`}>
      <div ref={inicio} className="janela-header-inicio">{children}</div>
      <span className={`janela-header-marca${nome ? '' : ' fruta'}`} aria-hidden="true">
        {nome ? (
          <>
            <img className="so-claro" src={logoClaro} alt="" draggable={false} />
            <img className="so-escuro" src={logoEscuro} alt="" draggable={false} />
          </>
        ) : <img src={simbolo} alt="" draggable={false} />}
      </span>
    </header>
    {/* MX-1287 (D2): linha fina de destaque no pé da faixa e halo só abaixo dela, fora da área de arrastar. Vai para o
        <body> porque as telas (Ajustes, Rotinas, Extensões, Times, Torre) são camadas z-40 fora do `.split` isolado e
        escondem o que fica dentro dele; no body o glow aparece sobre elas, sem clique. */}
    {createPortal(
      <div className="janela-glow" aria-hidden="true">
        <span className="janela-glow-linha" />
        <span className="janela-glow-halo" />
      </div>,
      document.body,
    )}
    </>
  )
}
