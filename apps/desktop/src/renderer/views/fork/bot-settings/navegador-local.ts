import { useEffect, useRef, useState } from 'react'
import type { BotStore } from '../../../store/bot-store'

type EstadoLocal = { enabled: boolean; ready: boolean; canChange: boolean; reason?: string | null }

async function lerEstadoLocal(store: BotStore, hubId: string): Promise<EstadoLocal | null> {
  const value = await (await store.ready()).call<EstadoLocal & { supported: boolean }>('donnaBrowserLocalState', { hubId })
  return value.supported === true ? value : null
}

export function imagemDoNavegador(value: unknown): string | null {
  if (!value || typeof value !== 'object' || !('jpeg' in value)) return null
  const { jpeg } = value
  return typeof jpeg === 'string' && jpeg.length <= 350_000 && jpeg.length % 4 === 0
    && /^\/9j\/[A-Za-z0-9+/]*={0,2}$/.test(jpeg) ? `data:image/jpeg;base64,${jpeg}` : null
}

export function useNavegadorLocal(store: BotStore, hubId: string, spaceId: string, botId: string, aberto: boolean) {
  const [estado, setEstado] = useState<EstadoLocal | null>(null)
  const [imagem, setImagem] = useState<string | null>(null)
  const [falhaNoQuadro, setFalhaNoQuadro] = useState(false)
  const [falhou, setFalhou] = useState(false)
  const [pendente, setPendente] = useState(false)
  const consentimento = useRef({ revisao: 0, gravando: false })
  useEffect(() => {
    let ativo = true
    if (!hubId || !store.isLoopback) return
    const consultar = () => {
      if (consentimento.current.gravando) return
      const { revisao } = consentimento.current
      void lerEstadoLocal(store, hubId)
        .then(value => {
          if (ativo && revisao === consentimento.current.revisao) setEstado(value)
        }).catch(() => {})
    }
    consultar()
    if (estado?.canChange || !estado?.ready || falhou) window.addEventListener('focus', consultar)
    return () => {
      ativo = false
      window.removeEventListener('focus', consultar)
    }
  }, [store, hubId, estado?.ready, estado?.canChange, falhou])

  useEffect(() => {
    if (!aberto || !estado?.enabled || !estado.ready || !hubId) return
    let ativo = true
    let falhas = 0
    let timer: ReturnType<typeof setTimeout> | undefined
    const atualizar = async () => {
      if (!document.hidden) {
        try {
          const quadro = await (await store.ready()).call<{ jpeg: unknown }>('donnaBrowserLocalFrame', { hubId, spaceId, botId }, 4_000)
          const src = imagemDoNavegador(quadro)
          if (!src && quadro.jpeg !== null) throw new Error()
          falhas = 0
          if (ativo) {
            setImagem(document.hidden ? null : src)
            setFalhaNoQuadro(false)
          }
        } catch {
          if (ativo) {
            setImagem(null)
            falhas++
            setFalhaNoQuadro(!document.hidden && falhas >= 2)
          }
        }
      } else if (ativo) {
        falhas = 0
        setImagem(null)
        setFalhaNoQuadro(false)
      }
      if (ativo) timer = setTimeout(atualizar, 500)
    }
    void atualizar()
    return () => { ativo = false; clearTimeout(timer); setImagem(null); setFalhaNoQuadro(false) }
  }, [store, hubId, spaceId, botId, aberto, estado?.enabled, estado?.ready])

  async function mudar(enabled: boolean) {
    if (consentimento.current.gravando || !estado?.canChange) return
    consentimento.current = { revisao: consentimento.current.revisao + 1, gravando: true }
    setFalhou(false)
    setPendente(true)
    try {
      const result = await (await store.ready()).call<{ enabled: boolean }>('donnaBrowserLocalConsent', { hubId, enabled })
      if (result.enabled !== enabled) throw new Error()
      setEstado(current => current && { ...current, enabled })
    } catch {
      const confirmado = await lerEstadoLocal(store, hubId).catch(() => null)
      if (confirmado) setEstado(confirmado)
      setFalhou(confirmado?.enabled !== enabled)
    } finally {
      consentimento.current.gravando = false
      setPendente(false)
    }
  }

  return { estado, imagem, falhaNoQuadro, falhou, pendente, mudar }
}
