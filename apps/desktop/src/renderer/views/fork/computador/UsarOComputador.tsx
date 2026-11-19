import { useContext, useEffect, useRef } from 'react'
import type { Bot, ScreenState } from '@shared/models'
import { RowLabel } from '../../../components/Controls'
import { VStack } from '../../../components/Stack'
import { SwitchRow } from '../../bots/parts'
import { AbrirComputadores } from './abrir'
import { controleDoComputador, moverFocoAoDesligar } from './estado'
import { useT } from '../../../lib/fork-i18n/estado'
import { Superficie } from '../../settings/rotulo'

/**
 * "Usar o computador" do bot (D240), no editor e no painel da conversa. O valor mostrado é sempre o
 * confirmado pelo Hub; a recusa aparece aqui, junto ao controle.
 */
export function UsarOComputador({ screen, bot, online, ocupado = false, erro = null, onChange }: {
  screen: ScreenState | null
  bot: Pick<Bot, 'computer'>
  online: boolean
  ocupado?: boolean
  erro?: string | null
  onChange: (on: boolean) => void
}) {
  const controle = controleDoComputador(screen, bot, online)
  const t = useT()
  const ajuda = controle.ajudaChaves.map((c) => t(c)).join(' ')
  const desligou = useRef(false)
  const anterior = useRef(controle.estado)
  const linha = useRef<HTMLDivElement>(null)
  useEffect(() => { if (erro) desligou.current = false }, [erro])
  const focoPendente = useRef(false)
  useEffect(() => {
    const antes = anterior.current
    anterior.current = controle.estado
    if (antes !== controle.estado) {
      const ativo = document.activeElement
      focoPendente.current = moverFocoAoDesligar(antes, controle.estado, desligou.current, !ativo || ativo === document.body)
      desligou.current = false
    }
    // Enquanto o controle está ocupado (no editor, o fieldset da página), o link ainda não recebe foco.
    if (!focoPendente.current || ocupado) return
    focoPendente.current = false
    const ativo = document.activeElement
    if (!ativo || ativo === document.body) (linha.current?.querySelector<HTMLElement>('button') ?? linha.current)?.focus()
  }, [controle.estado, ocupado])
  const mudar = (on: boolean) => {
    desligou.current = !on && controle.estado === 'ligadoSemTela'
    onChange(on)
  }
  return <VStack grow>
    <Superficie>
    {controle.mostraInterruptor
      ? <SwitchRow title={t('ajustes.usar.titulo')} detail={ajuda} on={bot.computer} disabled={controle.desabilitado || ocupado} onChange={mudar} />
      : <div ref={linha} tabIndex={-1}><VStack gap={6} align="flex-start"><RowLabel label={t('ajustes.usar.titulo')} detail={ajuda} /><VerComputadores /></VStack></div>}
    </Superficie>
    {erro ? <p className="bot-settings-control-error" role="alert" dir="auto">{erro}</p> : null}
  </VStack>
}

/** Ajustes → Computadores mostra os computadores; não liga a tela (isso se faz no próprio computador). */
export function VerComputadores() {
  const abrir = useContext(AbrirComputadores)
  const t = useT()
  return abrir ? <button type="button" className="plain-link small" onClick={abrir}>{t('ajustes.usar.ver')}</button> : null
}
