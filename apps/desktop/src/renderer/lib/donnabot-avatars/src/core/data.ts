import type {
  AvatarConfig, CheeksId, ChinId, EarsId, EyeId, EyeKey, FaceId, HeadId,
  IconId, NeckId, RolePreset, ShapeDef, ShapeId,
} from './types';

const EAR = 'polygon(50% 0, 100% 100%, 0 100%)';

export const SHAPES: Record<ShapeId, ShapeDef> = {
  oval: { label: 'Oval', body: [1.0, 1.9, 8.0, 7.1, '50% 50% 44% 44% / 54% 54% 46% 46%'], visor: [1.9, 4.1, 6.2, 2.6, '1.3em'], eye: 'cat', gap: 1.4, look: 1,
    parts: [{ l: 4.88, t: 1.0, w: 0.24, h: 1.2, r: '0.12em', fill: 'dark' }, { l: 4.55, t: 0.45, w: 0.9, h: 0.9, r: '0.18em', fill: 'gold', tf: 'rotate(45deg)', or: '50% 50%' }],
    hat: [5, 2.7, 5.4], side: [5.8, 1.0, 9.0], by: 9.0, nw: 4.4, badge: [6.7, 6.6] },
  pilula: { label: 'Pílula', body: [0.8, 2.6, 8.4, 5.6, '2.8em'], visor: [1.8, 3.8, 6.4, 3.2, '1.6em'], eye: 'pill', gap: 1.5, look: 1,
    parts: [{ l: 0.25, t: 4.6, w: 0.9, h: 1.6, r: '0.45em', fill: 'dark' }, { l: 8.85, t: 4.6, w: 0.9, h: 1.6, r: '0.45em', fill: 'dark' }],
    hat: [5, 3.0, 5.6], side: [5.4, 0.8, 9.2], by: 8.2, nw: 4.4, badge: [6.7, 6.2] },
  cubo: { label: 'Cubo', body: [1.4, 2.6, 7.2, 6.4, '1.8em'], visor: [2.3, 3.7, 5.4, 3.3, '1.1em'], eye: 'round', gap: 1.4, look: 1,
    parts: [{ l: 4.85, t: 1.1, w: 0.3, h: 1.8, r: '0.15em', fill: 'dark' }, { l: 4.35, t: 0.4, w: 1.3, h: 1.3, r: '50%', fill: 'tint' }],
    hat: [5, 2.9, 6.2], side: [5.6, 1.4, 8.6], by: 9.0, nw: 4.4, badge: [6.5, 6.8] },
  gota: { label: 'Gota', body: [1.6, 1.0, 6.8, 8.2, '50% 50% 46% 46% / 58% 58% 42% 42%'], visor: [2.5, 4.1, 5.0, 2.6, '1.3em'], eye: 'pill', gap: 1.4, look: 0.9, parts: [],
    hat: [5, 2.0, 4.4], side: [5.4, 1.6, 8.4], by: 9.2, nw: 3.6, badge: [6.3, 6.8] },
  ciclope: { label: 'Ciclope', body: [1.2, 1.6, 7.6, 7.6, '50%'], visor: [3.0, 3.4, 4.0, 4.0, '50%'], eye: 'cyc', gap: 1.2, look: 0.55,
    parts: [{ l: 4.45, t: 0.7, w: 1.1, h: 1.6, r: '0.55em 0.55em 0.2em 0.2em', fill: 'dark' }, { l: 0.5, t: 4.7, w: 1.3, h: 1.6, r: '0.45em', fill: 'dark' }, { l: 8.2, t: 4.7, w: 1.3, h: 1.6, r: '0.45em', fill: 'dark' }],
    hat: [5, 2.6, 5.2], side: [5.4, 1.2, 8.8], by: 9.2, nw: 3.8, badge: [6.6, 6.8] },
  tela: { label: 'Tela', body: [0.9, 2.8, 8.2, 6.4, '1.4em'], visor: [1.7, 3.6, 6.6, 4.0, '0.9em'], eye: 'pixel', gap: 1.6, look: 1.1,
    parts: [{ l: 3.4, t: 0.7, w: 0.26, h: 2.6, r: '0.13em', fill: 'dark', tf: 'rotate(-30deg)' }, { l: 6.34, t: 0.7, w: 0.26, h: 2.6, r: '0.13em', fill: 'dark', tf: 'rotate(30deg)' }, { l: 1.78, t: 0.6, w: 0.9, h: 0.9, r: '50%', fill: 'tint' }, { l: 7.32, t: 0.6, w: 0.9, h: 0.9, r: '50%', fill: 'tint' }],
    hat: [5, 3.1, 7.0], side: [5.8, 0.9, 9.1], by: 9.2, nw: 4.4, badge: [7.1, 7.0] },
  urso: { label: 'Urso', body: [1.6, 2.2, 6.8, 6.8, '50%'], visor: [2.6, 4.2, 4.8, 2.6, '1.3em'], eye: 'round', gap: 1.2, look: 0.8,
    parts: [{ l: 1.3, t: 1.4, w: 2.2, h: 2.2, r: '50%', fill: 'body' }, { l: 1.85, t: 1.95, w: 1.1, h: 1.1, r: '50%', fill: 'dark' }, { l: 6.5, t: 1.4, w: 2.2, h: 2.2, r: '50%', fill: 'body' }, { l: 7.05, t: 1.95, w: 1.1, h: 1.1, r: '50%', fill: 'dark' }],
    hat: [5, 3.0, 4.8], side: [5.6, 1.6, 8.4], by: 9.0, nw: 4.0, badge: [6.4, 6.7] },
  cupula: { label: 'Cúpula', body: [0.9, 2.4, 8.2, 6.4, '4.1em 4.1em 1.6em 1.6em / 3.6em 3.6em 1.6em 1.6em'], visor: [1.8, 4.3, 6.4, 2.4, '1.2em'], eye: 'pill', gap: 1.7, look: 1,
    parts: [{ l: 4.3, t: 1.5, w: 1.4, h: 1.3, r: '0.7em 0.7em 0.2em 0.2em', fill: 'dark' }, { l: 4.6, t: 0.9, w: 0.8, h: 0.8, r: '50%', fill: 'tint' }],
    hat: [5, 3.2, 5.4], side: [5.6, 0.9, 9.1], by: 8.8, nw: 4.4, badge: [6.9, 6.5] },
  losango: { label: 'Losango', body: [2.0, 2.0, 6.0, 6.0, '1.5em'], btf: 'rotate(45deg)', visor: [2.7, 3.8, 4.6, 2.5, '1.25em'], eye: 'round', gap: 1.2, look: 0.75, parts: [],
    hat: [5, 2.2, 3.2], side: [5.0, 0.8, 9.2], by: 8.4, nw: 2.6, badge: [5.3, 6.5] },
  nuvem: { label: 'Nuvem', body: [1.0, 3.6, 8.0, 5.2, '2.6em'], visor: [2.0, 4.6, 6.0, 2.8, '1.4em'], eye: 'round', gap: 1.5, look: 1,
    bsh: 'inset -0.45em -0.55em 0 rgba(0, 0, 0, 0.2), 0 0.6em 1.4em rgba(0, 0, 0, 0.35)',
    parts: [{ l: 1.6, t: 2.6, w: 3.2, h: 3.2, r: '50%', fill: 'body' }, { l: 3.8, t: 1.5, w: 4.0, h: 4.0, r: '50%', fill: 'body' }, { l: 6.3, t: 2.9, w: 2.8, h: 2.8, r: '50%', fill: 'body' }],
    hat: [5.8, 2.4, 3.6], side: [6.0, 1.0, 9.0], by: 8.8, nw: 4.4, badge: [6.9, 6.8] },
  hexa: { label: 'Hexa', body: [1.0, 1.8, 8.0, 7.2, '0px'], bcp: 'polygon(25% 0, 75% 0, 100% 50%, 75% 100%, 25% 100%, 0 50%)', visor: [2.4, 4.0, 5.2, 2.8, '0.9em'], eye: 'pill', gap: 1.4, look: 1, parts: [],
    hat: [5, 2.0, 4.6], side: [5.4, 1.0, 9.0], by: 8.9, nw: 3.6, badge: [6.2, 6.5] },
  gato: { label: 'Gato', body: [1.4, 2.8, 7.2, 6.2, '2.4em'], visor: [2.3, 4.2, 5.4, 2.7, '1.35em'], eye: 'pill', gap: 1.5, look: 1,
    parts: [{ l: 1.5, t: 1.0, w: 2.6, h: 3.0, fill: 'body', cp: EAR, tf: 'rotate(-12deg)' }, { l: 2.1, t: 2.0, w: 1.4, h: 1.7, fill: 'dark', cp: EAR, tf: 'rotate(-12deg)', or: '0.7em 2em' }, { l: 5.9, t: 1.0, w: 2.6, h: 3.0, fill: 'body', cp: EAR, tf: 'rotate(12deg)' }, { l: 6.5, t: 2.0, w: 1.4, h: 1.7, fill: 'dark', cp: EAR, tf: 'rotate(12deg)', or: '0.7em 2em' }],
    hat: [5, 3.1, 5.6], side: [5.8, 1.4, 8.6], by: 9.0, nw: 4.4, badge: [6.5, 6.6] },
  totem: { label: 'Totem', body: [2.2, 1.0, 5.6, 8.4, '2.8em'], visor: [2.8, 3.0, 4.4, 2.4, '1.2em'], eye: 'round', gap: 1.1, look: 0.7, parts: [],
    deco: [{ l: 4.55, t: 6.3, w: 0.9, h: 0.9, r: '50%', fill: 'tint' }, { l: 3.7, t: 7.6, w: 2.6, h: 0.32, r: '0.16em', fill: 'dark' }],
    hat: [5, 1.9, 4.2], side: [4.2, 2.2, 7.8], by: 9.4, nw: 3.8, badge: [5.7, 6.6] },
};

