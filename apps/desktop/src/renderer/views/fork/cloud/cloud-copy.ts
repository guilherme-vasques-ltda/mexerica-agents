// D171/D163: os textos do acesso pela nuvem, por estado do processo principal. Só descrevem o que o
// Main publicou: `linked` é vínculo gravado, não conexão; `ready` só conta com o anexo qualificado;
// uma recusa não diz a causa; e nada incerto vira sucesso.
import type { ForkCloudKind } from '@shared/fork-cloud'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { isolado, ltr } from '../../../../shared/bidi.ts'
import { localeDe, traduzir } from '../../../lib/fork-i18n/index.ts'
import type { Aparelho } from '../account/exit-copy'
import type { Flow } from './fork-cloud-model'

export interface Line {
  readonly text: string
  readonly tone: 'secondary' | 'attention' | 'danger'
}

const line = (text: string, tone: Line['tone'] = 'secondary'): Line => ({ text, tone })

/** Estados da conexão em andamento, com o canal ainda vivo. */
export const CONNECTION_KINDS: readonly ForkCloudKind[] = ['connecting', 'ready', 'reconnecting', 'hubOffline']

/**
 * O que um estado sem passo próprio diz: fim do que a pessoa começou (`flow`) ou resultado de uma
 * procura. Null: nada a dizer (por exemplo, `needsLink` depois de registrar ou procurar).
 */
export function statusLine(kind: ForkCloudKind, name: string, flow: Flow['kind'] | null, aparelho: Aparelho, idioma: AppLanguage): Line | null {
  // DON-917: o nome do Hub é texto livre de quem o cadastrou; isolado, não troca a ordem da frase em árabe.
  const nome = isolado(name)
  switch (kind) {
    case 'discovering':
      return line(traduzir(idioma, 'nuvem.estado.procurando'))
    case 'noHub':
      return line(traduzir(idioma, 'nuvem.estado.nenhumHub'))
    case 'needsLink':
      if (flow === 'local') return line(traduzir(idioma, `nuvem.estado.localNaoConfirmou.${aparelho}`, { nome }), 'attention')
      if (flow === 'remote') return line(traduzir(idioma, 'nuvem.estado.remotoNaoComecou', { nome }), 'attention')
      return flow ? line(traduzir(idioma, `nuvem.estado.semAcesso.${aparelho}`, { nome }), 'attention') : null
    case 'deploymentChanged':
      return line(traduzir(idioma, 'nuvem.estado.versaoNaoConfere'), 'attention')
    case 'hubCloudOff':
      return line(traduzir(idioma, `nuvem.estado.nuvemDesligada.${aparelho}`, { nome }), 'attention')
    case 'accountMismatch':
      return line(traduzir(idioma, `nuvem.estado.outraConta.${aparelho}`, { nome }), 'attention')
    case 'hubUnverified':
      return line(traduzir(idioma, 'nuvem.estado.naoVerificado', { nome }), 'danger')
    case 'handshakeRefused':
      return line(traduzir(idioma, `nuvem.estado.recusou.${aparelho}`, { nome }), 'attention')
    case 'accessEnded':
      return line(traduzir(idioma, 'nuvem.estado.acessoTerminou', { nome }), 'attention')
    case 'hubMissing':
      return line(traduzir(idioma, 'nuvem.estado.saiuDaConta', { nome }), 'attention')
    case 'updateRequired':
      return line(traduzir(idioma, 'nuvem.estado.versoes', { nome }), 'attention')
    case 'error':
      return line(traduzir(idioma, 'nuvem.estado.erro'), 'attention')
    case 'unsafeStorage':
      return line(traduzir(idioma, `nuvem.estado.semArmazenamento.${aparelho}`), 'danger')
    case 'storageError':
      return line(traduzir(idioma, `nuvem.estado.erroArmazenamento.${aparelho}`), 'danger')
    case 'signOutPending':
      return line(traduzir(idioma, 'nuvem.estado.saidaPendente'), 'attention')
    case 'revocationPending':
      return line(traduzir(idioma, 'nuvem.estado.remocaoPendente'), 'attention')
    case 'disabled':
      return line(traduzir(idioma, 'nuvem.estado.desligado'))
    default:
      return null
  }
}

/**
 * A conexão em andamento. `attached`: o Main publicou o anexo qualificado; sem ele, `ready` ainda
 * é conferência. `direct`: o SSH ou o host deste computador já mostra esse Hub e tem prioridade.
 */
export function connectionLine(kind: ForkCloudKind, name: string, attached: boolean, direct: boolean, idioma: AppLanguage): Line {
  const nome = isolado(name)
  switch (kind) {
    case 'ready':
      if (!attached) return line(traduzir(idioma, 'nuvem.conexao.conferindo', { nome }))
      return line(traduzir(idioma, direct ? 'nuvem.conexao.diretaPrioridade' : 'nuvem.conexao.conectado', { nome }))
    case 'reconnecting':
      return line(traduzir(idioma, 'nuvem.conexao.reconectando', { nome }), 'attention')
    case 'hubOffline':
      return line(traduzir(idioma, 'nuvem.conexao.foraDaPonte', { nome }), 'attention')
    default:
      return line(traduzir(idioma, 'nuvem.conexao.conectando', { nome }))
  }
}

/** Hora em que o pedido vence, sem relógio correndo na tela; a hora no formato do idioma e isolada (LTR). */
export function deadlineText(deadline: number | undefined, idioma: AppLanguage): string | null {
  if (typeof deadline !== 'number' || !Number.isFinite(deadline)) return null
  const hora = new Date(deadline).toLocaleTimeString(localeDe(idioma), { hour: '2-digit', minute: '2-digit' })
  return traduzir(idioma, 'nuvem.prazo', { hora: ltr(hora) })
}
