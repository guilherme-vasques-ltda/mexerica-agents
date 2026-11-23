// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { SSHProfile } from '../shared/ssh.ts'
import { pacotesDoApp } from '../shared/fork-nome-pacote.ts'
import { rcIdentity } from '../shared/fork-rc-profile.ts'

// Only Donnabot's own host, by absolute path: the remote PATH or Homebrew could hold the
// installed Codync's codync-host, which must never answer for Donnabot.
// D443 (PR d): o pacote se chama Mexerica.app; o Donnabot.app de antes (ou do rollback) segue reconhecido.
const MAIN_HOSTS = ['$HOME/.local/bin/donnabot-host', ...pacotesDoApp('main').map((nome) => `/Applications/${nome}.app/Contents/Resources/donnabot-host`)]
const DEV_HOSTS = [...pacotesDoApp('dev').map((nome) => `/Applications/${nome}.app/Contents/Resources/donnabot-host`), '$HOME/.local/bin/donnabot-dev-host']

/** The caller validates the port before constructing the remote shell command. */
export const remoteInfoCommand = (port: number, folder = '.mexerica') => {
  if (folder === rcIdentity.dataFolder) {
    return `sh -lc 'export MEXERICA_HOME="$HOME/${rcIdentity.dataFolder}"; exec "$HOME/.local/bin/donnabot-host" info --json --port ${port}'`
  }
  // Only the fixed development folder is accepted; no arbitrary input enters this shell command.
  const dev = folder === '.mexerica-dev'
  const hosts = (dev ? DEV_HOSTS : MAIN_HOSTS).map((h) => `"${h}"`).join(' ')
  const home = dev ? 'export MEXERICA_HOME="$HOME/.mexerica-dev"; ' : ''
  return `sh -lc '${home}for h in ${hosts}; do if [ -x "$h" ]; then exec "$h" info --json --port ${port}; fi; done; exit 127'`
}

/** `dir` is this app's host data folder (`~/.mexerica`, or a development sandbox's). */
export const knownHostsFiles = (h: string, dir = `${h}/.mexerica`, rc = false) => rc
  ? [`${dir}/ssh_known_hosts`]
  : [`${h}/.ssh/known_hosts`, `${dir}/ssh_known_hosts`]

/** `pkill -f` pattern for the tunnels that use `dir`'s known_hosts file: this copy's own, never another copy's. */
export const orphanTunnelPattern = (dir: string) => `^/usr/bin/ssh -N .*${dir.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}/ssh_known_hosts`

/** ssh's config tokenizer splits `UserKnownHostsFile` on spaces, so each path carries literal quotes. */
export const hostKeyOptions = (h: string, dir?: string, rc = false) => [
  '-o', 'UserKnownHostsFile=' + knownHostsFiles(h, dir, rc).map((f) => `"${f}"`).join(' '),
  ...(rc ? ['-o', 'GlobalKnownHostsFile=none'] : []),
  '-o', 'StrictHostKeyChecking=yes',
]

function targetOptions(p: SSHProfile) {
  const args: string[] = []
  if (p.port !== null) args.push('-p', String(p.port))
  if (p.identityFile !== null) args.push('-i', p.identityFile)
  if (p.user !== null) args.push('-l', p.user)
  return args
}

export function resolveArguments(p: SSHProfile) {
  const args = ['-G']
  if (p.port !== null) args.push('-p', String(p.port))
  if (p.user !== null) args.push('-l', p.user)
  return [...args, '--', p.host]
}

/** The remote command is fixed; only the validated port number goes into it. */
export const infoArguments = (p: SSHProfile, h: string, folder = '.mexerica', dir = `${h}/${folder}`) => [
  '-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '-o', 'ForwardAgent=no', '-o', 'ForwardX11=no',
  ...hostKeyOptions(h, dir, folder === rcIdentity.dataFolder), ...targetOptions(p),
  '--', p.host, remoteInfoCommand(p.remotePort, folder),
]

/**
 * `inet6`: the SSH config sets `AddressFamily inet6`, which OpenSSH also applies to the local
 * listener, so `-L 127.0.0.1:…` can't bind; `any` frees the listener (HostName still decides the address).
 */
export const tunnelArguments = (p: SSHProfile, localPort: number, h: string, dir?: string, inet6 = false, rc = false) => [
  '-N', '-T', '-o', 'ExitOnForwardFailure=yes', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3',
  ...(inet6 ? ['-o', 'AddressFamily=any'] : []),
  // A shared master owns the listener and lets this child exit. The app must own
  // its tunnel process to verify the listener and close the forward on disconnect.
  '-S', 'none',
  '-o', 'ForwardAgent=no', '-o', 'ForwardX11=no', '-o', 'BatchMode=yes',
  ...hostKeyOptions(h, dir, rc),
  '-L', `127.0.0.1:${localPort}:127.0.0.1:${p.remotePort}`,
  ...targetOptions(p), '--', p.host,
]