/** [largura, altura, border-radius] de cada tipo de olho */
export const EYE_TYPES: Record<EyeKey, [number, number, string]> = {
  pill: [0.95, 1.55, '0.48em'],
  round: [1.25, 1.25, '50%'],
  pixel: [1.15, 1.15, '0.16em'],
  bar: [1.35, 0.62, '0.31em'],
  cat: [1.3, 1.08, '0.55em'],
  cyc: [1.9, 1.9, '50%'],
};

/** Ícones do selo: paths SVG de traço (viewBox 0 0 24 24). */
export const ICONS: Record<IconId, string[]> = {
  sparkle: ['M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8L12 3z', 'M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8L19 15z'],
  clipboard: ['M9 3h6v3H9z', 'M7 4.5H5V21h14V4.5h-2', 'M8.5 13.5l2.5 2.5 4.5-5'],
  briefcase: ['M3.5 8h17v11h-17z', 'M9 8V5.5h6V8', 'M3.5 13h17'],
  kanban: ['M4 4h16v16H4z', 'M9.3 4v16M14.7 4v16', 'M6 8h1.5M11.3 8h1.5M16.7 8h1.5M6 11.5h1.5M11.3 11.5h1.5'],
  calendar: ['M4 6h16v14H4z', 'M4 10h16', 'M8 3.5v4M16 3.5v4'],
  notes: ['M6 3h9l4 4v14H6z', 'M15 3v4h4', 'M9 12h7M9 15.5h7M9 8.5h3'],
  search: ['M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13z', 'M15.5 15.5L20.5 20.5'],
  globe: ['M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z', 'M3 12h18', 'M12 3c2.4 2.6 3.6 5.6 3.6 9s-1.2 6.4-3.6 9c-2.4-2.6-3.6-5.6-3.6-9S9.6 5.6 12 3z'],
  translate: ['M3.5 5.5h9', 'M8 3.5v2c0 4-2.4 7-5 8.5M5.8 9c1.4 2.3 3.4 3.9 6 4.7', 'M13 21l4-10 4 10M14.6 17.2h4.8'],
  code: ['M8 7l-5 5 5 5', 'M16 7l5 5-5 5', 'M13.5 4.5l-3 15'],
  bug: ['M8 9h8v6a4 4 0 0 1-8 0V9z', 'M9 9a3 3 0 0 1 6 0', 'M4 13h4M16 13h4M5 8.5l3 1.5M19 8.5l-3 1.5M5 18l3-2M19 18l-3-2'],
  bag: ['M5 8h14l-1.2 12H6.2L5 8z', 'M9 8V6.5a3 3 0 0 1 6 0V8'],
  box: ['M3 7.5L12 3l9 4.5v9L12 21l-9-4.5v-9z', 'M3 7.5l9 4.5 9-4.5', 'M12 12v9'],
  ship: ['M3 14h18l-2.5 5h-13L3 14z', 'M6 14V9h5v5', 'M11 14V6h6v8'],
  cart: ['M3 4h2.2l2.3 11h10.3L20 7.5H6.3', 'M9.5 20.2a1.2 1.2 0 1 0 0-2.4 1.2 1.2 0 0 0 0 2.4z', 'M17 20.2a1.2 1.2 0 1 0 0-2.4 1.2 1.2 0 0 0 0 2.4z'],
  warehouse: ['M3 20V9l9-5 9 5v11', 'M7.5 20v-7h9v7', 'M7.5 16.5h9'],
  lifebuoy: ['M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z', 'M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8z', 'M5.6 5.6l3.5 3.5M14.9 14.9l3.5 3.5M18.4 5.6l-3.5 3.5M9.1 14.9l-3.5 3.5'],
  megaphone: ['M3 10v4h3l7 4V6l-7 4H3z', 'M16.5 9a4 4 0 0 1 0 6', 'M19 6.5a8 8 0 0 1 0 11'],
  target: ['M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z', 'M16.5 12a4.5 4.5 0 1 1-9 0 4.5 4.5 0 0 1 9 0z', 'M12.6 12a.6.6 0 1 1-1.2 0 .6.6 0 0 1 1.2 0z'],
  chat: ['M4 5h16v11H9.5L5 20V5z', 'M12 13.2l-2.4-2.3a1.5 1.5 0 0 1 2.2-2.1l.2.2.2-.2a1.5 1.5 0 0 1 2.2 2.1L12 13.2z'],
  pen: ['M16.5 3.5l4 4L8 20H4v-4L16.5 3.5z', 'M14 6l4 4'],
  seo: ['M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13z', 'M15.5 15.5L20.5 20.5', 'M7.5 12.5l2-2 1.5 1.5 2.5-2.7'],
  video: ['M3 7h12v10H3z', 'M15 10.5l6-3.5v10l-6-3.5'],
  palette: ['M12 3a9 9 0 0 0 0 18c1.2 0 1.8-.8 1.8-1.7 0-1.3-1.2-1.8-1.2-3 0-1.1.9-1.8 2-1.8H17a4 4 0 0 0 4-4C21 6.6 17 3 12 3z', 'M7.5 11h.01M10 7h.01M15 7.5h.01'],
  chart: ['M4 20h16', 'M5.5 16l4.5-5 3.5 3.5L19 8', 'M15 8h4v4'],
  phone: ['M6.5 3.5h3l1.8 4.6-2.3 1.4a11 11 0 0 0 5.5 5.5l1.4-2.3 4.6 1.8v3a2 2 0 0 1-2.2 2A16 16 0 0 1 4.5 5.7a2 2 0 0 1 2-2.2z'],
  coin: ['M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z', 'M14.8 9.2c-.5-1-1.6-1.6-2.8-1.6-1.6 0-2.8.9-2.8 2.1 0 2.9 5.8 1.6 5.8 4.5 0 1.2-1.3 2.2-3 2.2-1.3 0-2.5-.6-3-1.6', 'M12 5.5v2.1M12 16.4v2.1'],
  calc: ['M6 3h12v18H6z', 'M9 6.5h6v3H9z', 'M9 13.5h.01M12 13.5h.01M15 13.5h.01M9 17h.01M12 17h.01M15 17h.01'],
  ledger: ['M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5v-15z', 'M5 19.5A1.5 1.5 0 0 0 6.5 21H19v-3', 'M9 7.5h6M9 11h4'],
  people: ['M9 11a3.2 3.2 0 1 0 0-6.4A3.2 3.2 0 0 0 9 11z', 'M3 20a6 6 0 0 1 12 0', 'M15.5 5a3.2 3.2 0 0 1 0 6M17.5 14.2A6 6 0 0 1 21 20'],
  heart: ['M12 20s-7.5-4.6-7.5-10.2A4.2 4.2 0 0 1 12 7.3a4.2 4.2 0 0 1 7.5 2.5C19.5 15.4 12 20 12 20z'],
  mail: ['M3 6h18v12H3z', 'M3 6l9 7 9-7'],
  bolt: ['M13 2L4.5 13.5H11L10 22l8.5-11.5H12L13 2z'],
  rocket: ['M9.5 14.5l-2-2C9 7.5 13 4 20 4c0 7-3.5 11-8.5 12.5z', 'M7.5 12.5L4 12l2.5-3.5h4M11.5 16.5l.5 3.5 3.5-2.5v-4', 'M4.5 19.5c.8-2.2 2-3 3.5-2.4'],
  bulb: ['M9 18h6M10 21h4', 'M12 3a6 6 0 0 0-3.6 10.8c.6.5 1 1.3 1.1 2.2h5c.1-.9.5-1.7 1.1-2.2A6 6 0 0 0 12 3z'],
  mic: ['M12 3a3 3 0 0 0-3 3v5a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3z', 'M5.5 11a6.5 6.5 0 0 0 13 0', 'M12 17.5V21'],
};

