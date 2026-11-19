// Bot de fundo, desenho v2 (16/11): o seletor de cada função é o do chat (`seletor-modelo`), com a conta como parte da escolha.
// Aqui, sem React nem texto de tela: das contas do `functionModels` saem as que servem à função (marcáveis), as que não servem
// agora (com o estado e a ação), os motores que ela não aceita e a escolha atual; e de volta, a escolha vira o rascunho.
// Regra de ouro: conta ou modelo que não funciona nunca vem escolhido (`SEM_ESCOLHA`).
import type { ConfigFuncao, ContaInativa, ContaMotor, Escolha, Provedor } from '../seletor-modelo/regras.ts'
import type { Engine } from '../../../../shared/fork-engine-accounts.ts'
import { accountTitle, engineName, engineRegistry } from '../contas-ia/regras.ts'
import { automaticBackend, automaticoResolvivel, codexRefused, falhaDoRascunho, motivoDaConta, type Account, type Draft, type FunctionSnapshot, type Motivo } from './model.ts'

export { motivoDaConta, type Motivo }

/** Sem escolha: nenhum motor, conta ou modelo marcado (conta caída, sem conta pronta ou escolha que a função não aceita). */
export const SEM_ESCOLHA: Escolha = { accountId: null, backend: '', model: null, modelOptions: {}, fastMode: false }

/** Nome único por conta: o título (apelido, e-mail ou "Perfil antigo · motor"); repetido no mesmo motor, ganha o fim do id (e o número, se ainda repetir). */
export function rotulosUnicos(accounts: readonly Account[], legacy: string): Map<string, string> {
  const title = (a: Account) => accountTitle(a, legacy)
  const count = new Map<string, number>()
  for (const a of accounts) count.set(`${title(a)}|${a.backend}`, (count.get(`${title(a)}|${a.backend}`) ?? 0) + 1)
  const base = accounts.map((a) => [a.id, count.get(`${title(a)}|${a.backend}`)! > 1 ? `${title(a)} #${a.id.replace(/[^a-zA-Z0-9]/g, '').slice(-4)}` : title(a)] as const)
  const total = new Map<string, number>(), seen = new Map<string, number>()
  for (const [, label] of base) total.set(label, (total.get(label) ?? 0) + 1)
  return new Map(base.map(([id, label]) => {
    if (total.get(label)! < 2) return [id, label]
    const n = (seen.get(label) ?? 0) + 1
    seen.set(label, n)
    return [id, `${label} (${n})`]
  }))
}

/** Os textos do seletor da função, já no idioma da tela. */
export interface TextosSeletor {
  legacy: string
  estado: Record<Motivo | 'conectada', string>
  conectar: string
  confirmarLogin: string
  automatico: { nome: string; nomeCodex: string; etiqueta: string; meta: string }
  nota: string
  recusadoCurto: string
  recusado: (motor: string) => string
  semConta: (motor: string) => string
  trocarDica: (conta: string) => string
  automaticoFora: (conta: string) => string
}

/** Por que nada vem escolhido, quando há uma escolha salva que não vale (a conta, o modelo, a opção ou o Automático): a chave da tela e os dados. */
export type SemEscolha =
  | { chave: 'fundo.seletor.salvaConta'; params: { conta: string; estado: string } }
  | { chave: 'fundo.seletor.salvaModelo'; params: { conta: string; modelo: string } }
  | { chave: 'fundo.seletor.salvaOpcao'; params: { conta: string } }
  | { chave: 'fundo.seletor.automaticoFora'; params: { conta: string } }

export interface SeletorDaFuncao { provedores: Provedor[]; contas: ContaMotor[]; funcao: ConfigFuncao; valor: Escolha; rotulos: Map<string, string>; semEscolha: SemEscolha | null }

/**
 * Tudo o que o seletor precisa. `catalogo` são os motores e modelos do chat (`provedoresDoHub`): os mesmos grupos motor → modelo;
 * ficam só os motores com conta no Hub. `desconectada`: a conta da escolha salva caiu (resolução do Hub ou estado), então nada vem escolhido.
 */
