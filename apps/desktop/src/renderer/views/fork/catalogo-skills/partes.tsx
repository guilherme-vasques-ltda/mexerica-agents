import type { ReactNode } from 'react'
import { Icon } from '../../../components/Icon'
import { HStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { ThinkingOrb } from '../../../components/ThinkingOrb'
import type { CatalogSkill } from '../../../store/fork-skills'
import type { Pedido } from './catalogo-store'
import { rotuloDisponibilidade, type EstadoItem } from './regras'
import { textoDe, frase, frases, rotuloAcao } from './textos'
import { useIdioma } from '../../../lib/fork-i18n/estado'
import './catalogo-skills.css'

/** Selo curto com texto (nunca só cor). */
export function Selo({ children, tom = 'neutro', title, id }: { children: ReactNode; tom?: 'neutro' | 'ok' | 'aviso'; title?: string; id?: string }) {
  return <span id={id} className={`cs-selo ${tom}`} title={title}>{children}</span>
}

/** Os selos de um item: disponibilidade, execução desligada e outra versão. */
export function SelosDoItem({ item, estado }: { item: CatalogSkill; estado: EstadoItem }) {
  const idioma = useIdioma()
  return (
    <>
      <Selo tom={item.availability === 'text' || item.availability === 'adapted' ? 'neutro' : 'aviso'}>{textoDe(rotuloDisponibilidade[item.availability], idioma)}</Selo>
      {item.readiness ? <Selo tom="aviso" title={item.readiness.reason}>{textoDe(frase('skills.selo.execucaoDesligada'), idioma)}</Selo> : null}
      {estado.tipo === 'outraVersao' ? <Selo tom="aviso">{textoDe(frase('skills.selo.outraVersaoInstalada'), idioma)}</Selo> : null}
    </>
  )
}

/**
 * A ação de instalar de um item. Instalar, instalando e instalada são o mesmo botão, que muda de
 * rótulo sem sair do lugar: o foco fica nele. `aria-disabled` (e não `disabled`) pelo mesmo motivo.
 * Bloqueada e outra versão não têm botão. Com um pedido incerto ainda não conferido (`conferir`), o
 * mesmo botão só relê a lista do Hub: instalar de novo espera essa conferência.
 */
export function AcaoInstalar({ item, estado, pedido, falhou, onInstalar, conferir, folha = false, desligada = false }: {
  item: CatalogSkill
  estado: EstadoItem
  pedido: Pedido | null
  falhou: boolean
  onInstalar: () => void
  folha?: boolean
  /** A folha abriu em outra conexão: o botão fica, mas não age (o motivo está na folha). */
  desligada?: boolean
  /** Pedido incerto ainda não conferido: o botão relê a lista do Hub em vez de instalar. */
  conferir?: () => void
}) {
  const idioma = useIdioma()
  const ocupado = pedido?.tipo === 'instalar'
  if (!ocupado && conferir) {
    return (
      <button
        className={`${folha ? 'styled-button secondary cs-acao-folha' : 'capsule-action cs-acao'}${desligada ? ' inerte' : ''}`}
        aria-disabled={desligada || undefined}
        aria-label={textoDe(frase('skills.acao.conferirNome', { acao: rotuloAcao.conferir, nome: item.name }), idioma)}
        onClick={(e) => {
          e.stopPropagation()
          if (!desligada) conferir()
        }}
      >
        <HStack as="span" gap={5} align="center" justify="center">
          <Icon name="arrow.clockwise" size={11} weight="semibold" />
          <span>{textoDe(rotuloAcao.conferir, idioma)}</span>
        </HStack>
      </button>
    )
  }
  if (!ocupado && (estado.tipo === 'bloqueada' || estado.tipo === 'outraVersao')) {
    return (
      <span className={`cs-acao-texto${folha ? ' folha' : ''}`}>
        <Text size="caption" weight="semibold" tone="secondary">{estado.tipo === 'bloqueada' ? textoDe(frases.bloqueada, idioma) : textoDe(frase('skills.selo.outraVersao'), idioma)}</Text>
      </span>
    )
  }
  const instalada = !ocupado && estado.tipo === 'instalada'
  const acao = ocupado ? (pedido.fase === 'conferindo' ? rotuloAcao.conferindo : rotuloAcao.instalando)
    : instalada ? rotuloAcao.instalada : falhou ? rotuloAcao.tentar : rotuloAcao.instalar
  const rotulo = textoDe(acao, idioma)
  const inerte = ocupado || instalada || desligada
  return (
    <button
      className={`${folha ? 'styled-button primary cs-acao-folha' : 'capsule-action cs-acao'}${inerte ? ' inerte' : ''}${instalada ? ' instalada' : ''}`}
      aria-disabled={inerte || undefined}
      aria-busy={ocupado || undefined}
      aria-label={textoDe(frase('skills.acao.rotuloNome', { acao, nome: item.name }), idioma)}
      onClick={(e) => {
        e.stopPropagation()
        if (!inerte) onInstalar()
      }}
    >
      <HStack as="span" gap={5} align="center" justify="center">
        {ocupado ? <ThinkingOrb size={14} color="currentColor" />
          : instalada ? <Icon name="checkmark" size={11} weight="semibold" />
          : <Icon name={falhou ? 'arrow.clockwise' : 'arrow.down.to.line'} size={11} weight="semibold" />}
        <span>{rotulo}</span>
      </HStack>
    </button>
  )
}

/** Região que o leitor de tela anuncia sem mover o foco. */
export function Anuncio({ texto }: { texto: string }) {
  return <div role="status" aria-live="polite" className="cs-anuncio">{texto}</div>
}
