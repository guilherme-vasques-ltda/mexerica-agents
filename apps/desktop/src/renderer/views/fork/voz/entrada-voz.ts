import { useEffect, useRef, useState } from 'react'
import { decidirEntradaVoz, type ContextoEntradaVoz, type MotivoRecusaVoz } from '../../../../shared/fork-voz-entrada.ts'
import type { ResultadoAtalhoVoz } from '../../../../shared/fork-voz.ts'
import { Observable } from '../../../lib/observable'
import { pref, usePref } from '../../../lib/prefs'
import { settingsScreen } from '../ajustes/tela'
import { extensoesScreen } from '../extensoes/tela'
import { timesScreen } from '../times/navigation'
import { torreScreen } from '../torre/lugar-atual'
import { routinesScreen } from '../../rotinas-central/navigation'

// as entradas da chamada de voz além do botão. O atalho é uma preferência deste aparelho (desligada de fábrica,
// sem combinação padrão) que o main registra como atalho global; apertar só pede ao chat para decidir (decidirEntradaVoz)
// e abrir ou encerrar a MESMA chamada do botão. Nada aqui abre microfone: quem abre é a CallView, depois do início.

export interface PrefAtalhoVoz { ativo: boolean; acelerador: string }

const valido = (v: unknown): v is PrefAtalhoVoz =>
  typeof v === 'object' && v !== null && typeof (v as PrefAtalhoVoz).ativo === 'boolean' && typeof (v as PrefAtalhoVoz).acelerador === 'string'

/** Preferência deste aparelho; valor inválido volta ao desligado. */
export const atalhoVoz = pref<PrefAtalhoVoz>('voiceShortcut', { ativo: false, acelerador: '' }, valido)

/** O que o main respondeu ao último registro: a tela de Ajustes mostra o motivo se a combinação não pegou. */
class EstadoAtalho extends Observable {
  resultado: ResultadoAtalhoVoz | null = null
  set(resultado: ResultadoAtalhoVoz | null) {
    this.resultado = resultado
    this.changed()
  }
}
export const estadoAtalho = new EstadoAtalho()

/** Mantém o registro do main igual à preferência, desde a abertura do app (não só com Ajustes aberto). */
export function useSincronizarAtalhoVoz() {
  const [{ ativo, acelerador }] = usePref(atalhoVoz)
  // Declarado antes: ao sair (conta encerrada), solta o atalho do sistema; numa remontagem o efeito seguinte registra de novo.
  useEffect(() => () => { void window.codync.voz.definirAtalho({ ativo: false, acelerador: '' }).catch(() => {}) }, [])
  useEffect(() => {
    let atual = true
    const pedir = ativo && acelerador !== ''
    void window.codync.voz.definirAtalho({ ativo: pedir, acelerador }).then(
      (r) => { if (atual) estadoAtalho.set(pedir ? r : null) },
      () => { if (atual) estadoAtalho.set(pedir ? { ok: false, motivo: 'indisponivel' } : null) },
    )
    return () => { atual = false }
  }, [ativo, acelerador])
}

const CAMADAS = '[aria-modal="true"], [role="dialog"], [role="alertdialog"], [role="menu"], .context-layer'

/** A conversa está à vista: nenhuma tela (Ajustes, Rotinas…) nem camada na frente. O main já trouxe a janela à frente. */
export function conversaVisivel(): boolean {
  if (routinesScreen.open || timesScreen.open || torreScreen.open || settingsScreen.open || extensoesScreen.open) return false
  return !document.querySelector(CAMADAS)
}

const AVISO_MS = 6000

/** O que a conversa diz quando o atalho não liga. `oculta` não diz nada: a conversa não está à vista. */
export const TEXTO_RECUSA = {
  grupo: 'voz.chamada.entrada.recusa.grupo',
  sessao: 'voz.chamada.entrada.recusa.sessao',
  senha: 'voz.chamada.entrada.recusa.senha',
  indisponivel: 'voz.chamada.entrada.recusa.indisponivel',
} as const satisfies Record<Exclude<MotivoRecusaVoz, 'oculta'>, string>

/**
 * O atalho na conversa montada. `contexto` é lido na hora do aperto (estado vivo). Devolve o motivo da recusa
 * para a conversa dizer, por um instante, por que não ligou; nada é aberto em segundo plano.
 */
export function useEntradaVoz(contexto: () => Omit<ContextoEntradaVoz, 'conversaVisivel'>, iniciar: () => void, encerrar: () => void): MotivoRecusaVoz | null {
  const [recusa, setRecusa] = useState<MotivoRecusaVoz | null>(null)
  const vivo = useRef({ contexto, iniciar, encerrar })
  vivo.current = { contexto, iniciar, encerrar }

  useEffect(() => window.codync.app.onCommand((c) => {
    if (c.kind !== 'voiceToggle') return
    const atual = vivo.current
    const decisao = decidirEntradaVoz({ ...atual.contexto(), conversaVisivel: conversaVisivel() && c.modal !== true })
    if (decisao.acao === 'iniciar') { setRecusa(null); atual.iniciar() }
    else if (decisao.acao === 'encerrar') { setRecusa(null); atual.encerrar() }
    else setRecusa(decisao.motivo)
  }), [])

  useEffect(() => {
    if (!recusa) return
    const fim = setTimeout(() => setRecusa(null), AVISO_MS)
    return () => clearTimeout(fim)
  }, [recusa])

  return recusa
}
