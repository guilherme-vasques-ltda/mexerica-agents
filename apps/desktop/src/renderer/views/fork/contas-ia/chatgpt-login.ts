// Entrar com ChatGPT (Plano do ChatGPT): o Hub prepara o login no navegador e o retorno chega ao ouvinte do
// app (main/fork-oauth-loopback.ts); aqui só a regra da tela, pura e testável — a view não faz regra de negócio.
// Os tokens nunca passam por aqui: o Hub os guarda cifrados no cofre e devolve só o estado da conta.
import type { HostClient } from '../../../client/host-client'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { lerRetornoLogin } from '../meus-mcp/retorno-login.ts'

/** O ouvinte de retorno do app (main/fork-oauth-loopback-ipc.ts), como o renderer o vê. */
export interface PonteDoOuvinte {
  abrir(): Promise<{ porta: number; tentativa: number } | null>
  esperar(tentativa: number, url: string): Promise<string>
  responder(tentativa: number, resultado: { ok: boolean; nome?: string; detalhe?: string }): void
  fechar(tentativa: number): void
}

/** O estado da conta que o Hub devolve; nenhum token, só o que a tela mostra. */
export interface EstadoChatGPT {
  conectado: boolean
  rotulo?: string
  planoHabilitado: boolean
  precisaEntrar: boolean
}

/** O desfecho do login: a conta como ficou, ou a recusa com código estável do Hub. */
export type DesfechoChatGPT =
  | { ok: true; rotulo: string | null; planoHabilitado: boolean; avisoAnteriorSemConfirmacao?: boolean }
  | { ok: false; erro: string }

/** O login não chegou ao fim: a pessoa desistiu, o prazo acabou ou o fluxo não conseguiu continuar. */
export class FalhaChatGPT extends Error {
  readonly tipo: 'cancelado' | 'tempo' | 'falha'
  constructor(tipo: FalhaChatGPT['tipo']) {
    super(tipo) // textos-ok: código técnico interno, nunca mostrado
    this.tipo = tipo
  }
}

/** A frase de cada falha do login, para o bloco de avisos da seção. */
export const fraseDaFalha = (tipo: FalhaChatGPT['tipo']): ChaveTexto =>
  tipo === 'cancelado' ? 'contas.chatgpt.erro.cancelado'
    : tipo === 'tempo' ? 'contas.chatgpt.erro.tempo'
      : 'contas.chatgpt.erro.falha'

const PRAZO_MS = 60_000
/** O ouvinte corta a espera em 10 minutos; aqui corta um pouco antes, para a frase ser a de tempo e não a genérica. */
const ESPERA_MAX_MS = 10 * 60_000 - 2_000

/** Lê o estado da conta no Hub; a renovação que faltar por pouco o Hub mesmo resolve antes de responder. */
export const lerEstadoChatGPT = (client: HostClient) =>
  // A leitura pode renovar o access no caminho (um pedido ao emissor de até 15 s): o prazo cobre isso com folga.
  client.call<EstadoChatGPT>('chatgptEstado', {}, 30_000)

/** Sai: o Hub revoga o que der e apaga os tokens deste computador de qualquer jeito. */
export const sairDoChatGPT = (client: HostClient) =>
  client.call<{ ok: true; revogado: boolean }>('chatgptSair', {}, PRAZO_MS)

/**
 * O login inteiro: liga o ouvinte do app, pede a URL ao Hub (com `consentir` quando a pessoa escolhe ativar o uso
 * do plano), espera o navegador voltar, entrega o retorno ao Hub (que confere o state, consome a tentativa, troca o
 * código e guarda tudo no cofre) e mostra o desfecho ao navegador. `cancelar` desiste enquanto o navegador não voltou:
 * desliga o ouvinte e a espera termina como «cancelado»; depois do retorno (`aoConcluir`) ele não faz mais nada.
 */