export function seletorDaFuncao(o: { data: FunctionSnapshot; functionId: string; draft: Draft; catalogo: readonly Provedor[]; desconectada: boolean; textos: TextosSeletor; abrirContas: () => void }): SeletorDaFuncao {
  const { data, functionId, draft, textos } = o
  const rotulos = rotulosUnicos(data.accounts, textos.legacy)
  const motivos = new Map(data.accounts.map((a) => [a.id, motivoDaConta(a, functionId)] as const))
  const contas: ContaMotor[] = data.accounts.filter((a) => !motivos.get(a.id)).map((a) => ({ id: a.id, label: rotulos.get(a.id)!, backend: a.backend, state: 'connected', origin: 'managed', models: a.models.map((m) => ({ id: m.id })), automatico: automaticoResolvivel(a, functionId) }))
  const acao = (motivo: Motivo) => motivo === 'desconectada' ? { rotulo: textos.conectar, fazer: o.abrirContas } : motivo === 'naoConfirmada' ? { rotulo: textos.confirmarLogin, fazer: o.abrirContas } : undefined
  const inativas: ContaInativa[] = data.accounts.flatMap((a) => {
    const motivo = motivos.get(a.id)
    const a11y = motivo ? acao(motivo) : undefined
    return motivo ? [{ id: a.id, label: rotulos.get(a.id)!, backend: a.backend, estado: textos.estado[motivo === 'motorRecusado' ? 'conectada' : motivo], ...(a11y ? { acao: a11y } : {}) }] : []
  })
  const provedores = [...o.catalogo.filter((p) => data.accounts.some((a) => a.backend === p.id)), ...motoresSemCatalogo(data.accounts, o.catalogo)]
  const recusados = Object.fromEntries(provedores.filter((p) => codexRefused(functionId, { backend: p.id })).map((p) => [p.id, { curto: textos.recusadoCurto, motivo: textos.recusado(p.name) }]))
  // O Automático só existe com conta que o Hub resolve (receita): sem nenhuma, a linha nem aparece (nada de "Recomendado" sem prova).
  const auto = automaticBackend(functionId)
  const comAutomatico = provedores.some((p) => p.id === auto) && contas.some((c) => c.backend === auto && c.automatico)
  const funcao: ConfigFuncao = {
    recusados, inativas,
    automatico: comAutomatico ? { backend: auto, nome: auto === 'claude' ? textos.automatico.nome : textos.automatico.nomeCodex, etiqueta: textos.automatico.etiqueta, ...(auto === 'claude' ? { meta: textos.automatico.meta } : {}) } : null,
    nota: textos.nota, semConta: textos.semConta, trocarDica: textos.trocarDica, automaticoFora: textos.automaticoFora,
  }
  const valor = o.desconectada ? SEM_ESCOLHA : escolhaDoRascunho(draft, contas, data.accounts)
  return { provedores, contas, funcao, rotulos, valor, semEscolha: o.desconectada || valor !== SEM_ESCOLHA ? null : semEscolhaDe(draft, data.accounts, motivos, rotulos, textos) }
}

/** Por que a escolha salva não vem marcada (a conta que não serve, o modelo ou a opção que sumiu, o Automático sem receita); o rascunho fica como está. */
function semEscolhaDe(draft: Draft, accounts: readonly Account[], motivos: ReadonlyMap<string, Motivo | null>, rotulos: ReadonlyMap<string, string>, textos: TextosSeletor): SemEscolha | null {
  const account = accounts.find((a) => a.id === draft.accountId)
  if (!account) return null
  const conta = rotulos.get(account.id)!
  const motivo = motivos.get(account.id)
  if (motivo) return { chave: 'fundo.seletor.salvaConta', params: { conta, estado: motivo === 'motorRecusado' ? textos.recusadoCurto : textos.estado[motivo] } }
  switch (falhaDoRascunho(account, draft)) {
    case 'automatico': return { chave: 'fundo.seletor.automaticoFora', params: { conta } }
    case 'modelo': return draft.modelId ? { chave: 'fundo.seletor.salvaModelo', params: { conta, modelo: draft.modelId } } : null
    case 'opcao': return { chave: 'fundo.seletor.salvaOpcao', params: { conta } }
    default: return null
  }
}

