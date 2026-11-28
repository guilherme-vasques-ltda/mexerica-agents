// /clear da Open: diálogo de confirmação (nada é apagado antes do ok) e o pedido ao Hub. Sem lógica de negócio aqui.
import { useRef, useState } from 'react'
import { Dialog } from '../../../components/Overlay'
import { Spinner } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useStoreRef } from '../../../store/context'
import { falhaLimpar, RPC_LIMPAR, type FalhaLimpar } from './limpar'
import './contexto.css'

const PRAZO_LIMPAR_MS = 240_000

type Fase = { id: 'parado' } | { id: 'confirmando' } | { id: 'andamento' } | { id: 'feito' } | { id: 'falha'; falha: FalhaLimpar }

export function useLimparConversa(botId: string) {
  const store = useStoreRef()
  const t = useT()
  const idioma = useIdioma()
  const [fase, setFase] = useState<Fase>({ id: 'parado' })
  const ativo = useRef(false)

  const confirmar = async () => {
    const client = store.connectedClient
    if (!client || ativo.current) return setFase({ id: 'falha', falha: 'generico' })
    ativo.current = true
    setFase({ id: 'andamento' })
    try {
      await client.call(RPC_LIMPAR, { botId, confirm: true, ...(client.spaceId ? { spaceId: client.spaceId } : {}) }, PRAZO_LIMPAR_MS)
      setFase({ id: 'feito' })
    } catch (erro) {
      setFase({ id: 'falha', falha: falhaLimpar(erro) })
    } finally {
      ativo.current = false
    }
  }

  const elemento = () => (
    <>
      <Dialog language={superficieIdioma(idioma)} open={fase.id === 'confirmando'} title={t('contexto.limpar.titulo')} message={t('contexto.limpar.texto')}
        cancel={t('contexto.limpar.nao')} onClose={() => setFase((f) => (f.id === 'confirmando' ? { id: 'parado' } : f))}
        actions={[{ title: t('contexto.limpar.confirmar'), destructive: true, action: () => void confirmar() }]} />
      {fase.id === 'parado' || fase.id === 'confirmando' ? null : (
        <div className="limpar-estado">
          {fase.id === 'andamento' ? <Spinner size={13} /> : null}
          <span role="status" aria-live="polite">{fase.id === 'andamento' ? t('contexto.limpar.andamento') : fase.id === 'feito' ? t('contexto.limpar.feito') : t(`contexto.limpar.erro.${fase.falha}`)}</span>
          {fase.id === 'andamento' ? null : <button type="button" className="limpar-fechar" aria-label={t('contexto.limpar.fechar')} onClick={() => setFase({ id: 'parado' })}>×</button>}
        </div>
      )}
    </>
  )
  return { pedir: () => setFase({ id: 'confirmando' }), ocupado: fase.id === 'andamento', elemento }
}
