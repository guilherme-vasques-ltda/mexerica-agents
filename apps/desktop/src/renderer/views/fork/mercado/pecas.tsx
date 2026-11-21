// Peças pequenas que as telas do Marketplace repetem. Sem Hub: só desenham o que recebem.
import type { ReactNode } from 'react'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/index.ts'
import { ServiceLogo } from '../../marketplace/pieces'
import { codigoDaRecusa, type CodigoDeRecusa } from './regras.ts'
import './mercado.css'

/** O ícone do serviço: a marca dele (arquivo local, pela `marca` da ficha) ou, sem arquivo, o globo. Nada vem da rede. */
export const LogoServico = ({ nome, marca, size = 40 }: { nome: string; marca?: string | null; size?: number }) => <ServiceLogo name={nome} marca={marca} size={size} />

export const ChaveDoErro: Record<CodigoDeRecusa, ChaveTexto> = {
  nome_obrigatorio: 'mercado.erro.nome_obrigatorio',
  nome_primeira_obrigatorio: 'mercado.erro.nome_primeira_obrigatorio',
  nome_repetido: 'mercado.erro.nome_repetido',
  nome_invalido: 'mercado.erro.nome_invalido',
  ficha_vencida: 'mercado.erro.ficha_vencida',
  servico_desconhecido: 'mercado.erro.servico_desconhecido',
  tipo_desconhecido: 'mercado.erro.tipo_desconhecido',
  tipo_indisponivel: 'mercado.erro.tipo_indisponivel',
  liberacao_desconhecida: 'mercado.erro.liberacao_desconhecida',
  liberacao_indisponivel: 'mercado.erro.liberacao_indisponivel',
  liberacao_obrigatoria: 'mercado.erro.liberacao_obrigatoria',
  conta_de_outro_space: 'mercado.erro.conta_de_outro_space',
  conta_padrao_invalida: 'mercado.erro.conta_padrao_invalida',
  ficha_indisponivel: 'mercado.erro.ficha_indisponivel',
  ligar_e_desligar: 'mercado.erro.ligar_e_desligar',
  principal_sem_contas: 'mercado.erro.principal_sem_contas',
  confirmacao_obrigatoria: 'mercado.erro.confirmacao_obrigatoria',
  ferramenta_mudou: 'mercado.erro.ferramenta_mudou',
  vinculo_mudou: 'mercado.erro.vinculo_mudou',
  vinculo_obrigatorio: 'mercado.erro.ferramenta_mudou',
  ferramenta_desconhecida: 'mercado.erro.ferramenta_desconhecida',
  ferramenta_fora_da_ficha: 'mercado.erro.ferramenta_fora_da_ficha',
  conta_sem_ficha: 'mercado.erro.conta_sem_ficha',
  conta_desligada_no_bot: 'mercado.erro.conta_desligada_no_bot',
  app_proprio_ausente: 'google.erro.app_proprio_ausente',
  app_google_invalido: 'google.erro.app_google_invalido',
  cofre_trancado: 'google.erro.cofre_trancado',
}

/** A recusa do Hub na frase da tela, pelo código estável; sem código, a frase geral (o texto bruto vai em "Ver detalhes"). */
export function frasesDaRecusa(t: Tradutor, erro: unknown, nomeDigitado = ''): string {
  const codigo = codigoDaRecusa(erro)
  if (codigo === null) return t('mercado.erro.generico')
  const chave = ChaveDoErro[codigo]
  if (chave === 'mercado.erro.nome_repetido') return t('mercado.erro.nome_repetido', { sugestao: `${nomeDigitado.trim() || '…'} 2` })
  return (t as unknown as (c: ChaveTexto) => string)(chave)
}

/** Um aviso discreto; `alerta` o pinta com o destaque. */
export function Aviso({ icon = 'info.circle', alerta = false, children }: { icon?: string; alerta?: boolean; children: ReactNode }) {
  return (
    <div className={`mercado-aviso${alerta ? ' alerta' : ''}`} role={alerta ? 'alert' : undefined}>
      <Icon name={icon} size={13} />
      <Text size="callout" as="div">{children}</Text>
    </div>
  )
}

export function Erro({ children }: { children: ReactNode }) {
  return (
    <div className="mercado-erro" role="alert">
      <Icon name="exclamationmark.triangle" size={13} />
      <Text size="callout" as="div">{children}</Text>
    </div>
  )
}

/** O título de uma seção com uma frase de apoio. */
export function Titulo({ titulo, sub, nivel = 'headline' }: { titulo: string; sub?: string; nivel?: 'headline' | 'title3' }) {
  return (
    <VStack gap={2}>
      <Text as="h3" size={nivel === 'title3' ? 'title3' : 'headline'} weight="semibold" tone="text">{titulo}</Text>
      {sub ? <Text size="callout" tone="secondary" as="div">{sub}</Text> : null}
    </VStack>
  )
}

export const PontoDeEstado = ({ alerta = false }: { alerta?: boolean }) => <span aria-hidden className={`mercado-ponto${alerta ? ' alerta' : ''}`} />

export function Carregando({ texto, linhas = 4 }: { texto: string; linhas?: number }) {
  return (
    <div className="mercado-esqueleto" role="status" aria-live="polite" aria-label={texto}>
      <Text size="callout" tone="secondary">{texto}</Text>
      {Array.from({ length: linhas }, (_, i) => <div key={i} aria-hidden />)}
    </div>
  )
}

export const Linha = ({ children, gap = 10 }: { children: ReactNode; gap?: number }) => <HStack gap={gap} shrink>{children}</HStack>
