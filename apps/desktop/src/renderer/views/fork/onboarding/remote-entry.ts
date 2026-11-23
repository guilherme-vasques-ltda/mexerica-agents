import type { SSHState } from '../../../../shared/ssh.ts'
import type { ForkCloudModel } from '../cloud/fork-cloud-model.ts'

export function noConfiguredRemote(
  ssh: Pick<SSHState, 'profiles' | 'attachments'> | null,
  cloud: Pick<ForkCloudModel, 'busy' | 'unreachable' | 'state' | 'computers' | 'attachment'> | null,
): boolean {
  return ssh !== null && ssh.profiles.length === 0 && ssh.attachments.length === 0
    && (cloud === null || (!cloud.busy && !cloud.unreachable && cloud.state?.kind === 'noHub'
      && !cloud.state.readUnavailable && cloud.computers.length === 0 && !cloud.attachment))
}
