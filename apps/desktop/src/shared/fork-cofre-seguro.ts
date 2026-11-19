// MX-209 (fase 1 da MX-146): contrato do cartão seguro. Puro, sem Electron nem DOM, lido pelo main, pelos dois
// preloads e pelo renderer. Regra que este arquivo faz valer: nenhum tipo que volta ao chat carrega o valor.

export const cofreCanais = {
  /** Chat → main: "abra a janela segura para este pedido". Só a referência (ids); os textos da janela o main deriva da entrada no Hub. Nunca o valor. */
  abrir: 'fork-cofre-seguro:abrir',
  /** Chat → main (MX-277): "abra a janela segura VAZIA para guardar um login novo" (sem pedido do bot). Só o id do computador; nunca o valor. */
  abrirNovo: 'fork-cofre-seguro:abrir-novo',
  /** Main → chat: a janela abriu ou fechou. Sem valor. */
  estado: 'fork-cofre-seguro:estado',
  /** Janela segura → main: o que mostrar. */
  dados: 'fork-cofre-seguro:dados',
  /** Janela segura → main: o valor digitado. Único caminho do valor: janela → main → Hub. */
  salvar: 'fork-cofre-seguro:salvar',
  /** Janela segura → main: fechar sem guardar. */
  cancelar: 'fork-cofre-seguro:cancelar',
} as const

/**
 * `loginNovo` (MX-277): salvar um login sem pedido do bot, a partir do aviso de senha colada. A janela abre VAZIA: o
 * trecho escondido nunca sai do chat (nem por IPC); a pessoa cola a senha de novo, direto na janela segura.
 */
export type TipoPedidoSeguro = 'login' | 'secret' | 'loginNovo' | 'code'

/** O código de login (MX-286) tem de 4 a 10 dígitos, como o Hub aceita; a janela abre com 6 caixinhas. */
export const CODIGO_MIN = 4
export const CODIGO_MAX = 10
export const CODIGO_CAIXAS = 6
/** Só os dígitos de um texto (colar "123 456" ou "123-456" vira 123456), no máximo `CODIGO_MAX`. */
export const soDigitos = (texto: string): string => texto.replace(/\D/g, '').slice(0, CODIGO_MAX)
export const codigoValido = (texto: string): boolean => /^\d+$/.test(texto) && texto.length >= CODIGO_MIN && texto.length <= CODIGO_MAX

/** O "id do pedido" do login novo nos avisos de estado (não existe entrada no Hub); nunca aceito em `abrir`. */
export const ENTRY_LOGIN_NOVO = 'login-novo'

/** O que o cartão e a janela mostram sobre o pedido. Texto de exibição, sem segredo. */
export interface PedidoVisivel {
  tipo: TipoPedidoSeguro
  /** Título do pedido: o site (login) ou o nome do conector (chave). */
  titulo: string
  site: string | null
  /** Chave: o campo e onde entra (env/header). */
  campo: string | null
  local: string | null
  botNome: string
}

/** Qual Hub e qual pedido. O chat nunca manda endereço, token nem texto do pedido: o main resolve a
 * entrada no Hub e deriva dela tudo o que a janela mostra. `spaceId`/`conversationId`/`hubId` são só
 * roteamento, conferidos pelo próprio Hub. */
export interface AbrirCofre {
  computerId: string
  entryId: string
  /** Só quando o cartão vive num space; o Hub confere. */
  spaceId?: string
  hubId?: string
  conversationId?: string
}

/** Abrir a janela do login novo: SÓ o computador. Qualquer outro campo (um valor, por exemplo) recusa a chamada. */
export interface AbrirNovoCofre {
  computerId: string
}

/** Por que o main não abriu. Códigos estáveis; só o chat os traduz. */
export type MotivoRecusa = 'invalido' | 'semSuporte' | 'semConexao'
/** Resposta ao chat: sem valor, nunca. */
export type RespostaAbrir = { ok: true; foco: boolean } | { ok: false; motivo: MotivoRecusa }

export type EstadoJanela = 'aberta' | 'fechada' | 'salva'
export interface AvisoEstado { entryId: string; estado: EstadoJanela }

/** O que a janela segura recebe ao abrir (de novo, só texto de exibição). */
export interface DadosJanela { pedido: PedidoVisivel }

export interface EnvioSeguro {
  valor: string
  usuario: string
  rotulo: string
  /** Só no login novo: o site que a pessoa digitou na janela. */
  site?: string
}
/** `jaExiste` (MX-277, só login novo): o Hub recusou porque já há login para o mesmo site e usuário; nada foi alterado. */
export type ErroSalvar = 'vazio' | 'conexao' | 'recusado' | 'fechada' | 'jaExiste'
/** Resposta à janela segura: sem eco do valor. */
export type RespostaSalvar = { ok: true } | { ok: false; erro: ErroSalvar }

