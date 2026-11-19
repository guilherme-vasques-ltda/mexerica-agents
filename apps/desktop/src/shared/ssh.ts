// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
// SSH computers (spec §11.4): profiles, tunnel status and the computers they attach.
// The main process runs OpenSSH; the renderer shows the state and attaches a store per tunnel.

/** A computer reached over SSH. Nothing secret: keys stay in ssh-agent or on disk. */
export interface SSHProfile {
  id: string
  /** An `~/.ssh/config` alias or a hostname. */
  host: string
  port: number | null
  user: string | null
  identityFile: string | null
  remotePort: number
  /** Remembered after the first connection; a different computer behind the same address is refused. */
  computerId: string | null
  name: string
}

/**
 * D454 (fatia 4): o porquê de um passo ou falha, para a tela mostrar no idioma escolhido. `chave` é uma chave do
 * catálogo (lib/fork-i18n) e `params`, os seus parâmetros; `message`/`step` levam o mesmo texto em inglês.
 */
export interface SSHMotivo { chave: string; params?: Record<string, string | number> }

export type SSHStatus =
  | { kind: 'idle' }
  | { kind: 'connecting'; step: string; motivo?: SSHMotivo }
  /** First time: the user compares these fingerprints before anything connects. */
  | { kind: 'confirmHostKey'; name: string; fingerprints: string[] }
  | { kind: 'connected'; localPort: number }
  /** A retry is scheduled. */
  | { kind: 'retrying'; message: string; motivo?: SSHMotivo }
  /** Needs the user (settings, the remote side, or a security problem); nothing retries. */
  | { kind: 'failed'; message: string; motivo?: SSHMotivo }
  | { kind: 'notInstalled' }

/** A connected tunnel: the computer behind it, as a loopback host. */
export interface SSHAttachment {
  profileId: string
  computer: { id: string; name: string; signKey: string; boxKey: string | null }
  baseURL: string
  token: string
}

export interface SSHState {
  profiles: SSHProfile[]
  status: Record<string, SSHStatus>
  attachments: SSHAttachment[]
}

/**
 * D76/D293: o Hub da Mexerica (alias do ~/.ssh/config), criado na primeira abertura só se o alias existir; sempre sugerido ao adicionar.
 * Valor padrão genérico; o main aceita outro alias por `MEXERICA_HUB_SSH_ALIAS` (main/fork-ssh-semente/hub-alias.ts).
 */
export const DONNABOT_HUB_HOST = 'mexerica-hub'

export const newSSHProfile = (remotePort = 19422): SSHProfile => ({
  id: crypto.randomUUID(),
  host: '',
  port: null,
  user: null,
  identityFile: null,
  remotePort,
  computerId: null,
  name: '',
})

export interface SSHBridge {
  state(): Promise<SSHState>
  onChange(cb: (s: SSHState) => void): () => void
  /** Adds or replaces the profile; resolves with a problem to show, or null once saved. */
  /** O problema do perfil (D454: o motivo, para a tela traduzir), ou null quando salvou. */
  save(profile: SSHProfile): Promise<SSHMotivo | null>
  remove(id: string): void
  connect(id: string): void
  disconnect(id: string): void
  trustHostKey(id: string): void
  /** The system file dialog; the chosen path, or null. */
  chooseKey(): Promise<string | null>
}