export const NONE_ICON = ['M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z', 'M5.6 5.6l12.8 12.8'];

export const PALETTE: { name: string; hex: string }[] = [
  { name: 'Grafite', hex: '#4A4F5A' }, { name: 'Prata', hex: '#94A3B8' }, { name: 'Caramelo', hex: '#B8794A' },
  { name: 'Areia', hex: '#D4B483' }, { name: 'Terracota', hex: '#C2410C' }, { name: 'Laranja', hex: '#FF7A1A' },
  { name: 'Âmbar', hex: '#FFB020' }, { name: 'Mostarda', hex: '#CA8A04' }, { name: 'Lima', hex: '#9BD13A' },
  { name: 'Verde', hex: '#22C55E' }, { name: 'Menta', hex: '#6EE7B7' }, { name: 'Turquesa', hex: '#14B8A6' },
  { name: 'Petróleo', hex: '#0F766E' }, { name: 'Céu', hex: '#38BDF8' }, { name: 'Azul', hex: '#3B82F6' },
  { name: 'Marinho', hex: '#1E40AF' }, { name: 'Índigo', hex: '#6366F1' }, { name: 'Ameixa', hex: '#6D28D9' },
  { name: 'Roxo', hex: '#A855F7' }, { name: 'Lavanda', hex: '#A78BFA' }, { name: 'Rosa', hex: '#EC4899' },
  { name: 'Coral', hex: '#FB7185' }, { name: 'Vermelho', hex: '#F2414F' }, { name: 'Vinho', hex: '#9F1239' },
];

