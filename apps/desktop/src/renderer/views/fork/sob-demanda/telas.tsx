// Auditoria D249 (E2): as telas e peças pesadas só carregam quando abrem. A janela de conversa não lê o terminal
// (@xterm/xterm), o QR code, Times, Rotinas, Ajustes nem Extensões ao abrir; a ilha não chega a nenhuma delas.
// Enquanto a parte carrega (arquivo local, quase sempre um quadro), o lugar mostra o Spinner do sistema visual.
import { lazy, Suspense, useMemo, useState, type ComponentType } from 'react'
import { Button, Spinner } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { carregarOuIndisponivel } from './carregar'
import './sob-demanda.css'

/** A falha fica no espaço da parte e a nova tentativa não apaga o restante da janela. */
function ParteIndisponivel({ tentar }: { tentar: () => void }) {
  const idioma = useIdioma(), t = useT()
  return <div className="sob-demanda-indisponivel idioma-superficie" {...superficieIdioma(idioma)} role="alert">
    <p>{t('sistema.conversa.parteIndisponivel')}</p>
    <Button kind="secondary" onClick={tentar}>{t('sistema.conversa.tentarParte')}</Button>
  </div>
}

function sobDemanda<P extends object>(carregar: () => Promise<ComponentType<P>>): ComponentType<P> {
  return function SobDemanda(props: P) {
    const [tentativa, setTentativa] = useState(0)
    const Parte = useMemo(() => {
      const Falhou: ComponentType<P> = () => <ParteIndisponivel tentar={() => setTentativa(n => n + 1)} />
      return lazy(() => carregarOuIndisponivel<ComponentType<P>>(carregar, Falhou))
    }, [tentativa])
    return <Suspense fallback={<div className="sob-demanda-carregando"><Spinner size={22} /></div>}><Parte {...props} /></Suspense>
  }
}

export const RoutinesCentralView = sobDemanda(async () => (await import('../../rotinas-central/RoutinesCentralView')).RoutinesCentralView)
export const TimesScreenView = sobDemanda(async () => (await import('../times/TimesView')).TimesScreenView)
export const TorreScreenView = sobDemanda(async () => (await import('../torre/TorreView')).TorreScreenView)
export const AjustesTela = sobDemanda(async () => (await import('../ajustes/AjustesTela')).AjustesTela)
export const ExtensoesTela = sobDemanda(async () => (await import('../extensoes/ExtensoesTela')).ExtensoesTela)
export const SetupTerminalView = sobDemanda(async () => (await import('../../marketplace/SetupTerminal')).SetupTerminalView)
export const TerminalDeSetup = sobDemanda(async () => (await import('../contas-ia/TerminalDeSetup')).TerminalDeSetup)
export const PairingCode = sobDemanda(async () => (await import('../../PairingCode')).PairingCode)
