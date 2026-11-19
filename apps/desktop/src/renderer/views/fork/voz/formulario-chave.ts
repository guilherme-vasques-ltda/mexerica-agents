import type { HostClient } from '../../../client/host-client'
import { setVoiceKey, voiceModels, type VoiceProvider, type VoiceStatus } from '../../settings/voice-settings'
import { podeGerirNoHub, semChave } from './voz-model'
import type { Mensagem } from '../bot-settings/mensagem'

// voz-03: as ações da linha "Chave no Hub <nome>" (o formulário de antes, nas mesmas chamadas ao Hub).
// A chave digitada entra pelo campo da tela, sai do campo assim que é enviada e não volta em
// nenhum texto. setVoiceKey continua LOCAL_ONLY no Hub; aqui, nem é chamado fora do acesso local/SSH.

export interface HubDaChave {
  readonly isLoopback: boolean
  readonly connection: { kind: string }
  readonly hostName: string
}

/** Fatia 6b (D454, D507): o resultado como dado; a frase do Hub (sem a chave) vai como `texto`. */
export interface Resultado {
  ok: boolean
  partes: Mensagem[]
}

export interface Acoes {
  setVoiceKey: (c: HostClient, p: VoiceProvider, key: string) => Promise<VoiceStatus>
  voiceModels: typeof voiceModels
}

const reais: Acoes = { setVoiceKey, voiceModels }

const SEM_GESTAO: Resultado = { ok: false, partes: [{ chave: 'voz.ajustes.hub.semGestao' }] }
const semConexao = (): Resultado => ({ ok: false, partes: [{ chave: 'voz.ajustes.hub.semConexao' }] })
const erro = (e: unknown) => (e instanceof Error ? e.message : typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e))

/**
 * Guarda a chave no Hub. `limparCampo` é chamado antes de qualquer espera de rede: a chave não fica no
 * estado da tela enquanto o Hub confere com o provedor. O texto devolvido nunca contém a chave.
 */
export async function guardarChave(hub: HubDaChave, client: HostClient | null, p: VoiceProvider, digitada: string, limparCampo: () => void, acoes: Acoes = reais): Promise<Resultado & { status?: VoiceStatus }> {
  let chave = digitada.trim()
  limparCampo()
  if (!chave) return { ok: false, partes: [{ chave: 'voz.ajustes.resultado.cole' }] }
  if (!podeGerirNoHub(hub)) return SEM_GESTAO
  if (!client) return semConexao()
  try {
    const status = await acoes.setVoiceKey(client, p, chave)
    return { ok: true, partes: [{ chave: 'voz.ajustes.hub.guardou' }], status }
  } catch (e) {
    return { ok: false, partes: [{ texto: semChave(erro(e), chave) }] }
  } finally {
    chave = ''
  }
}

/** Limpar: apaga a chave deste provedor no Hub (o outro provedor fica). */
export async function limparChave(hub: HubDaChave, client: HostClient | null, p: VoiceProvider, acoes: Acoes = reais): Promise<Resultado & { status?: VoiceStatus }> {
  if (!podeGerirNoHub(hub)) return SEM_GESTAO
  if (!client) return semConexao()
  try {
    const status = await acoes.setVoiceKey(client, p, '')
    return { ok: true, partes: [{ chave: 'voz.ajustes.hub.apagou' }], status }
  } catch (e) {
    return { ok: false, partes: [{ texto: semChave(erro(e), '') }] }
  }
}

/** Testar: lista os modelos que a chave do Hub alcança. Não liga chamada, não gera áudio, não muda escolha. */
export async function testarChave(client: HostClient | null, p: VoiceProvider, acoes: Acoes = reais): Promise<Resultado> {
  if (!client) return semConexao()
  try {
    await acoes.voiceModels(client, p)
    return { ok: true, partes: [{ chave: 'voz.ajustes.resultado.funciona' }] }
  } catch (e) {
    return { ok: false, partes: [{ texto: semChave(erro(e), '') }] }
  }
}
