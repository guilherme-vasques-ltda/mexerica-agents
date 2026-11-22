// Modified for the standalone public library.
import { useState, type CSSProperties } from 'react';
import { createRoot } from 'react-dom/client';
import {
  AvatarEditor, BotFace, LABELS, ROLES, SHAPES, configFromRole, tintOf, useBotMood,
  type AvatarConfig, type BaseMood, type MomentAction, type RolePreset,
} from '../src';

const card: CSSProperties = { background: '#15171C', border: '1px solid #23262D', borderRadius: 24, padding: 24 };

const ACTIONS: [string, string, 'base' | 'play'][] = [
  ['idle', 'Ocioso', 'base'], ['blink', 'Piscar', 'play'], ['left', 'Olhar ←', 'play'], ['right', 'Olhar →', 'play'],
  ['up', 'Olhar ↑', 'play'], ['listen', 'Ouvindo', 'base'], ['think', 'Pensando', 'base'], ['work', 'Trabalhando', 'base'],
  ['happy', 'Feliz', 'play'], ['surprise', 'Surpreso', 'play'], ['sleep', 'Dormir', 'base'], ['wake', 'Acordar', 'play'],
];

function Studio() {
  const [role, setRole] = useState<RolePreset>(ROLES[0]);
  const [config, setConfig] = useState<AvatarConfig>(configFromRole(ROLES[0]));
  const [name, setName] = useState(ROLES[0].name);
  const { mood, base, setBase, play } = useBotMood('idle');

  return (
    <section style={{ display: 'flex', gap: 24, flexWrap: 'wrap' }}>
      <div style={{ ...card, flex: '1 1 520px', display: 'flex', flexDirection: 'column', gap: 20 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', gap: 16, flexWrap: 'wrap' }}>
          <label style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: '0.04em', textTransform: 'uppercase', color: '#9AA0AA' }}>Nome do bot</span>
            <input
              value={name}
              onChange={(e) => setName(e.target.value.slice(0, 24))}
              style={{ height: 44, padding: '0 14px', borderRadius: 12, border: '1px solid #2C3038', background: '#0F1014', color: '#F2F3F5', font: 'inherit', fontSize: 22, fontWeight: 700 }}
            />
            <span style={{ fontSize: 14, color: '#9AA0AA' }}>{role.role} · {SHAPES[config.shape].label}</span>
          </label>
          <span style={{ padding: '6px 14px', borderRadius: 999, background: '#1D2026', border: '1px solid #2C3038', fontSize: 14, fontWeight: 600 }}>
            {LABELS.mood[mood]}
          </span>
        </div>
        <div style={{ display: 'flex', justifyContent: 'center', padding: '24px 0' }}>
          <BotFace config={config} mood={mood} size={260} label={`${name}, ${role.role}`} />
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(110px, 1fr))', gap: 8 }}>
          {ACTIONS.map(([id, label, kind]) => {
            const on = kind === 'base' ? base === id && mood === base : false;
            return (
              <button
                key={id}
                type="button"
                aria-pressed={on}
                onClick={() => (kind === 'base' ? setBase(id as BaseMood) : play(id as MomentAction))}
                style={{
                  minHeight: 44, borderRadius: 12, border: `1px solid ${on ? '#F2F3F5' : '#2C3038'}`, cursor: 'pointer', font: 'inherit',
                  fontWeight: 600, background: on ? '#F2F3F5' : '#1D2026', color: on ? '#0D0E11' : '#E6E8EC',
                }}
              >
                {label}
              </button>
            );
          })}
        </div>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          {[44, 32, 24].map((s) => (
            <div key={s} style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 12px', borderRadius: 14, background: '#1A1C21' }}>
              <BotFace config={config} mood={mood} size={s} />
              <span style={{ fontSize: 13, color: '#9AA0AA' }}>{s}px</span>
            </div>
          ))}
        </div>
      </div>
      <div style={{ ...card, flex: '1 1 480px' }}>
        <AvatarEditor
          value={config}
          onChange={setConfig}
          selectedRoleId={role.id}
          onPickRole={(r) => { setRole(r); setName(r.name); play('happy'); }}
        />
      </div>
    </section>
  );
}

function Team() {
  return (
    <section style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 16 }}>
      {ROLES.map((r) => (
        <div key={r.id} style={{ ...card, padding: 16, display: 'flex', flexDirection: 'column', gap: 6 }}>
          <div style={{ display: 'flex', justifyContent: 'center', padding: '8px 0' }}>
            <BotFace config={configFromRole(r)} size={100} label={`${r.name}, ${r.role}`} mood={r.id === 'psicologa' ? 'listen' : r.id === 'estoque' ? 'work' : 'idle'} />
          </div>
          <div style={{ fontSize: 11, fontWeight: 700, letterSpacing: '0.06em', textTransform: 'uppercase', color: '#8A909A' }}>{r.area}</div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', gap: 8 }}>
            <strong style={{ fontSize: 18 }}>{r.name}</strong>
            <span style={{ fontSize: 12, fontWeight: 700, color: tintOf(r.color) }}>{r.role}</span>
          </div>
          <div style={{ fontSize: 13, lineHeight: 1.45, color: '#B4B9C2' }}>{r.desc}</div>
        </div>
      ))}
    </section>
  );
}

function App() {
  return (
    <main style={{ maxWidth: 1440, margin: '0 auto', padding: '40px 24px', display: 'flex', flexDirection: 'column', gap: 40 }}>
      <header>
        <h1 style={{ margin: 0, fontSize: 40 }}>Mexerica Bot Avatars</h1>
        <p style={{ margin: '6px 0 0', color: '#9AA0AA' }}>Demonstração interativa da biblioteca de avatares.</p>
      </header>
      <Studio />
      <h2 style={{ margin: 0 }}>Equipe (30 presets)</h2>
      <Team />
    </main>
  );
}

createRoot(document.getElementById('root')!).render(<App />);
