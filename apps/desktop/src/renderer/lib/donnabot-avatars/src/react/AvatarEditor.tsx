import { useState, type CSSProperties, type ReactNode } from 'react';
import { ICONS, LABELS, NONE_ICON, PALETTE, ROLES, SHAPES, configFromRole } from '../core/data';
import type {
  AvatarConfig, BadgeStyle, CheeksId, ChinId, EarsId, EyeId, FaceId, HeadId, IconId, NeckId, RolePreset, ShapeId,
} from '../core/types';
import { BotFace } from './BotFace';

export type EditorTab = 'role' | 'body' | 'eyes' | 'color' | 'acc' | 'badge';

export interface AvatarEditorProps {
  value: AvatarConfig;
  onChange: (next: AvatarConfig) => void;
  /** Presets mostrados na aba Função. Padrão: as 30 funções. */
  roles?: RolePreset[];
  /** Chamado quando a pessoa escolhe uma função (além do onChange com a aparência). */
  onPickRole?: (role: RolePreset) => void;
  selectedRoleId?: string;
  initialTab?: EditorTab;
  /** Esconde abas. Ex.: ['role'] para não mostrar presets. */
  hideTabs?: EditorTab[];
  /** Miniaturas piscando. Desligue em telas com muitos editores. Padrão true. */
  animateTiles?: boolean;
  className?: string;
  style?: CSSProperties;
}

/* Tema via variáveis CSS (todas com fallback escuro). Sobrescreva no container. */
const V = {
  surface: 'var(--dbe-surface, #15171C)',
  tile: 'var(--dbe-tile, #1A1C21)',
  tileActive: 'var(--dbe-tile-active, #23262D)',
  border: 'var(--dbe-border, #2C3038)',
  text: 'var(--dbe-text, #F2F3F5)',
  muted: 'var(--dbe-muted, #9AA0AA)',
  ring: 'var(--dbe-ring, #F2F3F5)',
  segBg: 'var(--dbe-seg-bg, #0F1014)',
  segActive: 'var(--dbe-seg-active, #F2F3F5)',
  segActiveText: 'var(--dbe-seg-active-text, #0D0E11)',
};

const TABS: [EditorTab, string][] = [
  ['role', 'Função'], ['body', 'Corpo'], ['eyes', 'Olhos'], ['color', 'Cor'], ['acc', 'Acessórios'], ['badge', 'Selo'],
];

const grid = (min: number): CSSProperties => ({ display: 'grid', gridTemplateColumns: `repeat(auto-fill, minmax(${min}px, 1fr))`, gap: 8 });
const caption: CSSProperties = { fontSize: 12, fontWeight: 700, letterSpacing: '0.04em', textTransform: 'uppercase', color: V.muted };

function Tile(props: { active: boolean; onClick: () => void; label: string; sub?: string; children: ReactNode }) {
  const { active, onClick, label, sub, children } = props;
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      style={{
        display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 4, padding: '8px 4px 10px',
        borderRadius: 14, border: `2px solid ${active ? V.ring : 'transparent'}`,
        background: active ? V.tileActive : V.tile, color: V.text, cursor: 'pointer', font: 'inherit', textAlign: 'center',
      }}
    >
      {children}
      <span style={{ fontSize: 12, fontWeight: 700 }}>{label}</span>
      {sub && <span style={{ fontSize: 11, lineHeight: 1.3, color: V.muted }}>{sub}</span>}
    </button>
  );
}

function Segmented<T extends string>(props: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string }) {
  return (
    <div role="group" aria-label={props.label} style={{ display: 'flex', gap: 4, padding: 4, borderRadius: 999, background: V.segBg, border: `1px solid ${V.border}` }}>
      {props.options.map(([id, text]) => {
        const on = props.value === id;
        return (
          <button
            key={id}
            type="button"
            aria-pressed={on}
            onClick={() => props.onChange(id)}
            style={{
              flex: '0 0 auto', whiteSpace: 'nowrap', minHeight: 36, padding: '0 14px', border: 0, borderRadius: 999, cursor: 'pointer', font: 'inherit',
              fontSize: 13, fontWeight: 700, background: on ? V.segActive : 'transparent', color: on ? V.segActiveText : V.muted,
            }}
          >
            {text}
          </button>
        );
      })}
    </div>
  );
}