/** Rótulos em pt-BR para montar UIs de escolha. */
export const LABELS = {
  eye: { auto: 'Do formato', pill: 'Pílula', round: 'Redondo', pixel: 'Pixel', bar: 'Barra', cat: 'Delineado', cyc: 'Único' } as Record<EyeId, string>,
  head: { none: 'Nenhum', cap: 'Boné', helmet: 'Capacete', phones: 'Fone', headset: 'Headset', beret: 'Boina', eyeshade: 'Viseira', bucket: 'Chapéu', beanie: 'Gorro', flower: 'Flor', pencil: 'Lápis' } as Record<HeadId, string>,
  face: { none: 'Nenhum', glasses: 'Óculos', shades: 'Óculos escuros' } as Record<FaceId, string>,
  ears: { none: 'Nenhum', hoops: 'Argolas', pearls: 'Pérolas' } as Record<EarsId, string>,
  chin: { none: 'Nenhum', beard: 'Barba', stache: 'Bigode' } as Record<ChinId, string>,
  neck: { none: 'Nenhum', bowtie: 'Borboleta', tie: 'Gravata', scarf: 'Cachecol', lanyard: 'Crachá', necklace: 'Colar' } as Record<NeckId, string>,
  cheeks: { none: 'Nenhuma', blush: 'Rosadas' } as Record<CheeksId, string>,
  badgeStyle: { light: 'Claro', dark: 'Escuro', color: 'Na cor' },
  icon: {
    sparkle: 'Brilho', clipboard: 'Prancheta', briefcase: 'Pasta', kanban: 'Kanban', calendar: 'Calendário', notes: 'Notas',
    search: 'Lupa', globe: 'Globo', translate: 'Idiomas', code: 'Código', bug: 'Bug', bag: 'Sacola',
    box: 'Caixa', ship: 'Navio', cart: 'Carrinho', warehouse: 'Armazém', lifebuoy: 'Boia', megaphone: 'Megafone',
    target: 'Alvo', chat: 'Chat', pen: 'Caneta', seo: 'Ranking', video: 'Vídeo', palette: 'Paleta',
    chart: 'Gráfico', phone: 'Telefone', coin: 'Moeda', calc: 'Calculadora', ledger: 'Livro', people: 'Pessoas',
    heart: 'Coração', mail: 'E-mail', bolt: 'Raio', rocket: 'Foguete', bulb: 'Ideia', mic: 'Microfone',
  } as Record<IconId, string>,
  mood: {
    idle: 'Ocioso', blink: 'Piscando', left: 'Olhando ←', right: 'Olhando →', up: 'Olhando ↑', listen: 'Ouvindo…',
    think: 'Pensando…', work: 'Trabalhando…', happy: 'Feliz', surprise: 'Surpreso!', sleep: 'Dormindo', drowsy: 'Acordando…', wake: 'Acordando…',
  },
};

