// Fundo ilustrado da entrada guiada: pomar de mexericas em SVG com animação só em CSS (entrada-guiada.css).
// Decorativo (aria-hidden): o texto da tela fica no cartão. Com "reduzir movimento", a cena fica parada no quadro final.

const FRUTAS_ESQUERDA: Array<[number, number, number]> = [
  [118, 352, 15], [196, 300, 14], [272, 340, 16], [330, 392, 13], [156, 430, 14], [238, 418, 15], [300, 470, 12], [96, 470, 12],
]
const FRUTAS_DIREITA: Array<[number, number, number]> = [
  [1262, 436, 12], [1318, 398, 13], [1372, 450, 12], [1300, 484, 11], [1390, 506, 11],
]
const FOLHAS_CAINDO = [
  { x: 300, y: 300, atraso: 3.2, duracao: 11 },
  { x: 180, y: 360, atraso: 7.4, duracao: 13 },
  { x: 1330, y: 420, atraso: 5.1, duracao: 12 },
]

function Fruta({ x, y, r, ordem }: { x: number; y: number; r: number; ordem: number }) {
  return (
    <g className="eg-fruta" style={{ ['--ordem' as string]: ordem }}>
      <circle cx={x} cy={y} r={r} fill="url(#eg-fruta)" />
      <path d={`M${x + 1} ${y - r + 1}c2-6 8-8 13-6c-2 5-7 7-13 6z`} fill="#6caa3a" />
    </g>
  )
}

/** Uma mexerica-robô: corpo laranja, folha, olhos e bracinhos; `cesto` leva mexericas colhidas. */
function Bot({ x, y, escala, cesto, classe }: { x: number; y: number; escala: number; cesto?: boolean; classe: string }) {
  return (
    <g transform={`translate(${x} ${y}) scale(${escala})`}>
      <g className={`eg-bot ${classe}`}>
        <ellipse cx="0" cy="34" rx="26" ry="5" fill="var(--eg-sombra)" />
        <g className="eg-bot-corpo">
          <ellipse cx="-27" cy="6" rx="7" ry="10" transform="rotate(14 -27 6)" fill="url(#eg-fruta)" />
          {cesto ? null : <ellipse className="eg-bot-acena" cx="27" cy="-8" rx="7" ry="10" transform="rotate(-38 27 -8)" fill="url(#eg-fruta)" />}
          <circle cx="0" cy="0" r="28" fill="url(#eg-fruta)" />
          <circle cx="-9" cy="-10" r="9" fill="#ffd9a0" opacity=".35" />
          <path d="M-2-27q1-9 3-13" stroke="#5e8f2e" strokeWidth="4" strokeLinecap="round" fill="none" />
          <path d="M1-36c5-10 16-12 22-8c-5 8-14 11-22 8z" fill="#79b347" />
          <g className="eg-bot-olhos">
            <circle cx="-9" cy="-1" r="3.6" fill="#2a1c14" />
            <circle cx="9" cy="-1" r="3.6" fill="#2a1c14" />
          </g>
          <circle cx="-16" cy="7" r="4" fill="#ff8a90" opacity=".55" />
          <circle cx="16" cy="7" r="4" fill="#ff8a90" opacity=".55" />
          <path d="M-5 8q5 5 10 0" stroke="#2a1c14" strokeWidth="2.4" strokeLinecap="round" fill="none" />
          {cesto ? (
            <g>
              <ellipse cx="27" cy="6" rx="7" ry="10" transform="rotate(-14 27 6)" fill="url(#eg-fruta)" />
              <circle cx="18" cy="14" r="7" fill="url(#eg-fruta)" />
              <circle cx="30" cy="12" r="7" fill="url(#eg-fruta)" />
              <circle cx="42" cy="15" r="6.5" fill="url(#eg-fruta)" />
              <path d="M10 16h40l-5 18h-30z" fill="var(--eg-cesto)" />
              <path d="M13 22h34M15 28h30" stroke="var(--eg-cesto-linha)" strokeWidth="1.6" />
            </g>
          ) : null}
        </g>
      </g>
    </g>
  )
}

