import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { RoutineDemoSession } from './rotina-etapas'

const enabled = new URLSearchParams(location.search).get('rotina-etapas') === 'on'

function RoutineCard() {
  const [session] = useState(() => new RoutineDemoSession(enabled))
  const [, render] = useState(0)
  const blocked = !session.enabled || session.recorded || session.steps.some((step) => step.state === 'uncertain')
  return <main className="routine-demo">
    <header><span className="routine-demo__eyebrow">Esperando você</span><h1>Revisar rascunho</h1>
      <p>Quando chegar uma mensagem, prepare uma resposta e mostre antes de enviar.</p></header>
    <article className="routine-demo__card">
      <div className="routine-demo__top"><div><span className="routine-demo__badge">SIMULAÇÃO · DADOS FICTÍCIOS</span><h2>Rotina de exemplo</h2></div><span className="routine-demo__status">Aguardando revisão</span></div>
      <p className="routine-demo__summary">Uma mensagem fictícia chegou em <code>caixa@example.invalid</code>. O estado foi retomado de um snapshot de demonstração.</p>
      <ol className="routine-demo__steps">{session.steps.map((step, index) => <li key={step.id} className={`routine-demo__step is-${step.state}`}>
        <span className="routine-demo__number">{step.state === 'done' ? '✓' : index + 1}</span><span>{step.title}</span>
        <small>{step.state === 'done' ? 'Concluída' : step.state === 'waiting' ? 'Esperando você' : step.state === 'uncertain' ? 'Efeito não confirmado · reenvio bloqueado' : 'Próxima'}</small>
      </li>)}</ol>
      <div className="routine-demo__draft"><b>Rascunho</b><p>Olá! Obrigado pela mensagem. Vou revisar os detalhes e retorno em breve.</p><small>Prévia local · nenhuma mensagem será enviada</small></div>
      {!session.enabled && <p className="routine-demo__hint" role="status">A demonstração está desligada. Para ativar somente este cenário, use <code>?rotina-etapas=on</code>.</p>}
      {session.recorded && <p className="routine-demo__hint" role="status">Intenção simulada registrada nesta sessão. Nenhuma ação externa foi executada.</p>}
      <button disabled={blocked} onClick={() => { session.continueOnce(); render((value) => value + 1) }}>Continuar</button>
    </article>
  </main>
}

const style = document.createElement('style')
style.textContent = `
  :root { color-scheme: light; font: 14px/1.5 Inter, ui-sans-serif, system-ui, sans-serif; color: #28251f; background: #f6f4f0; }
  * { box-sizing: border-box; } body { margin: 0; } .routine-demo { width: min(760px, calc(100% - 40px)); margin: 64px auto; }
  .routine-demo__eyebrow { color: #875226; font-size: 13px; font-weight: 650; } h1 { font-size: 30px; margin: 8px 0; letter-spacing: -.025em; } header p { color: #655f57; margin: 0 0 26px; }
  .routine-demo__card { border: 1px solid #e5e0d8; border-radius: 16px; background: white; padding: 24px; box-shadow: 0 8px 28px #382b1710; }
  .routine-demo__top { display:flex; justify-content:space-between; gap:16px; align-items:start; } h2 { margin: 10px 0 0; font-size: 20px; } .routine-demo__badge { display:inline-block; color:#6a4b13; background:#fff4cf; border-radius:999px; padding:5px 9px; font-size:11px; font-weight:700; letter-spacing:.04em; }
  .routine-demo__status { color:#76521a; background:#fff8e5; border-radius:999px; padding:6px 10px; font-size:12px; white-space:nowrap; } .routine-demo__summary { color:#635e56; margin:18px 0; }
  .routine-demo__steps { list-style:none; padding:0; margin:0; border:1px solid #ece8e2; border-radius:12px; overflow:hidden; } .routine-demo__step { display:grid; grid-template-columns:30px 1fr auto; gap:10px; align-items:center; padding:13px 14px; } .routine-demo__step + li { border-top:1px solid #ece8e2; }
  .routine-demo__number { width:24px;height:24px;border-radius:50%;background:#eeeae3;display:grid;place-items:center;font-size:12px;font-weight:700;color:#61594f; } .is-done .routine-demo__number { background:#e8f3e9;color:#317044; } .is-waiting .routine-demo__number { background:#fff0cc;color:#855a12; }
  .routine-demo__step small { color:#716b63; text-align:right; } .routine-demo__draft { margin-top:18px; padding:16px; background:#faf9f7; border:1px solid #ece8e2; border-radius:12px; } .routine-demo__draft p { margin:8px 0; } .routine-demo__draft small,.routine-demo__hint { color:#625d55; }
  .routine-demo button { min-height:44px; border:0; border-radius:10px; background:#9b5725; color:white; padding:0 18px; font:inherit; font-weight:650; cursor:pointer; margin-top:12px; } .routine-demo button:focus-visible { outline:3px solid #3423a0; outline-offset:2px; } .routine-demo button:disabled { background:#bdb8b0; cursor:not-allowed; }
  @media(max-width:600px) { .routine-demo { margin:32px auto; } .routine-demo__card { padding:18px; } .routine-demo__step { grid-template-columns:30px 1fr; } .routine-demo__step small { grid-column:2; text-align:left; } .routine-demo__top { flex-direction:column; } }
`
document.head.append(style)
createRoot(document.getElementById('root')!).render(<RoutineCard />)
