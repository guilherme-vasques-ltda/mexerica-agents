// DON-908 (C8) + MX-145 E2: Ajustes → Uso, o painel de uso e gasto do mês. Este arquivo só liga os dados à view
// (`PainelUso`): o Hub selecionado (senão o deste computador), como a C6; só o dono lê (403 vira aviso). Lido ao abrir,
// ao trocar o mês e por Atualizar; nada vai para o mirror. As abas ficam no cabeçalho de Ajustes (`abasDe.usage`).
import { HStack } from '../../../components/Stack'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import { useModel } from '../../../lib/observable'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { ComputerBadge } from '../../ComputerBadge'
import { mesDe, type AbaUso } from './modelo'
import { PainelUso, type LimitesItem } from './PainelUso'
import { useCusto } from './useCusto'
import { useCadastroContas } from './useCadastroContas'
import './custo.css'

export function CustoUso({ aba }: { aba: AbaUso }) {
  const app = useApp()
  const store = app.selectedStore ?? app.local
  const idioma = useIdioma()
  return (
    <div className="idioma-superficie custo-uso" {...superficieIdioma(idioma)}>
      {store ? <Painel key={store.computer.id} source={store} aba={aba} /> : <SemComputador aba={aba} />}
    </div>
  )
}

/** Os limites de todos os computadores conhecidos, com o último `usage` de cada um (como a lista de antes). */
function useLimites(): LimitesItem[] {
  const app = useApp()
  const stores = app.computers.flatMap((c) => app.store(c.id) ?? [])
  return stores.flatMap((s) => s.usage.providers.length === 0 ? [] : [{
    chave: s.computer.id,
    rotulo: stores.length > 1 ? <HStack gap={6} className="settings-usage-computer"><ComputerBadge computer={s.computer} size={16} /><bdi>{s.hostName}</bdi></HStack> : null,
    usage: s.usage,
  }])
}

function SemComputador({ aba }: { aba: AbaUso }) {
  const limites = useLimites()
  return <PainelUso estado={{ kind: 'lendo' }} lendo={false} online={false} mes={mesDe(Date.now())} onMes={() => {}} onAtualizar={() => {}} aba={aba} cadastro={{ kind: 'lendo' }} bot={() => undefined} limites={limites} />
}

function Painel({ source, aba }: { source: BotStore; aba: AbaUso }) {
  const store = useModel(source)
  // `undefined`: o mês corrente do Hub (UTC). As setas escolhem um mês exato a partir do que o Hub respondeu.
  // Por Hub: o mês escolhido num Hub não muda a leitura do outro (estado-pagina.ts).
  const [mes, setMes] = useEstadoDaPagina<string | undefined>(`uso/${store.computer.id}/mes`, undefined)
  const { estado, lendo, ler, online } = useCusto(store, mes ? { month: mes } : {}, true)
  const cadastro = useCadastroContas(store, aba === 'conta')
  const limites = useLimites()
  const atual = estado.kind === 'pronto' ? estado.relatorio.month : mes ?? mesDe(Date.now())
  return (
    <PainelUso estado={estado} lendo={lendo} online={online} mes={atual} onMes={setMes} onAtualizar={() => { ler(); cadastro.recarregar() }}
      aba={aba} cadastro={cadastro.estado} bot={(id) => store.bots.get(id)} limites={limites} />
  )
}