export function FundoMexericas() {
  return (
    <div className="eg-fundo" aria-hidden="true">
      <div className="eg-sol" />
      <svg className="eg-cena" viewBox="0 0 1440 900" preserveAspectRatio="xMidYMax slice" focusable="false">
        <defs>
          <radialGradient id="eg-fruta" cx=".36" cy=".3" r=".8">
            <stop offset="0" stopColor="#ffa232" />
            <stop offset=".55" stopColor="#fa8714" />
            <stop offset="1" stopColor="#d35a06" />
          </radialGradient>
          <radialGradient id="eg-copa" cx=".4" cy=".3" r=".8">
            <stop offset="0" stopColor="var(--eg-copa-luz)" />
            <stop offset="1" stopColor="var(--eg-copa)" />
          </radialGradient>
          <path id="eg-folha" d="M0 0c4-7 13-9 20-6c-4 7-12 10-20 6z" />
        </defs>

        <g className="eg-morros">
          <path d="M0 640C180 580 360 600 540 630S900 600 1080 590S1340 610 1440 600V900H0Z" fill="var(--eg-morro-longe)" />
          <path d="M0 720C220 680 420 700 640 720S1060 690 1440 700V900H0Z" fill="var(--eg-morro)" />
          <path d="M0 790C300 770 600 780 900 790S1300 780 1440 784V900H0Z" fill="var(--eg-chao)" />
        </g>

        <g className="eg-arvore eg-arvore-esquerda">
          <g className="eg-balanco">
            <path d="M206 800C214 700 218 620 210 520L236 520C232 620 236 700 252 800Z" fill="var(--eg-tronco)" />
            <path d="M222 560C196 530 170 512 140 500M226 548C256 520 290 506 324 500" stroke="var(--eg-tronco)" strokeWidth="12" strokeLinecap="round" fill="none" />
            <path d="M224 534c-90 4-170-30-176-104c-5-62 40-96 84-104c10-52 60-86 116-80c50 6 82 40 90 76c50 6 86 46 80 100c-6 64-68 104-130 106c-20 6-42 8-64 6z" fill="var(--eg-copa-fundo)" />
            <path d="M210 512c-70 0-130-34-130-92c0-46 36-74 74-78c12-40 52-64 94-60c40 4 66 30 72 60c40 6 66 36 62 76c-4 54-56 90-112 94c-20 2-40 2-60 0z" fill="url(#eg-copa)" />
            {FRUTAS_ESQUERDA.map(([x, y, r], i) => <Fruta key={i} x={x} y={y} r={r} ordem={i} />)}
          </g>
        </g>

        <g className="eg-arvore eg-arvore-direita">
          <g className="eg-balanco eg-balanco-lento">
            <path d="M1318 790C1324 720 1326 660 1320 590L1340 590C1338 660 1342 720 1352 790Z" fill="var(--eg-tronco)" />
            <path d="M1326 580c-72 0-126-28-126-78c0-40 32-64 66-68c10-34 46-54 82-50c34 4 56 26 62 52c34 6 56 32 52 66c-4 48-48 78-96 80c-14 0-28 0-40-2z" fill="url(#eg-copa)" />
            {FRUTAS_DIREITA.map(([x, y, r], i) => <Fruta key={i} x={x} y={y} r={r} ordem={i + FRUTAS_ESQUERDA.length} />)}
          </g>
        </g>

        <g className="eg-graminhas" fill="var(--eg-grama)">
          <path d="M360 790q3-12 6 0q2-10 5 0zM700 796q3-12 6 0q2-10 5 0zM980 788q3-12 6 0q2-10 5 0zM1180 794q3-12 6 0q2-10 5 0z" />
        </g>

        <Bot x={420} y={752} escala={1} cesto classe="eg-bot-1" />
        <Bot x={1150} y={758} escala={0.86} classe="eg-bot-2" />

        {FOLHAS_CAINDO.map((f, i) => (
          <g key={i} transform={`translate(${f.x} ${f.y})`}>
            <g className="eg-folha-cai" style={{ ['--atraso' as string]: `${f.atraso}s`, ['--duracao' as string]: `${f.duracao}s` }}>
              <use href="#eg-folha" fill="var(--eg-copa-luz)" />
            </g>
          </g>
        ))}
      </svg>
    </div>
  )
}
