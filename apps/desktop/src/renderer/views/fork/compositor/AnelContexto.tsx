import { useId } from 'react'

/** D533: o anel do medidor existente, segmentado; o botão externo anuncia o valor real. */
export function AnelContexto({ percentual }: { percentual: number }) {
  const mascara = `ctx-gomos-${useId()}`
  const raio = 12
  const volta = 2 * Math.PI * raio
  const passo = volta / 20
  return (
    <svg className="ctx-gomos" width="28" height="28" viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <defs>
        <mask id={mascara} maskUnits="userSpaceOnUse" style={{ maskType: 'alpha' }} x="0" y="0" width="32" height="32">
          <circle cx="16" cy="16" r={raio} fill="none" stroke="white" strokeWidth="7" strokeDasharray={`${passo * .8} ${passo * .2}`} transform="rotate(-90 16 16)" />
        </mask>
      </defs>
      <g mask={`url(#${mascara})`}>
        <circle className="ctx-trilho" cx="16" cy="16" r={raio} />
        <circle className="ctx-arco" cx="16" cy="16" r={raio} strokeDasharray={`${volta * percentual / 100} ${volta}`} transform="rotate(-90 16 16)" />
      </g>
    </svg>
  )
}
