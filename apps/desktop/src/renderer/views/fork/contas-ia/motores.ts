import type { Backend } from '../../../../shared/models.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { traduzir, type ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { ageSemAprovacao, textosSemAprovacao } from '../aviso-motor/aviso-motor.ts'

// D169/D164 (components/ContasIa, "Motores"): regras puras da seção de motores de Ajustes → Contas de IA, sobre o que o
// Hub manda em `hello.backends`. Nada aqui chama o Hub nem promete instalar ou entrar: cada estado vem de um campo real.

/** Codex e Claude Code entram pelas contas gerenciadas da seção de cima (D164), não pelo login antigo do Hub. */
export const MOTORES_COM_CONTAS = new Set(['codex', 'claude'])

/** A folha e seu terminal sobrevivem à reconexão do mesmo Hub, mas nunca aparecem em outro Hub. */
export function folhaNoHub<T extends { hubId: string }>(folha: T | null, hubId: string | undefined): T | null {
  return hubId && folha?.hubId === hubId ? folha : null
}

export type MotorTom = 'pronto' | 'atencao' | 'neutro'
/** `semAprovacao`: o Hub marcou o motor como "age sem pedir aprovação"; o selo e a explicação vêm à parte do estado de login e instalação. */
export interface MotorEstado { selo: string; tom: MotorTom; detalhe: string; instalado: boolean; semAprovacao: { selo: string; texto: string } | null }

/** Os motores que o Hub diz suportados (como o antigo catálogo): instalados, oficiais ou disponíveis pelo registro. */
export const motoresSuportados = (backends: readonly Backend[]) => backends.filter((b) => b.available || b.installed === true || b.curated === true)

/** Instalados primeiro, depois os para instalar, cada grupo na ordem do Hub. */
export function gruposDeMotores(backends: readonly Backend[]) {
  const suportados = motoresSuportados(backends)
  return { instalados: suportados.filter((b) => b.installed === true), paraInstalar: suportados.filter((b) => b.installed !== true) }
}

/** O estado de um motor no Hub, no idioma pedido (DON-917), só com o que o Hub informou. */
export const motorEstado = (b: Backend, idioma: AppLanguage): MotorEstado => ({ ...estadoDeInstalacao(b, idioma), semAprovacao: ageSemAprovacao(b) ? textosSemAprovacao(idioma) : null })

function estadoDeInstalacao(b: Backend, idioma: AppLanguage): Omit<MotorEstado, 'semAprovacao'> {
  const t = (chave: ChaveTexto) => (traduzir as (i: AppLanguage, c: ChaveTexto) => string)(idioma, chave)
  const contas = MOTORES_COM_CONTAS.has(b.id)
  if (b.installed !== true && b.curated === true) {
    return { selo: t('contas.motor.naoInstalado'), tom: 'neutro', detalhe: t(b.canInstall ? 'contas.motor.instaladorOficial' : 'contas.motor.instalacaoManual'), instalado: false }
  }
  if (b.node === 'missing') return { selo: t('contas.motor.precisaNode'), tom: 'atencao', detalhe: t('marca.motores.node'), instalado: b.installed === true }
  if (b.installed !== true) return { selo: t('contas.motor.instalaNoUso'), tom: 'neutro', detalhe: t('contas.motor.peloRegistro'), instalado: false }
  if (contas) return { selo: t('contas.motor.instalado'), tom: 'pronto', detalhe: t('contas.motor.loginNaAba'), instalado: true }
  if (b.signedIn === true) return { selo: t('contas.motor.pronto'), tom: 'pronto', detalhe: t('contas.motor.comLogin'), instalado: true }
  if (b.signedIn === false) return { selo: t('contas.motor.semLogin'), tom: 'atencao', detalhe: t('contas.motor.faltaEntrar'), instalado: true }
  return { selo: t('contas.motor.loginNaoVerificado'), tom: 'neutro', detalhe: t('contas.motor.abrirParaConferir'), instalado: true }
}

/** Algum motor suportado precisa do Node.js e o Hub ainda não tem a cópia do Donnabot. */
export const precisaNode = (backends: readonly Backend[]) => motoresSuportados(backends).some((b) => b.node === 'missing')
