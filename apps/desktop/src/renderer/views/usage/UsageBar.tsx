// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { ProgressBar } from '../../components/Controls'
import './usage.css'

/** A capsule filled to `percent`: the provider's tint below 70%, then amber, red from 90%. */
export function UsageBar({ percent, tint, label }: { percent: number; tint: string; label?: string }) {
  const fill = percent >= 90 ? 'var(--danger)' : percent >= 70 ? 'var(--warning)' : tint
  return (
    <ProgressBar
      value={Math.min(1, Math.max(0.02, percent / 100))}
      label={label ?? `${Math.round(percent)}% used`}
      fill={fill}
      className="usage-bar"
    />
  )
}
