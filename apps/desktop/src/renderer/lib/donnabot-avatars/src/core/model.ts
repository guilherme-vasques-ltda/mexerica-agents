import { EYE_TYPES, ICONS, SHAPES } from './data';
import type { AvatarConfig, EyeKey, FaceEye, FaceModel, FacePart, LifeState, Mood, PartDef, ShapeDef } from './types';

/*
 * Converte um AvatarConfig + Mood em um "modelo" de desenho: uma lista de peças
 * (divs absolutas) em unidades em. O avatar ocupa uma caixa de 10em x 10em e o
 * tamanho final vem do font-size (size / 10 px). Não depende de framework.
 */

type RGB = [number, number, number];

const r3 = (n: number) => Math.round(n * 1000) / 1000;
const em = (n: number) => r3(n) + 'em';

export function hexToRgb(hex: string): RGB {
  const clean = hex.replace('#', '');
  const full = clean.length === 3 ? clean.split('').map((c) => c + c).join('') : clean;
  const n = parseInt(full, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

const mix = (a: RGB, b: RGB, t: number): RGB => a.map((v, i) => Math.round(v + (b[i] - v) * t)) as RGB;
const css = (c: RGB, a?: number) => (a == null ? 'rgb(' + c.join(', ') + ')' : 'rgba(' + c.join(', ') + ', ' + a + ')');
const luminance = (c: RGB) => 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];

/** Tom claro da cor (usado em olhos e luzes). */
export const tintOf = (hex: string) => css(mix(hexToRgb(hex), [255, 255, 255], 0.58));

interface P {
  l: number; t: number; w: number; h: number;
  r?: string; bg?: string; bw?: string; bc?: string; tf?: string; or?: string; cp?: string; op?: number; sh?: string;
}

function part(o: P): FacePart {
  return {
    l: em(o.l), t: em(o.t), w: em(o.w), h: em(o.h), r: o.r || '0px',
    bg: o.bg || 'transparent', bw: o.bw || '0px', bc: o.bc || 'transparent',
    tf: o.tf || 'none', or: o.or || '50% 100%', cp: o.cp || 'none',
    op: o.op == null ? 1 : o.op, sh: o.sh || 'none',
  };
}

/*
 * Grupos de cor usados para escolher a cor dos acessórios com contraste.
 * Funciona com qualquer hex: além das listas, usa matiz/luminância aproximada.
 */
function colorTraits(hex: string) {
  const c = hexToRgb(hex);
  const [r, g, b] = c;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  let hue = 0;
  if (d > 0) {
    if (max === r) hue = ((g - b) / d) % 6;
    else if (max === g) hue = (b - r) / d + 2;
    else hue = (r - g) / d + 4;
    hue = (hue * 60 + 360) % 360;
  }
  const sat = max === 0 ? 0 : d / max;
  const lum = luminance(c);
  const chromatic = sat > 0.25;
  return {
    warm: chromatic && hue >= 15 && hue < 95,
    redpink: chromatic && (hue >= 320 || hue < 15),
    blue: chromatic && hue >= 160 && hue < 290,
    dark: lum < 95,
  };
}

function accessories(S: ShapeDef, cfg: AvatarConfig, eyeKey: EyeKey, gap: number, asleep: boolean) {
  const back: FacePart[] = [];
  const front: FacePart[] = [];
  const hex = cfg.color;
  const { warm, redpink, blue, dark } = colorTraits(hex);
  const hl = 'inset 0.25em 0.3em 0 rgba(255, 255, 255, 0.22), inset -0.25em -0.3em 0 rgba(0, 0, 0, 0.18)';
  const gold = '#E8C27A';
  const tintC = tintOf(hex);
  const [cx, hy, hw] = S.hat;
  const [sy, xl, xr] = S.side;
  const [vl, vt, vw, vh] = S.visor;
  const vcx = vl + vw / 2;
  const vcy = vt + vh / 2;
  const vb = vt + vh;
  const ch = Math.min(hw * 0.55, 2.6);
  const ink = '#1F2329';
  const brownBody = warm && luminance(hexToRgb(hex)) < 150;
  const hair = brownBody ? '#2A1E16' : '#6B4A2E';
  const nx = 5;
  const ny = Math.min(S.by, 9.0);
  const nw = S.nw || 4.4;

  // Bochechas
  if (cfg.cheeks === 'blush') {
    const bc = redpink ? 'rgba(255, 255, 255, 0.35)' : 'rgba(255, 110, 150, 0.55)';
    const yy = Math.min(vb + 0.2, S.by - 0.9);
    [vl + vw * 0.1, vl + vw * 0.9].forEach((x) => front.push(part({ l: x - 0.65, t: yy, w: 1.3, h: 0.62, r: '50%', bg: bc })));
  }
  // Queixo
  if (cfg.chin === 'beard') {
    const bw = Math.min(vw * 0.8, 5.0);
    const t = vb - 0.15;
    const h = Math.max(Math.min(S.by - t + 0.25, 2.2), 1.3);
    front.push(part({ l: vcx - bw / 2, t, w: bw, h, r: '0.35em 0.35em 50% 50% / 0.35em 0.35em 90% 90%', bg: hair, sh: 'inset -0.25em -0.3em 0 rgba(0, 0, 0, 0.2), inset 0.2em 0.2em 0 rgba(255, 255, 255, 0.1)' }));
  }
  if (cfg.chin === 'stache') {
    const t = vb + 0.1;
    front.push(part({ l: vcx - 1.55, t, w: 1.5, h: 0.72, r: '0.8em 0.15em 0.4em 0.9em / 0.5em 0.15em 0.5em 0.6em', bg: hair, tf: 'rotate(-8deg)', or: '100% 0%' }));
    front.push(part({ l: vcx + 0.05, t, w: 1.5, h: 0.72, r: '0.15em 0.8em 0.9em 0.4em / 0.15em 0.5em 0.6em 0.5em', bg: hair, tf: 'rotate(8deg)', or: '0% 0%' }));
  }
  // Pescoço
  if (cfg.neck === 'bowtie') {
    const c = redpink || (warm && hexToRgb(hex)[1] < 140) ? '#1E293B' : '#E11D48';
    front.push(part({ l: nx - 1.4, t: ny - 0.9, w: 1.2, h: 1.1, bg: c, cp: 'polygon(0 0, 100% 38%, 100% 62%, 0 100%)' }));
    front.push(part({ l: nx + 0.2, t: ny - 0.9, w: 1.2, h: 1.1, bg: c, cp: 'polygon(0 38%, 100% 0, 100% 100%, 0 62%)' }));
    front.push(part({ l: nx - 0.32, t: ny - 0.7, w: 0.64, h: 0.7, r: '0.2em', bg: c, sh: 'inset 0 -0.12em 0 rgba(0, 0, 0, 0.25)' }));
  }
  if (cfg.neck === 'tie') {
    const c = blue ? '#B91C1C' : dark ? '#E2E8F0' : '#1E3A8A';
    front.push(part({ l: nx - 0.5, t: ny - 0.3, w: 1.0, h: 1.35, bg: c, cp: 'polygon(18% 0, 82% 0, 100% 74%, 50% 100%, 0 74%)' }));
    front.push(part({ l: nx - 0.4, t: ny - 0.82, w: 0.8, h: 0.62, r: '0.18em', bg: c, sh: 'inset 0 -0.14em 0 rgba(0, 0, 0, 0.3)' }));
  }
  if (cfg.neck === 'scarf') {
    const c = warm ? '#2563EB' : '#F59E0B';
    front.push(part({ l: nx + nw * 0.12, t: ny - 0.2, w: 0.95, h: 1.55, r: '0.2em 0.2em 0.35em 0.35em', bg: c, tf: 'rotate(-10deg)', or: '50% 0%', sh: 'inset -0.15em 0 0 rgba(0, 0, 0, 0.2)' }));
    front.push(part({ l: nx - nw / 2, t: ny - 0.85, w: nw, h: 1.0, r: '999px', bg: c, sh: hl }));
  }
  if (cfg.neck === 'lanyard') {
    const sc = blue ? '#E11D48' : '#2563EB';
    const cy = ny - 0.35;
    front.push(part({ l: nx - 0.26, t: cy - 1.3, w: 0.13, h: 1.4, bg: sc, tf: 'rotate(-32deg)', or: '50% 100%' }));
    front.push(part({ l: nx + 0.13, t: cy - 1.3, w: 0.13, h: 1.4, bg: sc, tf: 'rotate(32deg)', or: '50% 100%' }));
    front.push(part({ l: nx - 0.65, t: cy, w: 1.3, h: 1.55, r: '0.18em', bg: '#F5F6F8', sh: '0 0.1em 0.25em rgba(0, 0, 0, 0.35)' }));
    front.push(part({ l: nx - 0.65, t: cy, w: 1.3, h: 0.4, r: '0.18em 0.18em 0 0', bg: hex }));
    front.push(part({ l: nx - 0.3, t: cy + 0.55, w: 0.6, h: 0.5, r: '0.1em', bg: '#C9CDD6' }));
    front.push(part({ l: nx - 0.45, t: cy + 1.18, w: 0.9, h: 0.14, r: '999px', bg: '#C9CDD6' }));
  }
  if (cfg.neck === 'necklace') {
    const R = nw * 0.42;
    const cyc = ny - 0.9;
    for (let i = 0; i < 9; i++) {
      const th = ((20 + i * 17.5) * Math.PI) / 180;
      const px = nx + Math.cos(th) * R;
      const py = cyc + Math.sin(th) * R * 0.45;
      front.push(part({ l: px - 0.26, t: py - 0.26, w: 0.52, h: 0.52, r: '50%', bg: '#F8F4EC', sh: 'inset -0.08em -0.1em 0 rgba(0, 0, 0, 0.18)' }));
    }
    front.push(part({ l: nx - 0.3, t: cyc + R * 0.45 + 0.05, w: 0.6, h: 0.6, r: '0.12em', bg: gold, tf: 'rotate(45deg)', or: '50% 50%', sh: '0 0 0.5em rgba(232, 194, 122, 0.7)' }));
  }
  // Orelhas
  if (cfg.ears === 'hoops' || cfg.ears === 'pearls') {
    [xl + 0.15, xr - 0.15].forEach((ex) => {
      const ey = sy + 0.7;
      if (cfg.ears === 'hoops') {
        front.push(part({ l: ex - 0.55, t: ey, w: 1.1, h: 1.2, r: '50%', bw: '0.2em', bc: gold, sh: '0 0 0.35em rgba(232, 194, 122, 0.55)' }));
      } else {
        front.push(part({ l: ex - 0.12, t: ey - 0.1, w: 0.24, h: 0.45, r: '0.12em', bg: gold }));
        front.push(part({ l: ex - 0.4, t: ey + 0.3, w: 0.8, h: 0.8, r: '50%', bg: '#F8F4EC', sh: 'inset -0.12em -0.15em 0 rgba(0, 0, 0, 0.15), inset 0.1em 0.1em 0 rgba(255, 255, 255, 0.8)' }));
      }
    });
  }
  // Rosto
  if (cfg.face === 'glasses' || cfg.face === 'shades') {
    const T = EYE_TYPES[eyeKey];
    const shades = cfg.face === 'shades';
    const lens = shades ? 'rgba(0, 0, 0, 0.74)' : 'rgba(255, 255, 255, 0.07)';
    if (eyeKey === 'cyc') {
      const d = Math.min(T[0] + (shades ? 1.2 : 0.9), vw - 0.2);
      front.push(part({ l: vcx - d / 2, t: vcy - d / 2, w: d, h: d, r: '50%', bg: lens, bw: '0.22em', bc: gold }));
      if (shades) {
        front.push(part({ l: vcx - d * 0.3, t: vcy - d * 0.3, w: d * 0.3, h: 0.16, r: '999px', bg: 'rgba(255, 255, 255, 0.6)', tf: 'rotate(-25deg)', or: '50% 50%' }));
      } else {
        front.push(part({ l: vcx + d * 0.33, t: vcy + d * 0.33 - 0.06, w: 2.0, h: 0.12, bg: gold, tf: 'rotate(62deg)', or: '0% 50%' }));
      }
    } else {
      const c = gap / 2 + T[0] / 2;
      const fw = Math.min(T[0] + (shades ? 1.3 : 1.0), 2 * c - 0.3);
      const fh = Math.max(Math.min(T[1] + (shades ? 1.0 : 0.85), vh + (shades ? 0.6 : 0.1)), shades ? 1.6 : 1.35);
      const fr = shades ? '0.35em 0.35em 48% 48% / 0.35em 0.35em 58% 58%' : eyeKey === 'round' ? '50%' : '0.6em';
      const fc = shades ? gold : '#EEF0F4';
      [-1, 1].forEach((sgn) => {
        const x = vcx + sgn * c;
        front.push(part({ l: x - fw / 2, t: vcy - fh / 2, w: fw, h: fh, r: fr, bg: lens, bw: shades ? '0.16em' : '0.2em', bc: fc }));
        if (shades) front.push(part({ l: x - fw * 0.3, t: vcy - fh * 0.3, w: fw * 0.32, h: 0.16, r: '999px', bg: 'rgba(255, 255, 255, 0.6)', tf: 'rotate(-25deg)', or: '50% 50%' }));
      });
      const bx = 2 * c - fw;
      front.push(part({ l: vcx - bx / 2 - 0.05, t: vcy - fh * 0.25, w: bx + 0.1, h: 0.2, r: '0.1em', bg: fc }));
    }
  }
  // Cabeça
  if (cfg.head === 'phones' || cfg.head === 'headset') {
    const phones = cfg.head === 'phones';
    const top = hy - 1.3;
    back.push(part({ l: xl - 0.35, t: top, w: xr - xl + 0.7, h: (sy - top) * 2, r: '50%', bw: phones ? '0.45em' : '0.3em', bc: `${ink} ${ink} transparent ${ink}` }));
    const cw = phones ? 1.5 : 1.2;
    const chh = phones ? 2.8 : 2.2;
    const cups = phones ? [xl - cw * 0.55, xr - cw * 0.45] : [xl - cw * 0.55];
    cups.forEach((x) => {
      front.push(part({ l: x, t: sy - chh / 2, w: cw, h: chh, r: em(cw / 2), bg: ink, sh: 'inset 0.2em 0.25em 0 rgba(255, 255, 255, 0.12)' }));
      front.push(part({ l: x + cw * 0.32, t: sy - chh * 0.28, w: cw * 0.36, h: chh * 0.56, r: '999px', bg: tintC, op: asleep ? 0.35 : 0.9 }));
    });
    if (!phones) {
      const sx = xl + 0.15;
      const sY = sy + chh * 0.28;
      const ex = vcx - 0.7;
      const ey = Math.min(vb + 0.6, S.by - 0.3);
      const dx = ex - sx;
      const dy = ey - sY;
      const len = Math.sqrt(dx * dx + dy * dy);
      const ang = (Math.atan2(dy, dx) * 180) / Math.PI;
      front.push(part({ l: sx, t: sY - 0.12, w: len, h: 0.24, r: '0.12em', bg: ink, tf: `rotate(${r3(ang)}deg)`, or: '0% 50%' }));
      front.push(part({ l: ex - 0.42, t: ey - 0.42, w: 0.84, h: 0.84, r: '50%', bg: ink, sh: '0 0 0 0.12em ' + tintC }));
    }
  }
  if (cfg.head === 'cap') {
    const light = dark || blue;
    const c = light ? '#EEF0F4' : '#1E293B';
    const brim = light ? '#C9CDD6' : '#0F172A';
    front.push(part({ l: cx - hw / 2, t: hy - ch, w: hw, h: ch + 0.1, r: '50% 50% 0.3em 0.3em / 90% 90% 0.3em 0.3em', bg: c, sh: hl }));
    front.push(part({ l: cx - 0.06, t: hy - ch + 0.3, w: 0.12, h: ch - 0.45, bg: 'rgba(0, 0, 0, 0.16)' }));
    front.push(part({ l: cx - 0.28, t: hy - ch - 0.14, w: 0.56, h: 0.34, r: '50%', bg: c }));
    const d = Math.min(1.1, ch * 0.42);
    front.push(part({ l: cx - d / 2, t: hy - ch * 0.52 - d / 2, w: d, h: d, r: '50%', bg: hex, sh: '0 0 0 0.1em ' + c }));
    front.push(part({ l: cx - hw * 0.64, t: hy - 0.24, w: hw * 1.28, h: 0.56, r: '50%', bg: brim }));
  }
  if (cfg.head === 'helmet') {
    const c = warm ? '#EEF0F4' : '#FFB020';
    const hh = ch * 1.05;
    front.push(part({ l: cx - hw * 0.56, t: hy - hh, w: hw * 1.12, h: hh + 0.1, r: '50% 50% 0.2em 0.2em / 95% 95% 0.2em 0.2em', bg: c, sh: hl }));
    front.push(part({ l: cx - hw * 0.08, t: hy - hh, w: hw * 0.16, h: hh, r: '50% 50% 0 0 / 22% 22% 0 0', bg: 'rgba(255, 255, 255, 0.3)' }));
    front.push(part({ l: cx - hw * 0.74, t: hy - 0.28, w: hw * 1.48, h: 0.6, r: '999px', bg: c, sh: 'inset 0 -0.18em 0 rgba(0, 0, 0, 0.22)' }));
    const d = Math.min(hh * 0.5, 1.3);
    const ly = hy - hh * 0.55;
    front.push(part({ l: cx - d / 2, t: ly - d / 2, w: d, h: d, r: '50%', bg: '#2A2D33' }));
    const d2 = d * 0.62;
    front.push(part({ l: cx - d2 / 2, t: ly - d2 / 2, w: d2, h: d2, r: '50%', bg: asleep ? '#5A5648' : '#FFF4C2', sh: asleep ? 'none' : '0 0 0.9em rgba(255, 236, 160, 0.95)' }));
  }
  if (cfg.head === 'beret') {
    const c = dark ? '#BE123C' : '#1F2329';
    front.push(part({ l: cx + 0.17, t: hy - 1.9, w: 0.26, h: 0.6, r: '0.13em', bg: c }));
    front.push(part({ l: cx - hw * 0.62 + 0.3, t: hy - 1.45, w: hw * 1.24, h: 1.6, r: '50% 50% 42% 42% / 75% 75% 25% 25%', bg: c, tf: 'rotate(-7deg)', or: '50% 50%', sh: hl }));
    front.push(part({ l: cx - hw * 0.46, t: hy - 0.25, w: hw * 0.92, h: 0.42, r: '999px', bg: c, sh: 'inset 0 -0.12em 0 rgba(0, 0, 0, 0.3)' }));
  }
  if (cfg.head === 'eyeshade') {
    front.push(part({ l: cx - hw * 0.55, t: hy - 0.5, w: hw * 1.1, h: 0.42, r: '999px', bg: '#14532D' }));
    front.push(part({ l: cx - hw * 0.62, t: hy - 0.2, w: hw * 1.24, h: 1.25, bg: 'rgba(34, 197, 94, 0.62)', cp: 'polygon(0 0, 100% 0, 86% 100%, 14% 100%)' }));
    front.push(part({ l: cx - hw * 0.62, t: hy - 0.2, w: hw * 1.24, h: 0.16, bg: 'rgba(187, 247, 208, 0.7)' }));
  }
  if (cfg.head === 'bucket') {
    const c = warm ? '#2F6B5E' : '#C8B48A';
    const bh = Math.min(ch * 0.85, 2.1);
    front.push(part({ l: cx - hw * 0.4, t: hy - bh - 0.2, w: hw * 0.8, h: bh + 0.3, r: '0.5em 0.5em 0 0', bg: c, sh: hl }));
    front.push(part({ l: cx - hw * 0.4, t: hy - 0.75, w: hw * 0.8, h: 0.38, bg: 'rgba(0, 0, 0, 0.28)' }));
    front.push(part({ l: cx - hw * 0.72, t: hy - 0.4, w: hw * 1.44, h: 0.85, r: '0.25em 0.25em 50% 50% / 0.25em 0.25em 100% 100%', bg: c, sh: 'inset 0 -0.15em 0 rgba(0, 0, 0, 0.2)' }));
  }
  if (cfg.head === 'beanie') {
    const c = redpink || brownBody ? '#1E293B' : '#DC2626';
    front.push(part({ l: cx - 0.45, t: hy - ch - 0.55, w: 0.9, h: 0.9, r: '50%', bg: '#F5F6F8' }));
    front.push(part({ l: cx - hw * 0.52, t: hy - ch, w: hw * 1.04, h: ch + 0.1, r: '50% 50% 0.2em 0.2em / 90% 90% 0.2em 0.2em', bg: c, sh: hl }));
    front.push(part({ l: cx - hw * 0.56, t: hy - 0.75, w: hw * 1.12, h: 0.9, r: '0.35em', bg: c, sh: 'inset 0 0.18em 0 rgba(255, 255, 255, 0.15), inset 0 -0.18em 0 rgba(0, 0, 0, 0.25)' }));
  }
  if (cfg.head === 'flower') {
    const fx = cx + hw * 0.34;
    const fy = hy - 0.15;
    const pc = redpink ? '#FFFFFF' : '#F9A8D4';
    for (let i = 0; i < 5; i++) {
      const a = ((i * 72 - 90) * Math.PI) / 180;
      front.push(part({ l: fx + Math.cos(a) * 0.55 - 0.45, t: fy + Math.sin(a) * 0.55 - 0.45, w: 0.9, h: 0.9, r: '50%', bg: pc, sh: 'inset -0.1em -0.12em 0 rgba(0, 0, 0, 0.12)' }));
    }
    front.push(part({ l: fx - 0.3, t: fy - 0.3, w: 0.6, h: 0.6, r: '50%', bg: '#FACC15' }));
  }
  if (cfg.head === 'pencil') {
    const a = 28;
    const s = Math.sin((a * Math.PI) / 180);
    const k = Math.cos((a * Math.PI) / 180);
    const px = xr - 0.9;
    const py = hy - 0.3;
    const seg = (d: number, w: number, h: number, bg: string, cp?: string, r?: string) =>
      part({ l: px + d * s - w / 2, t: py - d * k - h / 2, w, h, bg, cp, r, tf: `rotate(${a}deg)`, or: '50% 50%' });
    back.push(seg(0, 0.46, 2.4, '#FACC15'));
    back.push(seg(1.32, 0.46, 0.26, '#C9CDD6'));
    back.push(seg(1.6, 0.46, 0.34, '#FB7185', undefined, '0.12em 0.12em 0 0'));
    back.push(seg(-1.45, 0.46, 0.5, '#F5D0A9', 'polygon(0 0, 100% 0, 50% 100%)'));
  }
  return { back, front };
}

export const STILL_LIFE: LifeState = { blink: false, glancing: false, gx: 0, gy: 0 };

/**
 * Monta o modelo de desenho.
 * @param size tamanho final em px (lado do quadrado do avatar)
 * @param life piscadas/olhadas automáticas (use STILL_LIFE para estático)
 */
export function buildFace(cfg: AvatarConfig, mood: Mood = 'idle', size = 48, life: LifeState = STILL_LIFE): FaceModel {
  const S = SHAPES[cfg.shape] || SHAPES.cubo;
  const base = hexToRgb(cfg.color);
  const tint = mix(base, [255, 255, 255], 0.58);
  const dark = css(mix(base, [0, 0, 0], 0.4));
  const tintC = css(tint);
  const glow = css(base, 0.8);
  const asleep = mood === 'sleep';

  const conv = (p: PartDef): FacePart => {
    let bg = dark;
    let sh = 'none';
    if (p.fill === 'tint') { bg = asleep ? dark : tintC; sh = asleep ? 'none' : '0 0 0.8em ' + glow; }
    if (p.fill === 'gold') { bg = asleep ? '#8A7448' : '#E8C27A'; sh = asleep ? 'none' : '0 0 0.7em rgba(232, 194, 122, 0.8)'; }
    if (p.fill === 'body') { bg = cfg.color; sh = 'inset 0.3em 0.35em 0 rgba(255, 255, 255, 0.22), inset -0.25em -0.3em 0 rgba(0, 0, 0, 0.15)'; }
    return part({ l: p.l, t: p.t, w: p.w, h: p.h, r: p.r, tf: p.tf, or: p.or, cp: p.cp, bg, sh });
  };

  const eyeKey: EyeKey = cfg.eye && cfg.eye !== 'auto' ? cfg.eye : S.eye;
  const gap = eyeKey === 'cyc' ? 0 : S.gap || 1.2;
  const acc = accessories(S, cfg, eyeKey, gap, asleep);

  // Olhos
  const T = EYE_TYPES[eyeKey];
  let w = T[0];
  let h = T[1];
  let r = T[2];
  let tf = 'scale(1, 1)';
  const blinking = life.blink || mood === 'blink';
  if (mood === 'happy') {
    w = eyeKey === 'cyc' ? 2.1 : Math.max(T[0], 1.2);
    h = r3(w * 0.5);
    r = `${em(w / 2)} ${em(w / 2)} 0.12em 0.12em / ${em(h)} ${em(h)} 0.12em 0.12em`;
  }
  if (mood === 'surprise' || mood === 'wake') tf = 'scale(1.28, 1.28)';
  if (mood === 'work') tf = 'scale(1, 0.7)';
  if (mood === 'listen') tf = 'scale(1, 0.62)';
  if (mood === 'drowsy') tf = 'scale(1, 0.4)';
  if (asleep) tf = 'scale(1.1, 0.12)';
  if (blinking && mood !== 'happy' && !asleep) tf = 'scale(1.1, 0.1)';
  const eye: FaceEye = {
    w: em(w), h: em(h), r, tf,
    bg: asleep ? css(tint, 0.5) : tintC,
    sh: asleep ? 'none' : eyeKey === 'cyc' ? 'inset 0 0 0 0.4em rgba(0, 0, 0, 0.28), 0 0 0.7em ' + glow : '0 0 0.55em ' + glow,
  };
  let eyes: FaceEye[];
  if (eyeKey === 'cyc') {
    eyes = [eye];
  } else if (eyeKey === 'cat' && mood !== 'happy') {
    eyes = [
      { ...eye, r: '0.15em 0.65em 0.65em 0.65em / 0.15em 0.55em 0.55em 0.55em' },
      { ...eye, r: '0.65em 0.15em 0.65em 0.65em / 0.55em 0.15em 0.55em 0.55em' },
    ];
  } else {
    eyes = [eye, { ...eye }];
  }

  // Direção do olhar
  const looks: Partial<Record<Mood, [number, number]>> = {
    left: [-0.75, 0], right: [0.75, 0], up: [0, -0.5], think: [0.55, -0.42], work: [0, 0.3],
    listen: [0, 0.15], happy: [0, -0.12], surprise: [0, -0.1], wake: [0, -0.1],
  };
  let [lx, ly] = looks[mood] || [0, 0];
  const glancing = mood === 'idle' && life.glancing;
  if (glancing) { lx = life.gx; ly = life.gy; }
  lx = r3(lx * S.look);
  ly = r3(ly * S.look);

  // Pose do corpo
  const poses: Partial<Record<Mood, string>> = {
    left: 'translate(-0.15em, 0em) rotate(-5deg)', right: 'translate(0.15em, 0em) rotate(5deg)', up: 'translate(0em, -0.12em) rotate(0deg)',
    think: 'translate(0em, 0em) rotate(4deg)', surprise: 'translate(0em, -0.6em) rotate(0deg)', wake: 'translate(0em, -0.35em) rotate(0deg)',
    sleep: 'translate(0em, 0.25em) rotate(-6deg)', drowsy: 'translate(0em, 0.12em) rotate(-3deg)', listen: 'translate(-0.1em, 0.05em) rotate(-7deg)',
  };
  let pose = poses[mood] || 'translate(0em, 0em) rotate(0deg)';
  if (glancing) pose = `translate(0em, 0em) rotate(${lx > 0 ? 2 : lx < 0 ? -2 : 0}deg)`;

  const bob = asleep ? 'dbfBreath 4.6s ease-in-out infinite'
    : mood === 'happy' ? 'dbfHop 0.62s ease-in-out infinite'
    : mood === 'work' ? 'dbfBob 1.1s ease-in-out infinite'
    : mood === 'listen' ? 'dbfNod 2.8s ease-in-out infinite'
    : 'dbfBob 3.4s ease-in-out infinite';
  const filter = asleep ? 'saturate(0.55) brightness(0.78)' : mood === 'drowsy' ? 'saturate(0.8) brightness(0.9)' : 'none';

  const bs = cfg.badgeStyle || 'light';
  const badge = cfg.badge && cfg.badge !== 'none' && ICONS[cfg.badge]
    ? {
        l: em(S.badge[0]), t: em(S.badge[1]),
        bg: bs === 'dark' ? '#15171C' : bs === 'color' ? cfg.color : '#F5F6F8',
        fg: bs === 'dark' ? tintC : bs === 'color' ? '#FFFFFF' : '#15171C',
        ring: bs === 'color' ? '#0F1116' : dark,
        paths: ICONS[cfg.badge],
      }
    : null;

  return {
    fs: r3(size / 10) + 'px',
    bob, pose, filter, color: cfg.color,
    body: {
      l: em(S.body[0]), t: em(S.body[1]), w: em(S.body[2]), h: em(S.body[3]), r: S.body[4],
      tf: S.btf || 'none', cp: S.bcp || 'none',
      sh: S.bsh || 'inset -0.45em -0.55em 0 rgba(0, 0, 0, 0.2), inset 0.35em 0.4em 0 rgba(255, 255, 255, 0.22), 0 0.6em 1.4em rgba(0, 0, 0, 0.35)',
    },
    visor: { l: em(S.visor[0]), t: em(S.visor[1]), w: em(S.visor[2]), h: em(S.visor[3]), r: S.visor[4] },
    gap: em(gap),
    look: `translate(${lx}em, ${ly}em)`,
    eyes,
    back: S.parts.map(conv).concat(acc.back),
    front: (S.deco || []).map(conv).concat(acc.front),
    scan: mood === 'work',
    scanColor: css(tint, 0.3),
    zzz: asleep,
    dots: mood === 'think',
    waves: mood === 'listen',
    tint: tintC,
    badge,
  };
}
