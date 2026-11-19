import { ROLES, configFromRole, type AvatarConfig } from '../../lib/donnabot-avatars/src'

/**
 * D169 (components/AvatarEditor): the function preset whose look `config` is exactly, so the editor can
 * mark it; once body, eyes or color change, none is marked. Read from the config itself, no stored role.
 */
export function presetRoleId(config: AvatarConfig): string | undefined {
  return ROLES.find((role) => {
    const preset = configFromRole(role)
    return (Object.keys(preset) as (keyof AvatarConfig)[]).every((key) =>
      key === 'color' ? preset.color.toLowerCase() === String(config.color ?? '').toLowerCase() : preset[key] === config[key])
  })?.id
}