/** O olho "Mostrar" (MX-154): o valor aparece por no máximo 10 s e se esconde sozinho. */
export const OLHO_MS = 10_000

const LIMITE_VALOR = 64 * 1024
const LIMITE_LOGIN = 4096
const limpo = (texto: unknown, max: number): string | null =>
  typeof texto === 'string' ? texto.replace(/[\u0000-\u001f\u007f]/g, ' ').trim().slice(0, max) : null
const idCurto = (texto: unknown): string | null => {
  const v = limpo(texto, 129)
  return v && v.length <= 128 && v === texto ? v : null
}

/** Confere a referência do pedido que vem do chat (fora de confiança); `null` = recusa. */
export function lerAbrir(bruto: unknown): AbrirCofre | null {
  if (!bruto || typeof bruto !== 'object') return null
  const b = bruto as Record<string, unknown>
  const computerId = idCurto(b.computerId), entryId = idCurto(b.entryId)
  if (!computerId || !entryId) return null
  const spaceId = b.spaceId == null ? undefined : idCurto(b.spaceId)
  const hubId = b.hubId == null ? undefined : idCurto(b.hubId)
  const conversationId = b.conversationId == null ? undefined : idCurto(b.conversationId)
  if (spaceId === null || hubId === null || conversationId === null) return null
  return {
    computerId, entryId, ...(spaceId ? { spaceId } : {}), ...(hubId ? { hubId } : {}),
    ...(conversationId ? { conversationId } : {}),
  }
}

/** Confere o pedido de abrir a janela do login novo (vem do chat, fora de confiança): só `computerId`, nada mais. */
export function lerAbrirNovo(bruto: unknown): AbrirNovoCofre | null {
  if (!bruto || typeof bruto !== 'object' || Array.isArray(bruto)) return null
  const chaves = Object.keys(bruto)
  if (chaves.length !== 1 || chaves[0] !== 'computerId') return null
  const computerId = idCurto((bruto as Record<string, unknown>).computerId)
  return computerId ? { computerId } : null
}

/** Confere o envio da janela. Só o tamanho: o Hub decide o resto. `null` = recusa. */
export function lerEnvio(bruto: unknown, tipo: TipoPedidoSeguro): EnvioSeguro | null {
  if (!bruto || typeof bruto !== 'object') return null
  const b = bruto as Record<string, unknown>
  const { valor, usuario, rotulo } = b
  // Código de login (MX-286): só dígitos, 4 a 10 — o Hub confere de novo; aqui nem chega com outra coisa.
  if (tipo === 'code') {
    const codigo = typeof valor === 'string' ? valor.trim() : ''
    return codigoValido(codigo) ? { valor: codigo, usuario: '', rotulo: '' } : null
  }
  if (typeof valor !== 'string' || !valor || valor.length > (tipo === 'secret' ? LIMITE_VALOR : LIMITE_LOGIN)) return null
  if (usuario != null && (typeof usuario !== 'string' || usuario.length > 512)) return null
  if (rotulo != null && (typeof rotulo !== 'string' || rotulo.length > 200)) return null
  const base = { valor, usuario: typeof usuario === 'string' ? usuario : '', rotulo: typeof rotulo === 'string' ? rotulo : '' }
  if (tipo !== 'loginNovo') return base
  // Login novo: o site vem da janela (a pessoa digita); sem site não há o que guardar.
  const site = (b as Record<string, unknown>).site
  if (typeof site !== 'string' || !site.trim() || site.length > 253) return null
  return { ...base, site: site.trim() }
}

/** A ponte que o preload da janela segura expõe. Não existe `window.codync` nessa janela. */
export interface PonteCofre {
  dados(): Promise<DadosJanela | null>
  salvar(envio: EnvioSeguro): Promise<RespostaSalvar>
  cancelar(): void
}

/** A ponte que o preload geral expõe ao chat: abrir e ouvir o estado, nada mais. */
export interface PonteCofreChat {
  abrir(pedido: AbrirCofre): Promise<RespostaAbrir>
  /** MX-277: abre a janela segura vazia para guardar um login novo. Só o id do computador; o trecho colado nunca vai junto. */
  abrirNovo(pedido: AbrirNovoCofre): Promise<RespostaAbrir>
  onEstado(cb: (aviso: AvisoEstado) => void): () => void
}
