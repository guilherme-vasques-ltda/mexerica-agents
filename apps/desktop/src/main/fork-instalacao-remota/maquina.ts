// MEX-1307 F0 (plano §3): máquina de estados pura da instalação remota. Sem I/O, sem SSH, sem senha (a senha nunca
// entra aqui: só "foi enviada" e "foi recusada"). Fail-closed: evento fora de hora é RECUSADO e o estado não muda.
// Regras que os testes provam:
//   - nada é gravado no servidor antes de uma aprovação do Resumo para o `H` atual (`podeGravarNoServidor`);
//   - toda retomada volta pelo Resumo e pede nova aprovação, haja ou não mudança (`H` só explica);
//   - sem comparação da impressão digital, ou com chave diferente da gravada = Bloqueada;
//   - `sudo -n` sem senha dispensa a tela de senha (V6); com senha, até 3 tentativas por fase;
//   - servidor protegido (D6) nunca recebe instalar nem desinstalar; Host existente só conecta ou desinstala.
import { calcularH, diferencas, ehFingerprint, type CampoConsentimento, type EntradaConsentimento } from './consentimento.ts'
import { ehIdTentativa, type Classe812 } from './esquemas.ts'
import type { MotivoIncompativel, ResultadoSondagem } from './sondagem.ts'

export const ESTADOS = [
  'Nova', 'Alcancando', 'ConfirmandoChave', 'Entrando', 'Verificando', 'SondandoRoot', 'Incompativel', 'Bloqueada', 'Resumo',
  'PedindoSenha', 'SenhaErrada', 'Enviando', 'ConferindoRoot', 'Instalando', 'Saude', 'Conectando', 'Pronta', 'Interrompida',
  'PacoteRuim', 'Falhou', 'Limpa', 'Parcial', 'IntervencaoManual', 'Desinstalando', 'Desinstalada', 'DesinstalacaoParcial', 'Cancelada',
] as const
export type Estado = (typeof ESTADOS)[number]

export type Acao = 'instalar' | 'desinstalar' | 'conectar'
export type FasePedido = 'sondagem' | 'instalacao'
export const MAX_TENTATIVAS_SENHA = 3

export type MotivoBloqueio = 'sem-comparacao' | 'chave-mudou'

export interface Resumo {
  h: string
  entrada: EntradaConsentimento
  /** Só numa retomada: o que mudou desde a tentativa anterior. Vazio = "nada mudou" (e mesmo assim pede aprovação). */
  diferencas: CampoConsentimento[]
  acoes: Acao[]
}

export interface Tentativa {
  estado: Estado
  id: string
  objetivo: 'instalar' | 'desinstalar'
  /** Servidor marcado como protegido (D6), como a produção. */
  protegido: boolean
  /** Impressões digitais já gravadas no `ssh_known_hosts` do app para este servidor, ou null. */
  chaveGravada: readonly string[] | null
  chavesApresentadas: readonly string[]
  chavesAceitas: readonly string[]
  retomada: boolean
  sondagem: ResultadoSondagem | null
  classe812: Classe812 | null
  resumo: Resumo | null
  /** H da tentativa anterior e a entrada correspondente (só para explicar diferenças). */
  entradaAnterior: EntradaConsentimento | null
  aprovadoH: string | null
  acaoAprovada: Acao | null
  fasePedido: FasePedido | null
  senhaEnviada: { sondagem: boolean; instalacao: boolean }
  senhaErros: { sondagem: number; instalacao: number }
  /** Senhas já entregues ao `stdin` por fase (limite de 3 vale por fase, mesmo passando pelo Resumo de novo). */
  senhaEnvios: { sondagem: number; instalacao: number }
  motivoBloqueio: MotivoBloqueio | null
  motivosIncompativel: readonly MotivoIncompativel[]
  /** O app já pode ter gravado algo no servidor nesta tentativa (id). Só some com limpeza comprovada ou conclusão íntegra; sobrevive a queda, cancelamento e retomada. */
  efeitosPendentes: boolean
}

