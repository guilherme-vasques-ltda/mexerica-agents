import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { RoutineDemoSession } from './rotina-etapas'
import { useT } from '../lib/fork-i18n/estado'

const enabled = new URLSearchParams(location.search).get('rotina-etapas') === 'on'

function RoutineCard() {
  const [session] = useState(() => new RoutineDemoSession(enabled))
  const [, render] = useState(0)
  const t = useT()
  const stepLabel = (state: string) => state === 'done' ? t('torre.demo.step.done') : state === 'waiting' ? t('torre.demo.step.waiting') : state === 'uncertain' ? t('torre.demo.step.uncertain') : t('torre.demo.step.next')
  const blocked = !session.enabled || session.recorded || session.steps.some((step) => step.state === 'uncertain')
  return <main className="routine-demo">
    <header><span className="routine-demo__eyebrow">{t('torre.esperandoVoce.titulo')}</span><h1>{t('torre.demo.title')}</h1>
      <p>{t('torre.demo.description')}</p></header>
    <article className="routine-demo__card">
      <div className="routine-demo__top"><div><span className="routine-demo__badge">{t('torre.demo.badge')}</span><h2>{t('torre.demo.name')}</h2></div><span className="routine-demo__status">{t('torre.demo.status')}</span></div>
      <p className="routine-demo__summary">{t('torre.demo.arrived')} <code>caixa@example.invalid</code>. {t('torre.demo.snapshot')}</p>
      <ol className="routine-demo__steps">{session.steps.map((step, index) => <li key={step.id} className={`routine-demo__step is-${step.state}`}>
        <span className="routine-demo__number">{step.state === 'done' ? '✓' : index + 1}</span><span>{t(stepTitleKey(step.id))}</span>
        <small>{stepLabel(step.state)}</small>
      </li>)}</ol>
      <div className="routine-demo__draft"><b>{t('torre.demo.draft')}</b><p>{t('torre.demo.draft.text')}</p><small>{t('torre.demo.draft.note')}</small></div>
      {!session.enabled && <p className="routine-demo__hint" role="status">{t('torre.demo.disabled')} <code>?rotina-etapas=on</code>.</p>}
      {session.recorded && <p className="routine-demo__hint" role="status">{t('torre.demo.intentRecorded')}</p>}
      <button disabled={blocked} onClick={() => { session.continueOnce(); render((value) => value + 1) }}>{t('torre.demo.continue')}</button>
    </article>
  </main>
}

function stepTitleKey(id: string) {
  switch (id) {
    case 'read': return 'torre.demo.step.read'
    case 'draft': return 'torre.demo.step.draft'
    case 'review': return 'torre.demo.step.review'
    case 'send': return 'torre.demo.step.send'
    default: return 'torre.demo.step.unknown'
  }
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
