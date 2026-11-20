// o atalho que abre ou encerra a MESMA chamada de voz em tempo real do botão.
// Módulo puro (sem Electron nem DOM), compartilhado entre o processo principal e a tela. O atalho não cria uma
// sessão própria: termina em `iniciar` ou `encerrar` da chamada que o botão já abre (CallView).

export interface ContextoEntradaVoz {
  /** Há uma conversa de bot montada e a pessoa a vê (nenhuma tela de Ajustes, Rotinas… ou camada na frente). */
  conversaVisivel: boolean
  grupo: boolean
  /** A tela grande do WhatsApp ou o painel direito flutuante cobre a conversa: os controles da chamada ficariam escondidos. */
  coberta: boolean
  /** Uma aba de sessão está aberta: a chamada é sempre da conversa principal do bot. */
  sessaoAberta: boolean
  /** A mesma disponibilidade que habilita o botão (chave, Hub, modo). */
  disponivel: boolean
  /** O aviso de senha colada barra o envio neste bot. */
  senhaBloqueada: boolean
  emChamada: boolean
}

export type MotivoRecusaVoz = 'oculta' | 'grupo' | 'sessao' | 'senha' | 'indisponivel'

export type DecisaoEntradaVoz =
  | { acao: 'iniciar' }
  | { acao: 'encerrar' }
  | { acao: 'recusar'; motivo: MotivoRecusaVoz }

/**
 * A regra do atalho. Já em chamada, ele encerra (sempre, mesmo com a conversa fora de vista: desligar o microfone nunca é
 * recusado); o botão da conversa não abre uma segunda sessão porque some durante a chamada. Fora de chamada, só inicia na
 * conversa principal do bot que a pessoa está vendo.
 */
export function decidirEntradaVoz(c: ContextoEntradaVoz): DecisaoEntradaVoz {
  if (c.emChamada) return { acao: 'encerrar' }
  if (!c.conversaVisivel || c.coberta) return { acao: 'recusar', motivo: 'oculta' }
  if (c.grupo) return { acao: 'recusar', motivo: 'grupo' }
  if (c.sessaoAberta) return { acao: 'recusar', motivo: 'sessao' }
  if (c.senhaBloqueada) return { acao: 'recusar', motivo: 'senha' }
  if (!c.disponivel) return { acao: 'recusar', motivo: 'indisponivel' }
  return { acao: 'iniciar' }
}

export type Plataforma = 'darwin' | 'win32' | 'linux'

export type ResultadoAtalho = { ok: true; acelerador: string } | { ok: false; motivo: 'invalido' | 'reservado' }

const MODIFICADORES: Record<string, 'CommandOrControl' | 'Command' | 'Control' | 'Alt' | 'Shift'> = {
  commandorcontrol: 'CommandOrControl', cmdorctrl: 'CommandOrControl',
  command: 'Command', cmd: 'Command',
  control: 'Control', ctrl: 'Control',
  alt: 'Alt', option: 'Alt',
  shift: 'Shift',
}
const ORDEM = ['Command', 'Control', 'Alt', 'Shift'] as const

/** Combinações que o app (menus, Ilha, edição) ou o sistema já usam, com `CommandOrControl` resolvido por plataforma. */
const RESERVADOS = [
  'Control+CommandOrControl+S', // barra lateral
  'CommandOrControl+Shift+K', // seletor de Spaces (renderer/lib/fork-space-shortcuts.ts)
  'CommandOrControl+Shift+Z', // refazer
  'CommandOrControl+Shift+R', // recarregar (desenvolvimento)
  'CommandOrControl+Alt+I', // ferramentas do desenvolvedor
  'CommandOrControl+Shift+3', 'CommandOrControl+Shift+4', 'CommandOrControl+Shift+5', // capturas de tela do macOS
  'CommandOrControl+Shift+Q', 'Control+CommandOrControl+Q', // sair da sessão / bloquear (macOS)
]

const resolver = (mods: string[], plataforma: Plataforma) => mods.map((m) => (m === 'CommandOrControl' ? (plataforma === 'darwin' ? 'Command' : 'Control') : m))
const chave = (mods: string[], tecla: string, plataforma: Plataforma) => [...new Set(resolver(mods, plataforma))].sort().join('+') + '+' + tecla