export type Evento =
  | { t: 'iniciar' }
  | { t: 'alcancou'; chaves: readonly string[] }
  | { t: 'comparacao'; comparou: boolean }
  | { t: 'entrou' }
  | { t: 'sondagemA'; resultado: ResultadoSondagem }
  | { t: 'senhaEnviada' }
  | { t: 'senhaRecusada' }
  | { t: 'tentarDeNovo' }
  | { t: 'sondagemRoot'; entrada: EntradaConsentimento }
  | { t: 'sondagemRootIlegivel' }
  | { t: 'aprovar'; h: string; acao: Acao }
  | { t: 'transferenciaCompleta' }
  | { t: 'pacoteRecusado' }
  | { t: 'autenticado' }
  | { t: 'instalado' }
  | { t: 'faseBFalhou'; tocouNo812: boolean }
  | { t: 'limpeza'; completa: boolean }
  | { t: 'saudeOk' }
  | { t: 'saudeFalhou' }
  | { t: 'conectou' }
  | { t: 'desinstalar' }
  | { t: 'desinstalado'; completo: boolean }
  | { t: 'queda' }
  | { t: 'retomar' }
  | { t: 'cancelar' }

export type Recusa = 'fora-de-hora' | 'aguarde-fase-b' | 'dado-invalido' | 'h-diferente' | 'acao-nao-permitida' | 'sem-tentativas'
export interface Resultado { tentativa: Tentativa; recusa: Recusa | null }

export function novaTentativa(id: string, opcoes: { protegido: boolean; chaveGravada: readonly string[] | null }): Tentativa {
  if (!ehIdTentativa(id)) throw new RangeError('id-tentativa-invalido')
  return {
    estado: 'Nova', id, objetivo: 'instalar', protegido: opcoes.protegido, chaveGravada: opcoes.chaveGravada,
    chavesApresentadas: [], chavesAceitas: [], retomada: false, sondagem: null, classe812: null, resumo: null, entradaAnterior: null,
    aprovadoH: null, acaoAprovada: null, fasePedido: null,
    senhaEnviada: { sondagem: false, instalacao: false }, senhaErros: { sondagem: 0, instalacao: 0 }, senhaEnvios: { sondagem: 0, instalacao: 0 },
    motivoBloqueio: null, motivosIncompativel: [], efeitosPendentes: false,
  }
}

/** Estados em que o app já pode estar alterando o servidor. Só valem com aprovação do `H` atual. */
const GRAVA_NO_SERVIDOR: ReadonlySet<Estado> = new Set(['Enviando', 'ConferindoRoot', 'Instalando', 'Desinstalando'])
/** Estado em que o cancelamento é recusado: "aguarde, terminando com segurança". */
const FASE_B: ReadonlySet<Estado> = new Set(['ConferindoRoot', 'Instalando'])
/** Cancelar aqui não é "nada alterado": o envio, a instalação ou a desinstalação já pode ter deixado algo no servidor. */
const APOS_EFEITOS: ReadonlySet<Estado> = new Set(['Enviando', 'Saude', 'Conectando'])
/** Entrar aqui marca `efeitosPendentes` (fail-closed: já pode ter gravado). */
const MARCA_EFEITOS: ReadonlySet<Estado> = new Set(['Enviando', 'ConferindoRoot', 'Instalando', 'Desinstalando', 'Saude'])
/** Estados que encerram a tentativa com o servidor comprovadamente limpo ou íntegro: apagam `efeitosPendentes`. */
const LIMPA_EFEITOS: ReadonlySet<Estado> = new Set(['Limpa', 'Desinstalada', 'Pronta'])
/** Estados ativos em que uma queda de rede ou do processo leva a Interrompida. */
const ATIVOS: ReadonlySet<Estado> = new Set([
  'Alcancando', 'ConfirmandoChave', 'Entrando', 'Verificando', 'SondandoRoot', 'Resumo', 'PedindoSenha', 'SenhaErrada',
  'Enviando', 'ConferindoRoot', 'Instalando', 'Saude', 'Conectando', 'Desinstalando',
])

export const podeGravarNoServidor = (t: Tentativa) =>
  GRAVA_NO_SERVIDOR.has(t.estado) && t.aprovadoH !== null && t.resumo !== null && t.aprovadoH === t.resumo.h && t.acaoAprovada !== null && t.acaoAprovada !== 'conectar'

