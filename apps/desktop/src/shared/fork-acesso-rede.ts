// Acesso por rede própria: traduz recusas do Hub em decisão de interface. É só UX. Não autoriza nada:
// quem decide acesso é o Hub no despacho. Números de fechamento e a origem da rede (gateway ou Tailscale)
// não entram na decisão. Puro e sem DOM, sem I/O.

type Recusa = {
  acao: Acao
  renovarUmaVez: boolean
  terminal: boolean
  invalidaEstado: boolean
  encerraStream: boolean
}

type Acao =
  | 'renovar_sessao'
  | 'parear_de_novo'
  | 'reabrir_canal'
  | 'tirar_workspace'
  | 'recusar_acao'
  | 'descartar_intencao'
  | 'limpar_e_reabrir'
  | 'mostrar_indisponivel'
  | 'recusar'

export type Decisao = Recusa & { motivo: string }

const SEM_FLAGS = { renovarUmaVez: false, terminal: false, invalidaEstado: false, encerraStream: false }

const RECUSAS_RPC: Record<string, Recusa> = {
  auth_invalid: { ...SEM_FLAGS, acao: 'renovar_sessao', renovarUmaVez: true },
  device_unpaired: { ...SEM_FLAGS, acao: 'parear_de_novo' },
  device_revoked: { ...SEM_FLAGS, acao: 'parear_de_novo', terminal: true, invalidaEstado: true },
  device_lease_expired: { ...SEM_FLAGS, acao: 'reabrir_canal' },
  workspace_forbidden: { ...SEM_FLAGS, acao: 'tirar_workspace', terminal: true, invalidaEstado: true },
  role_insufficient: { ...SEM_FLAGS, acao: 'recusar_acao' },
  cross_workspace_reference: { ...SEM_FLAGS, acao: 'descartar_intencao' },
  workspace_epoch_mismatch: { ...SEM_FLAGS, acao: 'limpar_e_reabrir', invalidaEstado: true },
}

const EVENTOS_TERMINAIS: Record<string, Recusa> = {
  workspace_revoked: { ...SEM_FLAGS, acao: 'tirar_workspace', terminal: true, invalidaEstado: true, encerraStream: true },
  role_changed: { ...SEM_FLAGS, acao: 'limpar_e_reabrir', terminal: true, invalidaEstado: true, encerraStream: true },
}

const DESCONHECIDO: Recusa = { ...SEM_FLAGS, acao: 'recusar', terminal: true }

const OFFLINE: Recusa = { ...SEM_FLAGS, acao: 'mostrar_indisponivel' }

function decidir(tabela: Record<string, Recusa>, codigo: unknown): Decisao {
  if (typeof codigo === 'string' && Object.hasOwn(tabela, codigo)) return { motivo: codigo, ...tabela[codigo]! }
  return { motivo: 'desconhecido', ...DESCONHECIDO }
}

function ehObjeto(valor: unknown): valor is Record<string, unknown> {
  return typeof valor === 'object' && valor !== null
}

export function normalizarFalhaRemota(falha: unknown): Decisao {
  if (!ehObjeto(falha)) return { motivo: 'desconhecido', ...DESCONHECIDO }
  switch (falha.origem) {
    case 'transporte':
      return { motivo: 'hub_offline', ...OFFLINE }
    case 'rpc':
      return decidir(RECUSAS_RPC, falha.code)
    case 'canal':
      return decidir(RECUSAS_RPC, ehObjeto(falha.reject) ? falha.reject.stableCode : undefined)
    case 'evento':
      return decidir(EVENTOS_TERMINAIS, falha.evento)
    default:
      return { motivo: 'desconhecido', ...DESCONHECIDO }
  }
}