export const DEFAULT_CONFIG: AvatarConfig = {
  shape: 'oval', color: '#9F1239', eye: 'auto', head: 'none', face: 'none', ears: 'hoops',
  chin: 'none', neck: 'necklace', cheeks: 'none', badge: 'sparkle', badgeStyle: 'light',
};

type Acc = Partial<Pick<AvatarConfig, 'head' | 'face' | 'ears' | 'chin' | 'neck' | 'cheeks'>>;

const R = (
  id: string, name: string, role: string, area: string, shape: ShapeId, color: string,
  eye: EyeId, acc: Acc, badge: IconId, desc: string,
): RolePreset => ({
  id, name, role, area, desc, shape, color, eye, badge, badgeStyle: 'light',
  head: 'none', face: 'none', ears: 'none', chin: 'none', neck: 'none', cheeks: 'none', ...acc,
});

/** As 30 funções da equipe Donnabot. A primeira (Donna) é a líder. */
export const ROLES: RolePreset[] = [
  R('donna', 'Donna', 'Assistente geral', 'Líder da equipe', 'oval', '#9F1239', 'cat', { ears: 'hoops', neck: 'necklace' }, 'sparkle', 'A que resolve antes de você pedir: agenda, e-mails, follow-ups e decisões. Coordena todos os outros bots.'),
  R('chief', 'Brief', 'Chief of Staff', 'Gestão', 'losango', '#1E40AF', 'auto', { face: 'glasses', neck: 'tie' }, 'briefcase', 'Seu braço direito: prioridades, pautas, reuniões e cobranças do time.'),
  R('pm', 'Sprint', 'Project Manager', 'Gestão', 'cubo', '#3B82F6', 'auto', { face: 'glasses', neck: 'lanyard' }, 'kanban', 'Quebra projetos em tarefas, acompanha prazos e destrava gargalos.'),
  R('calendar', 'Agenda', 'Calendar', 'Gestão', 'tela', '#FB7185', 'pill', { neck: 'bowtie', cheeks: 'blush' }, 'calendar', 'Marca, remarca e protege seu tempo de foco.'),
  R('notes', 'Ata', 'Notes Taker', 'Gestão', 'totem', '#94A3B8', 'bar', { head: 'pencil', face: 'glasses' }, 'notes', 'Anota reuniões, resume decisões e distribui os próximos passos.'),
  R('research', 'Fonte', 'Researcher', 'Gestão', 'ciclope', '#38BDF8', 'auto', { face: 'glasses' }, 'search', 'Pesquisa a fundo, cruza fontes e entrega o resumo.'),
  R('travel', 'Rota', 'Travel Planner', 'Gestão', 'pilula', '#D4B483', 'round', { head: 'bucket', face: 'shades' }, 'globe', 'Roteiros, voos, hotéis e viagens de negócio, como feiras na China.'),
  R('translator', 'Lingo', 'Tradutor', 'Gestão', 'nuvem', '#0F766E', 'round', { head: 'headset' }, 'translate', 'Traduz conversas com fornecedores, contratos e conteúdos.'),
  R('dev', 'Byte', 'Desenvolvedor', 'Tech', 'tela', '#6366F1', 'pixel', { head: 'phones', face: 'glasses' }, 'code', 'Automações, integrações, scripts e correções.'),
  R('qa', 'Crivo', 'QA', 'Tech', 'cupula', '#CA8A04', 'cyc', { face: 'glasses' }, 'bug', 'Testa tudo antes de ir pro ar e caça bugs sem dó.'),
  R('ecommerce', 'Vitrine', 'E-commerce', 'Operação', 'cubo', '#FF7A1A', 'auto', { neck: 'bowtie' }, 'bag', 'Cuida da loja: pedidos, catálogo e atendimento.'),
  R('amazon', 'Buybox', 'Amazon', 'Operação', 'cupula', '#FFB020', 'auto', { head: 'cap' }, 'box', 'Vigia buybox, preços, estoque FBA e saúde da conta.'),
  R('importacao', 'Cargo', 'Importação', 'Operação', 'hexa', '#C2410C', 'auto', { head: 'beanie', chin: 'beard' }, 'ship', 'Fornecedores, frete, desembaraço e chegada do contêiner.'),
  R('compras', 'Cotação', 'Compras', 'Operação', 'totem', '#9BD13A', 'bar', { face: 'glasses', chin: 'stache' }, 'cart', 'Cota fornecedores, compara preços e negocia prazo.'),
  R('estoque', 'Giro', 'Estoque', 'Operação', 'gota', '#4A4F5A', 'bar', { head: 'helmet' }, 'warehouse', 'Controla giro, ruptura, reposição e inventário.'),
  R('suporte', 'Ajuda', 'Suporte ao cliente', 'Operação', 'urso', '#6EE7B7', 'round', { head: 'headset', cheeks: 'blush' }, 'lifebuoy', 'Responde clientes com paciência, rápido e no tom da marca.'),
  R('marketing', 'Hype', 'Marketing', 'Marketing', 'gato', '#EC4899', 'auto', { face: 'shades' }, 'megaphone', 'Calendário de conteúdo, campanhas e lançamentos.'),
  R('metaads', 'Pixel', 'Meta Ads', 'Marketing', 'hexa', '#6D28D9', 'pixel', { head: 'cap' }, 'target', 'Sobe e otimiza campanhas, de olho em CPA e ROAS.'),
  R('social', 'Feed', 'Redes sociais', 'Marketing', 'nuvem', '#14B8A6', 'auto', { head: 'phones' }, 'chat', 'Posts, respostas, DMs e tendências do momento.'),
  R('escritor', 'Prosa', 'Escritor', 'Marketing', 'gato', '#B8794A', 'pill', { face: 'glasses', neck: 'scarf' }, 'pen', 'Copies, anúncios, e-mails, roteiros e descrições de produto.'),
  R('seo', 'Rank', 'SEO & AEO', 'Marketing', 'pilula', '#9BD13A', 'pill', { head: 'cap', face: 'shades' }, 'seo', 'Faz você aparecer no Google e nas respostas das IAs.'),
  R('youtuber', 'Canal', 'Youtuber', 'Marketing', 'ciclope', '#F2414F', 'auto', { head: 'headset' }, 'video', 'Pautas, roteiros, títulos, thumbs e cortes de vídeo.'),
  R('designer', 'Traço', 'Designer', 'Marketing', 'gota', '#A855F7', 'pill', { head: 'beret' }, 'palette', 'Criativos, fotos de produto, artes e identidade visual.'),
  R('vendas', 'Closer', 'Vendas', 'Vendas', 'losango', '#F2414F', 'auto', { neck: 'tie' }, 'chart', 'Funil, propostas, objeções e fechamento.'),
  R('sdr', 'Prospecta', 'SDR', 'Vendas', 'pilula', '#FB7185', 'pill', { head: 'headset' }, 'phone', 'Prospecção, cadência de contato e agendamento de reuniões.'),
  R('financeiro', 'Saldo', 'Financeiro', 'Finanças', 'cupula', '#22C55E', 'round', { neck: 'tie' }, 'coin', 'Fluxo de caixa, contas a pagar e a receber.'),
  R('contabil', 'Balanço', 'Contabilidade', 'Finanças', 'totem', '#14B8A6', 'round', { face: 'glasses', neck: 'bowtie' }, 'calc', 'Impostos, notas fiscais, DRE e fechamento do mês.'),
  R('bookkeeper', 'Razão', 'Bookkeeper', 'Finanças', 'cubo', '#1E40AF', 'round', { head: 'eyeshade', face: 'glasses' }, 'ledger', 'Lança e concilia cada transação, sem deixar nada escapar.'),
  R('rh', 'Talento', 'RH', 'Pessoas', 'gota', '#FFB020', 'round', { neck: 'lanyard', cheeks: 'blush' }, 'people', 'Vagas, entrevistas, onboarding e o clima do time.'),
  R('psicologa', 'Calma', 'Psicóloga', 'Pessoas', 'urso', '#A78BFA', 'round', { head: 'flower', face: 'glasses', ears: 'pearls', cheeks: 'blush' }, 'heart', 'Te ouve sem julgar, acolhe e ajuda a organizar o que você sente.'),
];

/** Extrai só a aparência de um preset (o que vai pro banco). */
export function configFromRole(r: RolePreset): AvatarConfig {
  return {
    shape: r.shape, color: r.color, eye: r.eye, head: r.head, face: r.face, ears: r.ears,
    chin: r.chin, neck: r.neck, cheeks: r.cheeks, badge: r.badge, badgeStyle: r.badgeStyle,
  };
}