function IconBubble({ paths, bg, fg, ring }: { paths: string[]; bg: string; fg: string; ring: string }) {
  return (
    <span style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: 40, height: 40, borderRadius: '50%', background: bg, color: fg, boxShadow: ring }}>
      <svg viewBox="0 0 24 24" width={22} height={22} fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        {paths.map((d, i) => <path key={i} d={d} />)}
      </svg>
    </span>
  );
}

/** Editor completo de avatar (abas Função, Corpo, Olhos, Cor, Acessórios, Selo). */
export function AvatarEditor(props: AvatarEditorProps) {
  const { value, onChange, roles = ROLES, onPickRole, selectedRoleId, initialTab = 'role', hideTabs = [], animateTiles = true, className, style } = props;
  const tabs = TABS.filter(([id]) => !hideTabs.includes(id));
  const [tab, setTab] = useState<EditorTab>(tabs.some(([id]) => id === initialTab) ? initialTab : tabs[0][0]);
  const set = (patch: Partial<AvatarConfig>) => onChange({ ...value, ...patch });
  const eyeKey = value.eye !== 'auto' ? value.eye : SHAPES[value.shape].eye;
  const face = (cfg: AvatarConfig, size: number) => <BotFace config={cfg} size={size} autoLife={animateTiles} />;

  const groups: { key: keyof AvatarConfig; title: string; opts: [string, string][] }[] = [
    { key: 'head', title: 'Cabeça', opts: Object.entries(LABELS.head) as [HeadId, string][] },
    { key: 'face', title: 'Rosto', opts: (Object.entries(LABELS.face) as [FaceId, string][]).map(([k, l]) => [k, k === 'glasses' && eyeKey === 'cyc' ? 'Monóculo' : l]) },
    { key: 'ears', title: 'Orelhas', opts: Object.entries(LABELS.ears) as [EarsId, string][] },
    { key: 'chin', title: 'Queixo', opts: Object.entries(LABELS.chin) as [ChinId, string][] },
    { key: 'neck', title: 'Pescoço', opts: Object.entries(LABELS.neck) as [NeckId, string][] },
    { key: 'cheeks', title: 'Bochechas', opts: Object.entries(LABELS.cheeks) as [CheeksId, string][] },
  ];

  const bs = value.badgeStyle;
  const bubbleBg = bs === 'dark' ? '#0D0E11' : bs === 'color' ? value.color : '#F5F6F8';
  const bubbleFg = bs === 'dark' ? '#C9CDD6' : bs === 'color' ? '#FFFFFF' : '#15171C';

  return (
    <div className={className} style={{ display: 'flex', flexDirection: 'column', gap: 16, color: V.text, ...style }}>
      <div role="tablist" aria-label="Personalizar avatar" style={{ display: 'flex', flexWrap: 'wrap', gap: 4, padding: 4, borderRadius: 999, background: V.segBg, border: `1px solid ${V.border}` }}>
        {tabs.map(([id, text]) => {
          const on = tab === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={on}
              onClick={() => setTab(id)}
              style={{
                flex: '1 1 auto', minHeight: 40, padding: '0 10px', border: 0, borderRadius: 999, cursor: 'pointer', font: 'inherit',
                fontSize: 13, fontWeight: 700, background: on ? V.segActive : 'transparent', color: on ? V.segActiveText : V.muted,
              }}
            >
              {text}
            </button>
          );
        })}
      </div>

      {tab === 'role' && (
        <div role="tabpanel" style={grid(88)}>
          {roles.map((r) => (
            <Tile
              key={r.id}
              active={r.id === selectedRoleId}
              label={r.name}
              sub={r.role}
              onClick={() => { onChange(configFromRole(r)); onPickRole?.(r); }}
            >
              {face(configFromRole(r), 48)}
            </Tile>
          ))}
        </div>
      )}

      {tab === 'body' && (
        <div role="tabpanel" style={grid(96)}>
          {(Object.keys(SHAPES) as ShapeId[]).map((k) => (
            <Tile key={k} active={value.shape === k} label={SHAPES[k].label} onClick={() => set({ shape: k })}>
              {face({ ...value, shape: k }, 60)}
            </Tile>
          ))}
        </div>
      )}

      {tab === 'eyes' && (
        <div role="tabpanel" style={grid(110)}>
          {(Object.keys(LABELS.eye) as EyeId[]).map((k) => (
            <Tile key={k} active={value.eye === k} label={LABELS.eye[k]} onClick={() => set({ eye: k })}>
              {face({ ...value, eye: k }, 68)}
            </Tile>
          ))}
        </div>
      )}

      {tab === 'color' && (
        <div role="tabpanel" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(64px, 1fr))', gap: 12, justifyItems: 'center' }}>
            {PALETTE.map((c) => {
              const on = value.color.toLowerCase() === c.hex.toLowerCase();
              return (
                <div key={c.hex} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6 }}>
                  <button
                    type="button"
                    aria-pressed={on}
                    aria-label={'Cor ' + c.name}
                    onClick={() => set({ color: c.hex })}
                    style={{ width: 48, height: 48, borderRadius: '50%', border: 0, padding: 0, cursor: 'pointer', background: c.hex, boxShadow: on ? `0 0 0 3px ${V.surface}, 0 0 0 5px ${V.ring}` : 'none' }}
                  />
                  <span style={{ fontSize: 12, color: V.muted }}>{c.name}</span>
                </div>
              );
            })}
          </div>
          <label style={{ display: 'flex', alignItems: 'center', gap: 12, fontSize: 14, color: V.muted }}>
            <input
              type="color"
              value={value.color}
              onChange={(e) => set({ color: e.target.value.toUpperCase() })}
              style={{ width: 44, height: 44, padding: 0, border: 0, background: 'transparent', cursor: 'pointer' }}
            />
            Cor própria (qualquer cor funciona)
          </label>
        </div>
      )}

      {tab === 'acc' && (
        <div role="tabpanel" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          {groups.map((g) => (
            <div key={g.key} style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div style={caption}>{g.title}</div>
              <div style={grid(80)}>
                {g.opts.map(([id, text]) => (
                  <Tile key={id} active={value[g.key] === id} label={text} onClick={() => set({ [g.key]: id } as Partial<AvatarConfig>)}>
                    {face({ ...value, [g.key]: id, badge: 'none' } as AvatarConfig, 44)}
                  </Tile>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {tab === 'badge' && (
        <div role="tabpanel" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16, flexWrap: 'wrap' }}>
            <div style={{ fontSize: 13, color: V.muted }}>Add-on no canto do bot: escolha o ícone e o estilo.</div>
            <Segmented<BadgeStyle>
              label="Estilo do selo"
              value={bs}
              onChange={(v) => set({ badgeStyle: v })}
              options={[['light', LABELS.badgeStyle.light], ['dark', LABELS.badgeStyle.dark], ['color', LABELS.badgeStyle.color]]}
            />
          </div>
          <div style={grid(72)}>
            <Tile active={value.badge === 'none'} label="Nenhum" onClick={() => set({ badge: 'none' })}>
              <IconBubble paths={NONE_ICON} bg="transparent" fg={V.muted} ring={`inset 0 0 0 1px ${V.border}`} />
            </Tile>
            {(Object.keys(ICONS) as IconId[]).map((id) => (
              <Tile key={id} active={value.badge === id} label={LABELS.icon[id]} onClick={() => set({ badge: id })}>
                <IconBubble paths={ICONS[id]} bg={bubbleBg} fg={bubbleFg} ring={bs === 'dark' ? `inset 0 0 0 1px ${V.border}` : 'none'} />
              </Tile>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