/**
 * Valida e normaliza o atalho global. Exige pelo menos dois modificadores (ou um modificador com tecla F), porque um
 * atalho global de uma tecla só com ⌘, ⌃ ou ⌥ colide com a edição de texto, com os menus do app e com os caracteres
 * que ⌥ digita no Mac. Só letras, dígitos e F1–F24. Nunca devolve algo que o app ou a Ilha já usam.
 */
export function validarAtalho(entrada: unknown, plataforma: Plataforma): ResultadoAtalho {
  if (typeof entrada !== 'string' || entrada.length > 64) return { ok: false, motivo: 'invalido' }
  const partes = entrada.split('+').map((p) => p.trim())
  const tecla = partes.pop() ?? ''
  const mods: string[] = []
  for (const p of partes) {
    const m = Object.hasOwn(MODIFICADORES, p.toLowerCase()) ? MODIFICADORES[p.toLowerCase()] : undefined
    if (!m || mods.includes(m)) return { ok: false, motivo: 'invalido' }
    mods.push(m)
  }
  const teclaFn = /^F([1-9]|1\d|2[0-4])$/i.test(tecla)
  if (!teclaFn && !/^[A-Za-z0-9]$/.test(tecla)) return { ok: false, motivo: 'invalido' }
  const reais = new Set(resolver(mods, plataforma))
  if (!reais.size || (reais.size === 1 && reais.has('Shift')) || (!teclaFn && reais.size < 2)) return { ok: false, motivo: 'invalido' }
  const t = tecla.toUpperCase()
  const entre = chave(mods, t, plataforma)
  if (RESERVADOS.some((r) => { const ps = r.split('+'); return chave(ps.slice(0, -1), ps.at(-1)!.toUpperCase(), plataforma) === entre })) return { ok: false, motivo: 'reservado' }
  const ordenados = ORDEM.filter((m) => reais.has(m))
  return { ok: true, acelerador: [...ordenados, t].join('+') }
}

/**
 * Pressões repetidas: cada evento, aceito ou não, renova a janela. Segurar a tecla (repetição do sistema) ou apertar
 * duas vezes seguidas não liga e desliga a chamada em sequência; só um novo toque depois da pausa vale.
 */
export function criarGuardaPressao(agora: () => number, janelaMs = 1200) {
  let ultimo = -Infinity
  return () => {
    const t = agora()
    const aceita = t - ultimo >= janelaMs
    ultimo = t
    return aceita
  }
}

/** O que a pessoa apertou ao gravar a combinação. Letra e dígito vêm de `key` (a tecla que o teclado em uso mostra, como em AZERTY); sem ela (⌥ no Mac digita outro caractere), vale a posição física (`code`). `null` até haver uma tecla de verdade. */
export function aceleradorDoEvento(e: { key?: string; code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }, plataforma: Plataforma): string | null {
  const impressa = e.key !== undefined && /^[A-Za-z0-9]$/.test(e.key) ? e.key.toUpperCase() : null
  const tecla = impressa ?? /^Key([A-Z])$/.exec(e.code)?.[1] ?? /^Digit(\d)$/.exec(e.code)?.[1] ?? (/^F\d{1,2}$/.test(e.code) ? e.code : null)
  if (!tecla || (e.metaKey && plataforma !== 'darwin')) return null
  return [e.metaKey && 'Command', e.ctrlKey && 'Control', e.altKey && 'Alt', e.shiftKey && 'Shift', tecla].filter(Boolean).join('+')
}

/** "CommandOrControl+Shift+K" como a pessoa lê: ⇧⌘K no Mac, Ctrl+Shift+K nos outros. */
export function rotuloAtalho(acelerador: string, plataforma: Plataforma): string {
  const partes = acelerador.split('+')
  const tecla = partes.pop() ?? ''
  const mods = partes.map((m) => (m === 'CommandOrControl' ? (plataforma === 'darwin' ? 'Command' : 'Control') : m))
  if (plataforma !== 'darwin') return [...mods.map((m) => (m === 'Control' ? 'Ctrl' : m)), tecla].join('+')
  const simbolo: Record<string, string> = { Control: '⌃', Alt: '⌥', Shift: '⇧', Command: '⌘' }
  return ['Control', 'Alt', 'Shift', 'Command'].filter((m) => mods.includes(m)).map((m) => simbolo[m]).join('') + tecla
}