/** Motor com conta no Hub mas fora do catálogo do chat (login caído, indisponível, não instalado): continua no trilho, para mostrar
 * as contas, o estado e o Conectar / Confirmar login. Só a representação: não muda quem serve (`motivoDaConta`) nem o modo conversa.
 * Sem o catálogo do motor, os modelos são os que as contas dele anunciam. */
function motoresSemCatalogo(accounts: readonly Account[], catalogo: readonly Provedor[]): Provedor[] {
  const motores = [...new Set(accounts.map((a) => a.backend))].filter((b): b is Engine => (b === 'claude' || b === 'codex') && !catalogo.some((p) => p.id === b))
  return motores.map((id) => ({
    id, name: engineName(id), registry: engineRegistry(id),
    models: [...new Set(accounts.filter((a) => a.backend === id).flatMap((a) => a.models.map((m) => m.id)))].map((m) => ({ id: m, name: m })),
  }))
}

/** O rascunho como a escolha do seletor; só vale com uma conta que serve (`contas`) e com o que ela de fato oferece: o Automático
 * precisa de receita que o Hub resolve, o modelo e as opções precisam existir na conta. Senão nada vem escolhido (`SEM_ESCOLHA`),
 * sem trocar nada no rascunho salvo. */
export function escolhaDoRascunho(draft: Draft, contas: readonly ContaMotor[], accounts: readonly Account[]): Escolha {
  const conta = contas.find((c) => c.id === draft.accountId)
  const account = accounts.find((a) => a.id === draft.accountId)
  if (!conta || falhaDoRascunho(account, draft)) return SEM_ESCOLHA
  if (draft.mode === 'automatic') return { accountId: conta.id, backend: conta.backend, model: null, modelOptions: {}, fastMode: false }
  return { accountId: conta.id, backend: conta.backend, model: draft.modelId, modelOptions: draft.modelOptions, fastMode: false }
}

/** A escolha do seletor como campos do rascunho: sem modelo é o Automático. */
export const rascunhoDaEscolha = (e: Escolha): Pick<Draft, 'mode' | 'accountId' | 'modelId' | 'modelOptions'> =>
  e.model === null ? { mode: 'automatic', accountId: e.accountId ?? '', modelId: '', modelOptions: {} }
    : { mode: 'custom', accountId: e.accountId ?? '', modelId: e.model, modelOptions: e.modelOptions }

/** O campo fechado: motor, conta e o nome do modelo (ou o Automático); `null` sem escolha. */
export function resumoDaEscolha(s: SeletorDaFuncao, textos: Pick<TextosSeletor, 'automatico'>): { motor: string; conta: string; modelo: string } | null {
  const { valor } = s
  const conta = s.contas.find((c) => c.id === valor.accountId)
  const provedor = s.provedores.find((p) => p.id === valor.backend)
  if (!conta || !provedor) return null
  const modelo = valor.model === null ? (valor.backend === 'claude' ? textos.automatico.nome : textos.automatico.nomeCodex) : (provedor.models?.find((m) => m.id === valor.model)?.name ?? valor.model)
  return { motor: provedor.name, conta: conta.label, modelo }
}

/** A frase do que acontece, como chave + dados. Os valores do tempo vêm do código (`MEMORIA_OCIOSA_MIN`); só a memória diz quando roda.
 * `modeloAuto`: o texto do Automático ("modelo leve"), usado no lugar do nome quando a escolha é automática. */
export function fraseDaFuncao(functionId: string, resumo: { motor: string; conta: string; modelo: string } | null, automatico: boolean, modeloAuto: string, minutos: number):
  | { chave: 'fundo.frase.memoria'; params: { motor: string; conta: string; modelo: string; minutos: number } }
  | { chave: 'fundo.frase.generica'; params: { motor: string; conta: string; modelo: string } }
  | null {
  if (!resumo) return null
  const params = { motor: resumo.motor, conta: resumo.conta, modelo: automatico ? modeloAuto : resumo.modelo }
  return functionId === 'memory_consolidation' ? { chave: 'fundo.frase.memoria', params: { ...params, minutos } } : { chave: 'fundo.frase.generica', params }
}
