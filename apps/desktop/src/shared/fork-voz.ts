// voz-04 (plano §2): contrato da chave de voz guardada no cofre deste aparelho.
// A ponte só escreve: guardar, limpar e testar recebem a chave ou o provedor e devolvem só estado ou
// categoria. Não existe método que leia a chave; a credencial curta é a única coisa que volta.

// D382: a ElevenLabs saiu das integrações. Um `elevenlabs.bin` antigo fica no disco, ignorado; apagar é decisão do dono.
export const VOZ_PROVEDORES_APARELHO = ['openai', 'gemini', 'xai'] as const
export type VozProvedorAparelho = typeof VOZ_PROVEDORES_APARELHO[number]

/**
 * `guardada`: abre e o provedor não recusou nesta sessão. `travada`: o arquivo existe, mas o cofre do
 * sistema não abriu (bloqueado ou indisponível). `recusada`: abre, mas o provedor recusou a chave.
 * `incerta`: a gravação ou a leitura não foi confirmada. `limpeza-pendente`: remoção/sync não terminaram.
 * Esses dois estados bloqueiam a chave do aparelho e nunca passam sozinhos para a chave do Hub.
 */
export type SituacaoChave = 'ausente' | 'guardada' | 'travada' | 'recusada' | 'incerta' | 'limpeza-pendente'

export interface ChaveAparelho {
  situacao: SituacaoChave
  /** A pessoa escolheu "Usar a chave do Hub" com a chave travada ou recusada. Só até reiniciar, Trocar ou Limpar. */
  reserva: boolean
}

export interface EstadoChaves {
  /** `sem-cofre`: este computador não tem cofre seguro aceito; nada é gravado. */
  cofre: 'seguro' | 'sem-cofre'
  chaves: Record<'openai' | 'gemini', ChaveAparelho> & Partial<Record<'xai', ChaveAparelho>>
}

/**
 * Por que uma ação não deu certo, sem nada do provedor: `chave`/`saldo`/`limite`/`rede`/`provedor` vêm da
 * resposta (só o código), `cofre` é o cofre do sistema, `registro` é falha antes de escrever o marcador,
 * `cancelado` é uma resposta superada por Limpar/Trocar, `sem-cofre` é a falta de cofre aceito,
 * `sem-chave` é a falta de chave guardada, `invalida` é argumento fora do formato e `origem` é quem pediu.
 */
export type CategoriaChave = 'chave' | 'saldo' | 'limite' | 'rede' | 'provedor' | 'cofre' | 'registro' | 'cancelado' | 'sem-cofre' | 'sem-chave' | 'invalida' | 'origem'

export interface ResultadoChave {
  ok: boolean
  categoria: CategoriaChave | null
  estado: EstadoChaves
}

export type ResultadoCredencial = { ok: true; credential: string } | { ok: false; categoria: CategoriaChave }

export interface VozBridge {
  catalogoXai(): Promise<import('./fork-voz-xai.ts').ResultadoCatalogoXai>
  credencialXai(pedido: import('./fork-voz-xai.ts').PedidoXai): Promise<import('./fork-voz-xai.ts').ResultadoCredencialXai>
  conferirSessaoXai(negociacao: string, sessao: unknown): Promise<import('./fork-voz-xai.ts').ResultadoSessaoXai>
  fecharSessaoXai(negociacao: string): Promise<import('./fork-voz-xai.ts').ResultadoSessaoXai>
  estado(): Promise<EstadoChaves>
  /** Confere com o provedor e só então cifra e grava. Recusa não grava nada. */
  guardar(provider: VozProvedorAparelho, chave: string): Promise<ResultadoChave>
  /** Apaga o arquivo, sincroniza a pasta e descarta a cópia em memória. */
  limpar(provider: VozProvedorAparelho): Promise<ResultadoChave>
  /** Abre de novo a chave e confere com o provedor, sem abrir sessão de áudio (sem custo). */
  testar(provider: VozProvedorAparelho): Promise<ResultadoChave>
  /** "Usar a chave do Hub" enquanto a chave deste aparelho não abre. */
  usarReserva(provider: VozProvedorAparelho): Promise<ResultadoChave>
  /** Credencial curta de uma sessão do Tempo real, feita com a chave deste aparelho. */
  credencial(provider: VozProvedorAparelho, model: string, voice: string): Promise<ResultadoCredencial>
  /** O estado mudou no main. Sem dados: releia por `estado()`. Devolve a remoção. */
  onChange(listener: () => void): () => void
  /** registra (ou solta) o atalho global que abre/encerra a chamada. Não abre microfone; só pede ao chat. */
  definirAtalho(pedido: PedidoAtalhoVoz): Promise<ResultadoAtalhoVoz>
}

/** `ativo: false` solta o atalho; o `acelerador` é validado de novo no main (shared/fork-voz-entrada). */
export interface PedidoAtalhoVoz { ativo: boolean; acelerador: string }
export type ResultadoAtalhoVoz = { ok: true; acelerador: string | null } | { ok: false; motivo: 'invalido' | 'reservado' | 'em-uso' | 'indisponivel' }

export const ehProvedorAparelho = (v: unknown): v is VozProvedorAparelho => v === 'openai' || v === 'gemini' || v === 'xai'

/** Linux: só cofres reais conhecidos. `basic_text` (texto simples), `unknown` e outros ficam de fora. */
export const COFRES_LINUX_ACEITOS: readonly string[] = ['gnome_libsecret', 'kwallet', 'kwallet5', 'kwallet6']

/** Nomes de modelo e de voz que entram no pedido ao provedor: letras, dígitos, `-`, `.`, `_` (como o Hub). */
export const nomeValido = (s: unknown): s is string => typeof s === 'string' && s.length > 0 && s.length <= 100 && /^[A-Za-z0-9._-]+$/.test(s)

/** Mesmas famílias do Hub (`host/src/voice.rs`, `gemini_async_tools`): só as com ferramentas assíncronas confirmadas. */
const FAMILIAS_GEMINI = ['gemini-3.8-live', 'gemini-2.5-flash-native-audio', 'gemini-2.5-flash-live']
export const geminiComFerramentasAssincronas = (model: string) =>
  FAMILIAS_GEMINI.some((f) => model === f || model.startsWith(`${f}-`))

/** Tamanho máximo aceito para a chave digitada (as chaves reais têm bem menos). */
export const TAMANHO_MAXIMO_CHAVE = 512
/** Uma chave tem só caracteres visíveis, sem espaço. */
export const formatoDeChave = (s: string) => s.length > 0 && s.length <= TAMANHO_MAXIMO_CHAVE && /^[\x21-\x7e]+$/.test(s)
