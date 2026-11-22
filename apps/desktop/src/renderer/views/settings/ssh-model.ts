// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect } from 'react'
import type { SSHState } from '@shared/ssh'
import { LoopbackTransport } from '../../client/host-client'
import { Observable, useModel } from '../../lib/observable'
import type { AppModel } from '../../store/app-model'
import { BotStore } from '../../store/bot-store'
import { linkCaido, transporteParado } from '../../client/fork-transporte-parado'
import { sshQuedaTemporaria } from '../../store/fork-queda-curta'
import { AnexosSSH } from './fork-ssh-anexos'

/** The main process's SSH computers, mirrored for views. */
class SSHModel extends Observable {
  state: SSHState = { profiles: [], status: {}, attachments: [] }
  hasSnapshot = false
  private started = false

  start() {
    if (this.started) return
    this.started = true
    window.codync.ssh.onChange((s) => this.apply(s))
    void window.codync.ssh.state().then((s) => {
      if (!this.hasSnapshot) this.apply(s)
    }).catch(() => {})
  }

  private apply(s: SSHState) {
    if (!Array.isArray(s?.profiles) || !Array.isArray(s.attachments)) return
    this.state = s
    this.hasSnapshot = true
    this.changed()
  }

  status(id: string) {
    return this.state.status[id] ?? { kind: 'idle' as const }
  }

  isSSH(computerId: string) {
    return this.state.attachments.some((a) => a.computer.id === computerId)
  }
}

export const ssh = new SSHModel()

export function useSSH() {
  useEffect(() => ssh.start(), [])
  return useModel(ssh)
}

/**
 * Attaches each connected SSH computer to the window's model as a loopback store (fork-ssh-anexos.ts). MX-217: a short
 * tunnel drop keeps the store, cache, drafts and selection; a definitive end detaches it as before. Call once, from
 * ChatWindow.
 */
export function useSSHAttachments(app: AppModel) {
  useEffect(() => {
    ssh.start()
    const anexos = new AnexosSSH<BotStore>({
      store: (id) => app.stores.get(id) ?? null,
      computers: () => app.computers,
      attach: (computer, store) => app.attach(computer, store),
      detach: (id) => app.detach(id),
      lojasDo: (id) => app.spaceMirrors.doComputador(id),
    }, (computer, transporte) => new BotStore(computer, true, transporte), (t) => new LoopbackTransport(t.baseURL, t.token), transporteParado, linkCaido)
    const sync = () => anexos.sync(ssh.state)
    sync()
    const off = ssh.subscribe(sync)
    return () => {
      off()
      anexos.parar()
    }
  }, [app])
}

/** Tenta agora (MX-217): reabre já o túnel SSH deste computador, se ele está caído e o Main ainda tenta. */
export function reconectarSSH(computerId: string): boolean {
  const profile = ssh.state.profiles.find((p) => p.computerId === computerId)
  if (!profile || !sshQuedaTemporaria(ssh.status(profile.id)) || ssh.isSSH(computerId)) return false
  window.codync.ssh.connect(profile.id)
  return true
}
