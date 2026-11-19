// MEX-1307 F0 (plano §4.3.1, V6): lê os quadros da sondagem (a) e decide se o servidor é compatível. Fail-closed: requisito
// que não veio, veio repetido com estados diferentes ou veio `pulado` (não consegui determinar) vale como não atendido.
import type { Arquitetura, CodigoEvento, Dado, EstadoEvento, IdSo } from './esquemas.ts'
import type { Evento, ResultadoQuadros } from './quadros.ts'

export const REQUISITOS = ['so', 'arquitetura', 'systemd', 'espaco', 'sudo', 'sudotty', 'libcap', 'nosuid', 'hardlinks', 'relogio'] as const
export type Requisito = (typeof REQUISITOS)[number]

/** Por que o servidor é Incompatível; cada valor tem uma frase no catálogo (`instalacaoRemota.incompativel.*`). */
export type MotivoIncompativel =
  | Requisito | 'sudo-indisponivel' | 'login-root' | 'login-e-hub' | 'hub-uid-zero' | 'identidade' | 'porta-ocupada' | 'porta-desconhecida' | 'saida-invalida'
  | 'estado-ilegivel'

/** V6: `sem-senha` = `sudo -n` passou (não abre a tela de senha); `pede-senha` = só falhou por exigir senha. */
export type ModoSudo = 'sem-senha' | 'pede-senha' | 'indisponivel'
export type Presenca = 'presente' | 'ausente' | 'desconhecido'

export interface ResultadoSondagem {
  compativel: boolean
  motivos: MotivoIncompativel[]
  sudo: ModoSudo
  arquitetura: Arquitetura | null
  so: { id: IdSo; versao: string } | null
  uidAdmin: number | null
  uidHub: number | null
  contaHub: Presenca
  motor: Presenca
  hostInstalado: Presenca
  linger: Presenca
  portaOcupada: Presenca
}

const presenca = (e: EstadoEvento | undefined): Presenca => (e === 'ok' ? 'presente' : e === 'pulado' ? 'ausente' : 'desconhecido')

/** Um único estado por código; repetido com estados diferentes = inconsistente (`undefined` + `conflito`). */
function indexar(eventos: readonly Evento[]) {
  const mapa = new Map<CodigoEvento, EstadoEvento>()
  let conflito = false
  for (const e of eventos) {
    const antes = mapa.get(e.codigo)
    if (antes !== undefined && antes !== e.estado) conflito = true
    mapa.set(e.codigo, e.estado)
  }
  return { mapa, conflito }
}

export function avaliarSondagem(saida: ResultadoQuadros): ResultadoSondagem {
  const motivos: MotivoIncompativel[] = []
  const { mapa, conflito } = indexar(saida.eventos)
  if (saida.falha !== null || conflito) motivos.push('saida-invalida')
  for (const r of REQUISITOS) if (mapa.get(r) !== 'ok') motivos.push(r)

  const sudoN = mapa.get('sudosemsenha')
  const sudo: ModoSudo = sudoN === 'ok' ? 'sem-senha' : sudoN === 'pulado' ? 'pede-senha' : 'indisponivel'
  if (sudo === 'indisponivel' && !motivos.includes('sudo') && !motivos.includes('sudotty')) motivos.push('sudo-indisponivel')

  // Dado repetido e contraditório não vale (nem o primeiro): a saída inteira é inválida.
  const unico = <T extends Dado['tipo']>(tipo: T) => {
    const todos = saida.dados.filter((d): d is Extract<Dado, { tipo: T }> => d.tipo === tipo)
    if (todos.some((d) => JSON.stringify(d) !== JSON.stringify(todos[0]))) { motivos.push('saida-invalida'); return undefined }
    return todos[0]
  }
  const uid = unico('uid')
  const arq = unico('arq')
  const so = unico('so')
  // Requisito `ok` sem o dado que o sustenta não é compatibilidade positiva.
  if (mapa.get('arquitetura') === 'ok' && !arq) motivos.push('arquitetura')
  if (mapa.get('so') === 'ok' && !so) motivos.push('so')
  const contaHub = presenca(mapa.get('contahub'))
  const ehHub = mapa.get('ehhub')
  const hostInstalado = presenca(mapa.get('hostinst'))
  const motor = presenca(mapa.get('motor'))
  // Identidade: o root reconfere por `getent` na fase B (F1); aqui os UIDs são só informativos (§4.6).
  if (!uid || contaHub === 'desconhecido' || ehHub === undefined || ehHub === 'falha' || hostInstalado === 'desconhecido' || motor === 'desconhecido') motivos.push('identidade')
  if (uid) {
    if (uid.uidAdmin === 0) motivos.push('login-root')
    if (uid.uidHub === 0) motivos.push('hub-uid-zero')
    if (uid.uidHub !== null && uid.uidAdmin === uid.uidHub) motivos.push('login-e-hub')
    if (contaHub === 'presente' && uid.uidHub === null) motivos.push('identidade')
    if (contaHub === 'ausente' && uid.uidHub !== null) motivos.push('identidade')
  }
  if (ehHub === 'ok') motivos.push('login-e-hub')
  const porta = presenca(mapa.get('portaocupada'))
  if (porta === 'presente' && hostInstalado === 'ausente') motivos.push('porta-ocupada')
  // Não consegui ver as portas (ausência de `ss` ou erro): sem prova de porta livre, bloqueia.
  if (porta === 'desconhecido') motivos.push('porta-desconhecida')

  const unicos = [...new Set(motivos)]
  return {
    compativel: unicos.length === 0,
    motivos: unicos,
    sudo,
    arquitetura: arq?.arquitetura ?? null,
    so: so ? { id: so.id, versao: so.versao } : null,
    uidAdmin: uid?.uidAdmin ?? null,
    uidHub: uid?.uidHub ?? null,
    contaHub,
    motor,
    hostInstalado,
    linger: presenca(mapa.get('linger')),
    portaOcupada: porta,
  }
}

/**
 * Sondagem (b): exatamente um quadro `estado812`; ausente, repetido ou com a fase falha = `ilegivel` (bloqueio, nunca limpo).
 * `ilegivel` e quadro inválido viram Incompatível; `incompleto` vira IntervencaoManual.
 */
export function classificarEstado812(saida: ResultadoQuadros) {
  const quadros = saida.dados.filter((d) => d.tipo === 'estado812')
  if (saida.falha !== null || quadros.length !== 1) return 'ilegivel' as const
  return (quadros[0] as Extract<Dado, { tipo: 'estado812' }>).classe
}