export function entrarComChatGPT(
  client: HostClient,
  ouvinte: PonteDoOuvinte,
  consentir: boolean,
  space: string,
  novaConta = false,
  esperaMs = ESPERA_MAX_MS,
  aoConcluir?: () => void,
) {
  let tentativa: number | null = null
  let cancelado = false
  let concluindo = false
  const fim = (async (): Promise<DesfechoChatGPT> => {
    const aberto = await ouvinte.abrir().catch(() => null)
    if (!aberto) throw new FalhaChatGPT('falha')
    tentativa = aberto.tentativa
    try {
      if (cancelado) throw new FalhaChatGPT('cancelado')
      let url: string
      try {
        url = (await client.call<{ url: string }>('chatgptEntrar',
          { loopbackPort: aberto.porta, spaceId: space, ...(consentir ? { consentir: true } : {}), ...(novaConta ? { novaConta: true } : {}) }, 15_000)).url
      } catch { throw new FalhaChatGPT('falha') }
      if (cancelado) throw new FalhaChatGPT('cancelado')
      let temporizador: ReturnType<typeof setTimeout> | undefined
      const espera = ouvinte.esperar(aberto.tentativa, url)
      const prazo = new Promise<never>((_, recusar) => { temporizador = setTimeout(() => recusar(new FalhaChatGPT('tempo')), esperaMs) })
      // Quem perder a corrida rejeita depois, sem ninguém esperando: não pode derrubar o processo.
      espera.catch(() => {})
      prazo.catch(() => {})
      let volta: string
      try {
        volta = await Promise.race([espera, prazo])
      } catch (e) {
        if (e instanceof FalhaChatGPT) throw e
        throw new FalhaChatGPT(cancelado ? 'cancelado' : 'falha')
      } finally {
        clearTimeout(temporizador)
      }
      // O retorno pode já estar em trânsito quando o Cancelar foi clicado: o login para aqui e o Hub nunca grava.
      if (cancelado) throw new FalhaChatGPT('cancelado')
      let retorno: ReturnType<typeof lerRetornoLogin>
      try { retorno = lerRetornoLogin(volta) } catch { throw new FalhaChatGPT('falha') }
      // Daqui em diante o Hub troca o código e grava a conta: não há mais o que cancelar.
      concluindo = true
      aoConcluir?.()
      // O cliente emitido volta no retorno do primeiro registro; repetido ou vazio, o retorno é recusado.
      const parametros = new URL(volta).searchParams
      const clientes = parametros.getAll('client_id')
      if (clientes.length > 1 || clientes[0] === '') throw new FalhaChatGPT('falha')
      // O erro do fornecedor vale antes de qualquer código no mesmo retorno. O Hub classifica o
      // código `error`; a descrição que o acompanha (lida como `retorno.error`) não o substitui.
      const corpo = retorno.error !== null
        ? { state: retorno.state, erro: parametros.get('error') ?? retorno.error, spaceId: space }
        : { state: retorno.state, code: retorno.code ?? '', spaceId: space, ...(clientes.length === 1 ? { client_id: clientes[0] } : {}) }
      let desfecho: { ok: boolean; conta?: { rotulo: string | null; planoHabilitado: boolean }; erro?: string; avisoAnteriorSemConfirmacao?: boolean }
      try {
        desfecho = await client.call('chatgptEntrarConcluir', corpo, PRAZO_MS)
      } catch { throw new FalhaChatGPT('falha') }
      ouvinte.responder(aberto.tentativa, desfecho.ok ? { ok: true, nome: 'ChatGPT' } : { ok: false })
      if (!desfecho.ok && desfecho.erro === 'entrada_negada') throw new FalhaChatGPT('cancelado')
      return desfecho.ok
        ? {
            ok: true,
            rotulo: desfecho.conta?.rotulo ?? null,
            planoHabilitado: desfecho.conta?.planoHabilitado === true,
            // Só cabe na troca de conta: o Hub marca qualquer encerramento sem confirmação, inclusive
            // na reautorização da mesma conta, em que ninguém foi substituído.
            ...(novaConta && desfecho.avisoAnteriorSemConfirmacao === true ? { avisoAnteriorSemConfirmacao: true } : {}),
          }
        : { ok: false, erro: desfecho.erro ?? '' }
    } finally {
      ouvinte.fechar(aberto.tentativa)
    }
  })()
  const cancelar = () => {
    if (concluindo) return
    cancelado = true
    if (tentativa !== null) ouvinte.fechar(tentativa)
  }
  return { fim, cancelar }
}