/** O que o Resumo oferece. Protegido: nunca instalar nem desinstalar; Host existente: só conectar ou desinstalar. */
export function acoesPermitidas(t: Tentativa, s: ResultadoSondagem): Acao[] {
  const existe = s.hostInstalado === 'presente'
  const podeDesinstalar = existe && !t.protegido && s.contaHub !== 'desconhecido'
  if (t.objetivo === 'desinstalar') return podeDesinstalar ? ['desinstalar'] : []
  if (existe) return ['conectar', ...(podeDesinstalar ? (['desinstalar'] as const) : [])]
  return t.protegido ? [] : ['instalar']
}

const ok = (tentativa: Tentativa): Resultado => ({ tentativa, recusa: null })
const recusar = (tentativa: Tentativa, recusa: Recusa): Resultado => ({ tentativa, recusa })
const para = (t: Tentativa, estado: Estado, mudanca: Partial<Tentativa> = {}): Resultado => {
  const antes = mudanca.efeitosPendentes ?? t.efeitosPendentes
  const efeitosPendentes = LIMPA_EFEITOS.has(estado) ? false : MARCA_EFEITOS.has(estado) ? true : antes
  return ok({ ...t, ...mudanca, estado, efeitosPendentes })
}
/** Cancelamento com possível efeito no servidor: nunca "nada alterado". Desinstalação vira parcial; o resto, interrupção. */
const interromper = (t: Tentativa): Resultado =>
  para(t, t.estado === 'Desinstalando' || t.objetivo === 'desinstalar' ? 'DesinstalacaoParcial' : 'Interrompida', { aprovadoH: null, acaoAprovada: null, fasePedido: null })

/** Depois do Resumo aprovado: senha na tela (só se o `sudo` pede senha), ou direto à ação. */
function seguirAposAprovacao(t: Tentativa): Resultado {
  const sudoPedeSenha = t.sondagem?.sudo === 'pede-senha'
  if (t.acaoAprovada === 'conectar') return para(t, 'Conectando')
  if (sudoPedeSenha) return para(t, 'PedindoSenha', { fasePedido: 'instalacao' })
  return para(t, t.acaoAprovada === 'desinstalar' ? 'Desinstalando' : 'Enviando')
}

