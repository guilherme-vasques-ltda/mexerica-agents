// A página Extensões → Conectores: "Catálogo | Minhas contas" no topo e, abaixo, o catálogo, a ficha, o conectar ou as
// contas. Só navega e entrega ao Hub; o MarketplaceView herdado a coloca no lugar do antigo "Meus MCP" (um gancho).
import { useEffect, useMemo, useRef, useState } from 'react'
import { Button, TabChoice } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { HStack, VStack } from '../../../components/Stack'
import { useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { Catalogo } from './Catalogo'
import { Conectar } from './Conectar'
import { Ficha } from './Ficha'
import { Google } from './Google'
import { Personalizado } from './Personalizado'
import { MinhasContas } from './MinhasContas'
import { useMercado } from './estado'
import type { FluxoConectar } from './fluxo.ts'
import type { FluxoGoogle } from './google-fluxo.ts'
import { ehDoGoogle, jeitoRecomendado } from './regras.ts'
import type { Servico, TipoConexao } from './tipos.ts'
import { Aviso, Carregando } from './pecas'
import './mercado.css'

type Vista = { tipo: 'lista' } | { tipo: 'ficha'; id: string } | { tipo: 'conectar'; fluxo: FluxoConectar } | { tipo: 'google'; fluxo: FluxoGoogle } | { tipo: 'personalizado' }

export function PaginaConectores({ ativa = true, pedidoAdicionar = 0 }: { ativa?: boolean; pedidoAdicionar?: number }) {
  const store = useStore()
  const t = useT()
  const mercado = useMercado(store)
  const [vista, setVista] = useState<Vista>({ tipo: 'lista' })
  const hub = store.hello?.hostId ?? null
  // A leitura é uma só por Hub e link: volta ao abrir a página e quando a conexão muda (nunca mostra dado de outra origem).
  useEffect(() => { if (ativa) void mercado.ler() }, [ativa, hub, store.connectedClient, store.principalEpoch, mercado])
  // O botão "Adicionar conector personalizado" de Extensões abre a tela própria (v3 02), não a folha antiga.
  useEffect(() => {
    if (pedidoAdicionar <= 0) return
    void emAndamento.current?.cancelar()
    setVista({ tipo: 'personalizado' })
  }, [pedidoAdicionar])
  // Sair da página no meio de conectar (outra página de Extensões, fechar a tela): a conta pela metade não fica.
  const emAndamento = useRef<FluxoConectar | FluxoGoogle | null>(null)
  emAndamento.current = vista.tipo === 'conectar' || vista.tipo === 'google' ? vista.fluxo : null
  useEffect(() => () => { void emAndamento.current?.cancelar() }, [])
  const aba = mercado.aba
  const catalogo = mercado.catalogo
  const contas = mercado.contas
  const servicoAberto = useMemo(() => (vista.tipo === 'ficha' ? mercado.servico(vista.id) : null), [vista, mercado, catalogo])

  const mudarAba = (a: 'catalogo' | 'contas') => { mercado.abrirAba(a); setVista({ tipo: 'lista' }) }
  const abrirFicha = (s: Servico) => setVista({ tipo: 'ficha', id: s.id })
  // Serviço do Google conecta pelo fluxo próprio (o app da pessoa, a escolha dos serviços e o login em fila).
  const abrirGoogle = () => { const f = mercado.fluxoGoogle(); if (f) setVista({ tipo: 'google', fluxo: f }) }
  const conectar = (s: Servico, tipo: TipoConexao) => (ehDoGoogle(s) ? abrirGoogle() : setVista({ tipo: 'conectar', fluxo: mercado.fluxo(s, tipo) }))

  const abas = (
    <div className="mercado-topo mercado-abas">
      <TabChoice label={t('mercado.tab.rotulo')} selection={aba} onChange={mudarAba}
        options={[{ id: 'catalogo', label: t('mercado.tab.catalogo') }, { id: 'contas', label: `${t('mercado.tab.contas')}${contas.length ? ` ${contas.length}` : ''}` }]} />
    </div>
  )

  // Conectar e ficha ocupam a página inteira, com o próprio "Voltar".
  if (vista.tipo === 'personalizado') {
    return <Personalizado adicionar={(p) => mercado.adicionarPersonalizado(p)} aoCancelar={() => setVista({ tipo: 'lista' })}
      aoAdicionado={(nome) => { mercado.recemAdicionado = nome; mudarAba('contas') }} />
  }
  if (vista.tipo === 'google') {
    return <Google fluxo={vista.fluxo} aoSair={() => setVista({ tipo: 'lista' })} aoConcluir={() => { void mercado.ler(); mudarAba('contas') }} />
  }
  if (vista.tipo === 'conectar') {
    const fluxo = vista.fluxo
    return (
      <Conectar fluxo={fluxo} contaViva={fluxo.conta ? mercado.conta(fluxo.conta.id) : null} usarNosBots={(id, ids) => mercado.usarNosBots(id, ids)}
        aoVoltar={() => setVista({ tipo: 'ficha', id: fluxo.servico.id })} aoConcluir={() => { void mercado.ler(); setVista({ tipo: 'lista' }) }} />
    )
  }
  if (vista.tipo === 'ficha' && servicoAberto && catalogo) {
    return (
      <Ficha servico={servicoAberto} prazoDias={catalogo.prazoConferenciaDias} aoVoltar={() => setVista({ tipo: 'lista' })}
        aoGerenciar={() => mudarAba('contas')} aoConectar={(tipo) => conectar(servicoAberto, tipo)} />
    )
  }

  return (
    <div className="mercado">
      {abas}
      {aba === 'contas' ? <MinhasContas mercado={mercado} aoVerCatalogo={() => mudarAba('catalogo')} aoAdicionar={(s) => {
        const tipo = jeitoRecomendado(s)
        if (tipo) conectar(s, tipo)
      }} /> : (
        mercado.estado === 'semConexao' ? <Aviso>{t('mercado.catalogo.semConexao')}</Aviso>
        : mercado.estado === 'erro' ? (
          <VStack gap={8} className="mercado-vazio" align="center">
            <Text size="compactBody" weight="semibold" tone="text">{t('mercado.catalogo.erro.titulo')}</Text>
            <Text size="callout" tone="secondary">{mercado.erro}</Text>
            <HStack><Button kind="secondary" onClick={() => void mercado.ler()}>{t('mercado.catalogo.tentar')}</Button></HStack>
          </VStack>
        )
        : !catalogo ? <Carregando texto={t('mercado.catalogo.carregando')} />
        : <Catalogo catalogo={catalogo} contas={contas} cofreTrancado={mercado.cofreTrancado} contasFalharam={mercado.contasErro !== null} appGoogle={mercado.appGoogle} aoTentar={() => void mercado.ler()} aoAbrir={abrirFicha} aoAbrirGoogle={abrirGoogle} />
      )}
    </div>
  )
}