export function aplicar(t: Tentativa, e: Evento): Resultado {
  // Cancelar vale em todos os estados ativos, menos na fase B.
  if (e.t === 'cancelar') {
    if (FASE_B.has(t.estado)) return recusar(t, 'aguarde-fase-b')
    if (!ATIVOS.has(t.estado) && t.estado !== 'Nova') return recusar(t, 'fora-de-hora')
    // Antes de qualquer gravação: cancelar a senha da instalação ou o erro de senha volta ao Resumo, nada foi alterado.
    // Com efeitos pendentes (a tentativa já gravou, mesmo em retomada), é sempre interrupção a verificar.
    if (t.efeitosPendentes || t.estado === 'Desinstalando') return interromper(t)
    if ((t.estado === 'PedindoSenha' || t.estado === 'SenhaErrada') && t.fasePedido === 'instalacao') return para(t, 'Resumo', { aprovadoH: null, acaoAprovada: null, fasePedido: null })
    if (APOS_EFEITOS.has(t.estado)) return interromper(t)
    return para(t, 'Cancelada', { aprovadoH: null, acaoAprovada: null, fasePedido: null })
  }
  if (e.t === 'queda') return ATIVOS.has(t.estado) ? para(t, 'Interrompida', { fasePedido: null, aprovadoH: null, acaoAprovada: null }) : recusar(t, 'fora-de-hora')
  // Retomada: nunca reaproveita aprovação, sondagem nem senha; volta a Alcançando e ao Resumo. Contadores de senha e
  // `efeitosPendentes` pertencem à tentativa (id) e continuam.
  if (e.t === 'retomar') {
    if (t.estado !== 'Interrompida' && t.estado !== 'DesinstalacaoParcial') return recusar(t, 'fora-de-hora')
    return para(t, 'Alcancando', {
      retomada: true, objetivo: t.estado === 'DesinstalacaoParcial' ? 'desinstalar' : t.objetivo,
      entradaAnterior: t.resumo?.entrada ?? t.entradaAnterior, resumo: null, sondagem: null, classe812: null,
      aprovadoH: null, acaoAprovada: null, fasePedido: null, chavesApresentadas: [], chavesAceitas: [],
      senhaEnviada: { sondagem: false, instalacao: false },
      motivoBloqueio: null, motivosIncompativel: [],
    })
  }
  if (e.t === 'desinstalar') {
    if (t.estado !== 'Pronta') return recusar(t, 'fora-de-hora')
    if (t.protegido) return recusar(t, 'acao-nao-permitida')
    return para(t, 'Alcancando', { objetivo: 'desinstalar', retomada: false, resumo: null, sondagem: null, classe812: null, aprovadoH: null, acaoAprovada: null, chavesApresentadas: [], chavesAceitas: [], senhaEnviada: { sondagem: false, instalacao: false }, senhaErros: { sondagem: 0, instalacao: 0 }, senhaEnvios: { sondagem: 0, instalacao: 0 } })
  }

  switch (t.estado) {
    case 'Nova':
      return e.t === 'iniciar' ? para(t, 'Alcancando') : recusar(t, 'fora-de-hora')
    case 'Alcancando': {
      if (e.t !== 'alcancou') break
      if (e.chaves.length < 1 || e.chaves.length > 8 || !e.chaves.every(ehFingerprint)) return recusar(t, 'dado-invalido')
      const gravada = t.chaveGravada
      if (gravada !== null) {
        // Chave diferente da gravada: Bloqueada, nunca sobrescreve. Igual: o dono já a confirmou antes.
        const igual = gravada.length === e.chaves.length && gravada.every((f) => e.chaves.includes(f))
        return igual ? para(t, 'Entrando', { chavesApresentadas: e.chaves, chavesAceitas: e.chaves }) : para(t, 'Bloqueada', { chavesApresentadas: e.chaves, motivoBloqueio: 'chave-mudou' })
      }
      return para(t, 'ConfirmandoChave', { chavesApresentadas: e.chaves })
    }
    case 'ConfirmandoChave':
      if (e.t !== 'comparacao') break
      // D3: sem comparação por outro canal = bloqueado (sem desvio nem TOFU).
      return e.comparou ? para(t, 'Entrando', { chavesAceitas: t.chavesApresentadas }) : para(t, 'Bloqueada', { motivoBloqueio: 'sem-comparacao' })
    case 'Entrando':
      return e.t === 'entrou' ? para(t, 'Verificando') : recusar(t, 'fora-de-hora')
    case 'Verificando': {
      if (e.t !== 'sondagemA') break
      const s = e.resultado
      if (!s.compativel || s.sudo === 'indisponivel') return para(t, 'Incompativel', { sondagem: s, motivosIncompativel: s.motivos })
      // V6: com `sudo -n` já aprovado não se abre a tela de senha.
      if (s.sudo === 'pede-senha') return t.senhaEnvios.sondagem >= MAX_TENTATIVAS_SENHA ? recusar(t, 'sem-tentativas') : para(t, 'PedindoSenha', { sondagem: s, fasePedido: 'sondagem' })
      return para(t, 'SondandoRoot', { sondagem: s })
    }
    case 'PedindoSenha':
      if (e.t !== 'senhaEnviada' || t.fasePedido === null) break
      // Limite de 3 senhas por fase também na entrega: sair e voltar pelo Resumo não zera a conta.
      if (t.senhaEnvios[t.fasePedido] >= MAX_TENTATIVAS_SENHA) return recusar(t, 'sem-tentativas')
      const envios = { ...t.senhaEnvios, [t.fasePedido]: t.senhaEnvios[t.fasePedido] + 1 }
      if (t.fasePedido === 'sondagem') return para(t, 'SondandoRoot', { senhaEnviada: { ...t.senhaEnviada, sondagem: true }, senhaEnvios: envios, fasePedido: null })
      return para(t, t.acaoAprovada === 'desinstalar' ? 'Desinstalando' : 'Enviando', { senhaEnviada: { ...t.senhaEnviada, instalacao: true }, senhaEnvios: envios, fasePedido: null })
    case 'SenhaErrada':
      if (e.t !== 'tentarDeNovo' || t.fasePedido === null) break
      return t.senhaErros[t.fasePedido] >= MAX_TENTATIVAS_SENHA || t.senhaEnvios[t.fasePedido] >= MAX_TENTATIVAS_SENHA ? recusar(t, 'sem-tentativas') : para(t, 'PedindoSenha')
    case 'SondandoRoot': {
      if (e.t === 'senhaRecusada') return recusarSenha(t, 'sondagem')
      if (e.t === 'sondagemRootIlegivel') return para(t, 'Incompativel', { classe812: 'ilegivel', motivosIncompativel: ['estado-ilegivel'] })
      if (e.t !== 'sondagemRoot' || t.sondagem === null) break
      // Com senha exigida, a sondagem privilegiada só é válida depois de a senha ter sido enviada.
      if (t.sondagem.sudo === 'pede-senha' && !t.senhaEnviada.sondagem) return recusar(t, 'fora-de-hora')
      const classe = e.entrada.estado.classe812
      if (classe === 'ilegivel') return para(t, 'Incompativel', { classe812: classe, motivosIncompativel: ['estado-ilegivel'] })
      // `incompleto` = o #812 deixou estado administrativo incompleto: só diagnóstico, nada é alterado (§4.8).
      if (classe === 'incompleto') return para(t, 'IntervencaoManual', { classe812: classe })
      const h = calcularH(e.entrada)
      const acoes = acoesPermitidas(t, t.sondagem)
      const dif = t.retomada && t.entradaAnterior ? diferencas(t.entradaAnterior, e.entrada) : []
      // Aprovação anterior nunca vale: o Resumo sempre nasce sem aprovação.
      return para(t, 'Resumo', { classe812: classe, resumo: { h, entrada: e.entrada, diferencas: dif, acoes }, aprovadoH: null, acaoAprovada: null })
    }
    case 'Resumo': {
      if (e.t !== 'aprovar' || t.resumo === null) break
      if (e.h !== t.resumo.h) return recusar(t, 'h-diferente')
      if (!t.resumo.acoes.includes(e.acao)) return recusar(t, 'acao-nao-permitida')
      // Já gastou as 3 senhas desta tentativa: aprovar de novo não abre outro pedido de senha.
      if (e.acao !== 'conectar' && t.sondagem?.sudo === 'pede-senha' && t.senhaEnvios.instalacao >= MAX_TENTATIVAS_SENHA) return recusar(t, 'sem-tentativas')
      return seguirAposAprovacao({ ...t, aprovadoH: e.h, acaoAprovada: e.acao, objetivo: e.acao === 'conectar' ? t.objetivo : e.acao })
    }
    case 'Enviando':
      return e.t === 'transferenciaCompleta' ? para(t, 'ConferindoRoot') : recusar(t, 'fora-de-hora')
    case 'ConferindoRoot':
      if (e.t === 'senhaRecusada') return recusarSenha(t, 'instalacao')
      if (e.t === 'autenticado') return para(t, 'Instalando')
      if (e.t === 'pacoteRecusado') return para(t, 'PacoteRuim')
      if (e.t === 'faseBFalhou') return para(t, e.tocouNo812 ? 'IntervencaoManual' : 'Falhou')
      break
    case 'Instalando':
      if (e.t === 'instalado') return para(t, 'Saude')
      // Falha depois que o #812 começou: sem retomada automática (§4.8).
      if (e.t === 'faseBFalhou') return para(t, e.tocouNo812 ? 'IntervencaoManual' : 'Falhou')
      break
    case 'Saude':
      if (e.t === 'saudeOk') return para(t, 'Conectando')
      if (e.t === 'saudeFalhou') return para(t, 'Falhou')
      break
    case 'Conectando':
      return e.t === 'conectou' ? para(t, 'Pronta') : recusar(t, 'fora-de-hora')
    case 'PacoteRuim':
    case 'Falhou':
      return e.t === 'limpeza' ? para(t, e.completa ? 'Limpa' : 'Parcial') : recusar(t, 'fora-de-hora')
    case 'Desinstalando':
      // Senha recusada na desinstalação: mesma transição, contagem e nova tentativa da instalação.
      if (e.t === 'senhaRecusada') return recusarSenha(t, 'instalacao')
      return e.t === 'desinstalado' ? para(t, e.completo ? 'Desinstalada' : 'DesinstalacaoParcial') : recusar(t, 'fora-de-hora')
    default:
      break
  }
  return recusar(t, 'fora-de-hora')
}

function recusarSenha(t: Tentativa, fase: FasePedido): Resultado {
  return para(t, 'SenhaErrada', { fasePedido: fase, senhaErros: { ...t.senhaErros, [fase]: t.senhaErros[fase] + 1 } })
}
