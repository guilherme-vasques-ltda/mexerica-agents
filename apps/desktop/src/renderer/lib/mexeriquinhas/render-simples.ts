// SPDX-License-Identifier: Apache-2.0
// Gerador de SVG das Mexeriquinhas: texto puro, sem DOM, sem filtros e sem raster.
// Grade de 128 × 128. Até 40 px o desenho simplifica, mas o selo de estado nunca some.

import { estadoPorId, type EstadoId } from './estado.ts';
import {
  corPorId,
  normalizarConfig,
  type AcessorioId,
  type AvatarConfig,
  type ObjetoId,
  type OlhosId,
  type RoupaId,
} from './pecas.ts';

export interface RenderOptions {
  /** Lado em px (atributos width/height). */
  size: number;
  /** Estado visual; padrão "livre". */
  state?: EstadoId;
  /** Prefixo dos ids internos (gradientes, clip, title). Sanitizado; padrão mxq1, mxq2… */
  id?: string;
  /** Rótulo acessível; padrão "<nome>, <estado>". */
  label?: string;
}

/** Até este lado (px) o avatar usa a versão simplificada. */
export const COMPACTO_ATE = 40;

const TINTA = '#2a1a12';
const CORPO = 'M64 24C91 24 108 44 108 72C108 96 94 109 64 109C34 109 20 96 20 72C20 44 37 24 64 24Z';
const GOLA = 'M0 94Q64 84 128 94V128H0Z';

let contador = 0;

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (ch) => `&#${ch.charCodeAt(0)};`);
}

function idSeguro(id: string | undefined): string {
  const limpo = (id ?? '').replace(/[^A-Za-z0-9_-]/g, '');
  if (!limpo) return `mxq${++contador}`;
  return /^[A-Za-z]/.test(limpo) ? limpo : `mxq-${limpo}`;
}

const traco = (d: string, cor: string, w: number, extra = ''): string =>
  `<path d="${d}" fill="none" stroke="${cor}" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round"${extra}/>`;

const ROUPA_COR: Record<RoupaId, string | null> = {
  nenhuma: null,
  blazer: '#2c3e6b',
  colete: '#f3efe6',
  moletom: '#d9b98b',
  avental: null,
  jaleco: '#f2f2ee',
  polo: '#2e8b83',
};

function roupa(r: RoupaId, compacto: boolean): string {
  const base = ROUPA_COR[r];
  let s = base ? `<path d="${GOLA}" fill="${base}"/>` : '';
  switch (r) {
    case 'blazer':
      s += '<path d="M53 89L64 106L75 89Q64 87 53 89Z" fill="#f4f1ea"/>';
      if (!compacto) s += traco('M53 89L61 103M75 89L67 103', '#1d2b4f', 2);
      break;
    case 'colete':
      s += '<path d="M0 94Q40 86 54 89L64 110V128H0ZM128 94Q88 86 74 89L64 110V128H128Z" fill="#4c7a3c"/>';
      break;
    case 'moletom':
      s += traco('M42 91Q64 104 86 91', '#bf9d6c', compacto ? 6 : 5);
      if (!compacto) s += traco('M58 98V109M70 98V109', '#f6efe2', 2.2);
      break;
    case 'avental':
      s += '<path d="M47 91Q64 88 81 91V128H47Z" fill="#ecdcbc"/>';
      if (!compacto) s += traco('M48 92L38 87M80 92L90 87M55 104H73V112H55Z', '#c8b086', 2);
      break;
    case 'jaleco':
      s += '<path d="M54 89L64 104L74 89Z" fill="#2e4b74"/>';
      if (!compacto) s += traco('M54 89L62 106M74 89L66 106M80 102H91', '#c9c9c0', 2);
      break;
    case 'polo':
      s += '<path d="M52 89L58 98L64 91ZM76 89L70 98L64 91Z" fill="#44aaa0"/>';
      if (!compacto) s += traco('M64 91V104', '#22706a', 1.8);
      break;
    case 'nenhuma':
      break;
  }
  return s;
}

type Olho = 'ponto' | 'sonolento' | 'brilhante' | 'piscando' | 'fechado' | 'aberto' | 'xx';

function olhoDaIdentidade(o: OlhosId): Olho {
  if (o === 'sonolentos') return 'sonolento';
  if (o === 'brilhantes') return 'brilhante';
  if (o === 'piscando') return 'piscando';
  return 'ponto';
}

function olhos(tipo: Olho, compacto: boolean): string {
  const w = compacto ? 4.4 : 2.8;
  const r = compacto ? 6.6 : 5.2;
  const ponto = (x: number, rr = r): string =>
    `<circle cx="${x}" cy="70" r="${rr}" fill="${TINTA}"/>` +
    (compacto ? '' : `<circle cx="${x + 1.8}" cy="68.2" r="1.7" fill="#fff"/>`);
  switch (tipo) {
    case 'ponto':
      return ponto(48) + ponto(80);
    case 'aberto':
      return ponto(48, r + 0.8) + ponto(80, r + 0.8);
    case 'sonolento':
      return `<path d="M42 69A6 6 0 0 0 54 69ZM74 69A6 6 0 0 0 86 69Z" fill="${TINTA}"/>` + traco('M41 69H55M73 69H87', TINTA, w);
    case 'brilhante': {
      const estrela = (x: number): string =>
        `<path d="M${x} 65.5L${x + 1.2} 68.8L${x + 4.5} 70L${x + 1.2} 71.2L${x} 74.5L${x - 1.2} 71.2L${x - 4.5} 70L${x - 1.2} 68.8Z" fill="#fff"/>`;
      const c = (x: number): string => `<circle cx="${x}" cy="70" r="${r + 1.6}" fill="${TINTA}"/>`;
      return c(48) + c(80) + (compacto ? '' : estrela(48) + estrela(80));
    }
    case 'piscando':
      return ponto(48) + traco('M74 72Q80 64 86 72', TINTA, w);
    case 'fechado':
      return traco('M42 69Q48 75 54 69M74 69Q80 75 86 69', TINTA, w);
    case 'xx':
      return traco('M43 65L53 70L43 75M85 65L75 70L85 75', TINTA, w);
  }
}

function oculos(o: OlhosId, compacto: boolean): string {
  const w = compacto ? 4 : 3;
  const hastes = traco('M37 68L27 65M91 68L101 65', '#1e1a18', w);
  if (o === 'oculos-redondos') {
    return (
      `<path d="M38 70a10 10 0 1 0 20 0a10 10 0 1 0-20 0ZM70 70a10 10 0 1 0 20 0a10 10 0 1 0-20 0Z" fill="#fff" fill-opacity=".14" stroke="#1e1a18" stroke-width="${w}"/>` +
      traco('M58 69Q64 65 70 69', '#1e1a18', w) +
      hastes
    );
  }
  if (o === 'oculos-escuros' || o === 'oculos-verdes') {
    const lente = o === 'oculos-verdes' ? '#20b455' : '#141416';
    return (
      `<path d="M36 62H59V69A9 9 0 0 1 50 78H45A9 9 0 0 1 36 69ZM69 62H92V69A9 9 0 0 1 83 78H78A9 9 0 0 1 69 69Z" fill="${lente}" stroke="#0d0d0f" stroke-width="2" stroke-linejoin="round"/>` +
      traco('M59 64H69', '#0d0d0f', w) +
      hastes +
      (compacto ? '' : traco('M40 65.5H46M73 65.5H79', '#fff', 2, ' stroke-opacity=".45"'))
    );
  }
  return '';
}

function sobrancelhas(estado: EstadoId, comOculos: boolean, compacto: boolean): string {
  const y = comOculos ? 54 : 59;
  const w = compacto ? 4.2 : 2.8;
  if (estado === 'trabalhando') return traco(`M41 ${y}L53 ${y + 3}M87 ${y}L75 ${y + 3}`, TINTA, w);
  if (estado === 'precisa-resposta') return traco(`M42 ${y + 3}L53 ${y}M86 ${y + 3}L75 ${y}`, TINTA, w);
  return '';
}

function boca(estado: EstadoId, compacto: boolean): string {
  const w = compacto ? 4.4 : 2.8;
  switch (estado) {
    case 'livre':
      return traco('M57 82Q64 89 71 82', TINTA, w);
    case 'trabalhando':
      return traco('M58 84Q64 86 70 84', TINTA, w);
    case 'dormindo':
      return traco('M60 84Q64 87 68 84', TINTA, w);
    case 'precisa-resposta':
      return `<ellipse cx="64" cy="85" rx="${compacto ? 4.4 : 3.2}" ry="${compacto ? 5 : 3.8}" fill="${TINTA}"/>`;
    case 'erro':
      return traco('M56 86q2 -3 4 0t4 0t4 0t4 0', TINTA, w);
  }
}

const COBRE_FOLHA: ReadonlySet<AcessorioId> = new Set(['coroa', 'capacete', 'boina', 'bone', 'cartola']);

function acessorio(a: AcessorioId, compacto: boolean): string {
  switch (a) {
    case 'coroa':
      return (
        '<path d="M40 42L36 20L51 31L64 14L77 31L92 20L88 42Z" fill="#f4b731" stroke="#c98b14" stroke-width="1.5" stroke-linejoin="round"/>' +
        (compacto
          ? ''
          : '<path d="M33 20a3 3 0 1 0 6 0a3 3 0 1 0-6 0ZM61 14a3 3 0 1 0 6 0a3 3 0 1 0-6 0ZM89 20a3 3 0 1 0 6 0a3 3 0 1 0-6 0Z" fill="#ffd45e"/><circle cx="64" cy="35" r="2.8" fill="#e04848"/>')
      );
    case 'fone':
      return (
        traco('M20 72C18 12 110 12 108 72', '#3a3a42', 6) +
        '<rect x="12" y="60" width="15" height="26" rx="7" fill="#34343a" stroke="#4e4e58" stroke-width="1.5"/><rect x="101" y="60" width="15" height="26" rx="7" fill="#34343a" stroke="#4e4e58" stroke-width="1.5"/>' +
        (compacto ? '' : traco('M20 85Q24 98 43 96', '#34343a', 3) + '<circle cx="46" cy="96" r="3.6" fill="#34343a"/>')
      );
    case 'capacete':
      return (
        '<path d="M30 52C30 20 98 20 98 52Z" fill="#f6c12a"/>' +
        (compacto ? '' : traco('M64 24V49', '#ffdc72', 5)) +
        '<path d="M22 52Q64 44 106 52Q107 57 102 57Q64 50 26 57Q21 57 22 52Z" fill="#d9a015"/>'
      );
    case 'boina':
      return (
        '<path d="M26 46C20 30 46 18 74 20C100 22 110 34 102 46C86 52 42 52 26 46Z" fill="#d63c3c"/>' +
        '<circle cx="72" cy="19" r="3.2" fill="#b02c2c"/>' +
        (compacto ? '' : traco('M30 46C46 50 84 50 100 45', '#a92a2a', 2))
      );
    case 'bone':
      return (
        '<path d="M30 50C30 20 98 20 98 50Z" fill="#f3b13a"/>' +
        '<path d="M34 47Q18 48 10 56Q30 58 52 50Z" fill="#d8962a"/>' +
        (compacto ? '' : traco('M64 23V50', '#d8962a', 1.5) + '<circle cx="64" cy="23" r="2.6" fill="#d8962a"/>')
      );
    case 'flor':
      return (
        '<g transform="translate(90 32)" fill="#f58fb3"><circle cy="-6.8" r="6.3"/><circle cx="6.5" cy="-2.1" r="6.3"/><circle cx="4" cy="5.5" r="6.3"/><circle cx="-4" cy="5.5" r="6.3"/><circle cx="-6.5" cy="-2.1" r="6.3"/>' +
        '<circle r="4.2" fill="#ffcf3a"/></g>'
      );
    case 'cartola':
      return (
        '<ellipse cx="64" cy="42" rx="36" ry="6.5" fill="#1f1f24" stroke="#4a4a54" stroke-width="1.5"/>' +
        '<path d="M44 42L46 6H82L84 42Z" fill="#2a2a30" stroke="#4a4a54" stroke-width="1.5" stroke-linejoin="round"/>' +
        '<path d="M45.3 30H82.7L83.3 39H44.7Z" fill="#6c3fa6"/>'
      );
    case 'capuz':
    case 'nenhum':
      return '';
  }
}

const CAPUZ =
  '<path d="M64 14C100 14 118 44 118 76C118 100 108 116 64 116C20 116 10 100 10 76C10 44 28 14 64 14ZM64 46C86 46 98 60 98 76C98 94 84 104 64 104C44 104 30 94 30 76C30 60 42 46 64 46Z" fill="#2b2b31" fill-rule="evenodd" stroke="#4a4a54" stroke-width="2"/>';

function objeto(o: ObjetoId): string {
  switch (o) {
    case 'moeda':
      return '<circle cx="20" cy="86" r="11" fill="#f4b731" stroke="#c98b14" stroke-width="2"/>' +
        traco('M24 81Q20 78 17 80Q14 83 18 85.5Q23 88 22 91Q19 94 15.5 91.5M19.7 77V95', '#9a6a0c', 2);
    case 'chave':
      return traco('M18 80V104M18 97H25M18 102H24', '#e3a823', 3.6) +
        '<circle cx="18" cy="74" r="6" fill="none" stroke="#e3a823" stroke-width="3.6"/>';
    case 'megafone':
      return traco('M24 90L22 99', '#e04848', 4) +
        '<path d="M30 80L9 70V100L30 90Z" fill="#f2eee8" stroke="#cfc6ba" stroke-width="1.5" stroke-linejoin="round"/>' +
        '<ellipse cx="9" cy="85" rx="3.4" ry="15" fill="#e04848"/>';
    case 'notebook':
      return '<rect x="84" y="82" width="30" height="21" rx="2.5" fill="#2c2f37" stroke="#a8aeb8" stroke-width="2"/>' +
        traco('M95 88l-4 4l4 4M103 88l4 4l-4 4M101 87l-4 10', '#8fe3a0', 1.6) +
        '<path d="M80 103H118L115 108H83Z" fill="#c3c8d0"/>';
    case 'lupa':
      return traco('M106 82L116 94', '#6b4a2b', 5) +
        '<circle cx="99" cy="74" r="10" fill="#d7ecff" fill-opacity=".35" stroke="#2c2c32" stroke-width="3.5"/>';
    case 'pincel':
      return traco('M98 106L113 80', '#8b5a2b', 4) + traco('M113 80L116.5 74', '#b9bec6', 5) +
        '<path d="M116 75Q122 66 118 58Q111 64 114 73Z" fill="#e44d8a"/>';
    case 'caixa':
      return '<rect x="5" y="82" width="24" height="20" rx="2" fill="#c99a5b" stroke="#9c7240" stroke-width="1.5"/>' +
        traco('M17 82V90', '#ecd7ad', 3);
    case 'carrinho':
      return traco('M4 80H9L13 96H29L32 84H11', '#3a3a40', 2.6) +
        '<path d="M12.4 101a2.6 2.6 0 1 0 5.2 0a2.6 2.6 0 1 0-5.2 0ZM24.4 101a2.6 2.6 0 1 0 5.2 0a2.6 2.6 0 1 0-5.2 0Z" fill="#3a3a40"/>';
    case 'coracao':
      return '<path d="M20 97C7 89 9 77 15.5 77C18.5 77 20 80 20 80C20 80 21.5 77 24.5 77C31 77 33 89 20 97Z" fill="#f05a7e" stroke="#c93d61" stroke-width="1.2"/>';
    case 'ferramenta':
      return traco('M100 106L113 84', '#8e96a3', 5) +
        '<path d="M108 80a7 7 0 1 0 14 0a7 7 0 1 0-14 0ZM113.2 73.6h3.6V80h-3.6Z" fill="#8e96a3" fill-rule="evenodd"/>';
    case 'gravata':
    case 'nenhum':
      return '';
  }
}

const GRAVATA = '<path d="M61 94H67L68 103L64 109L60 103Z" fill="#d23a3a"/><path d="M60.5 92.5H67.5L66 97H62Z" fill="#b02c2c"/>';

function selo(estado: EstadoId, compacto: boolean): string {
  const tipo = estadoPorId(estado).selo;
  if (!tipo) return '';
  const [cx, cy, r] = compacto ? [99, 29, 22] : [103, 27, 14];
  const fundo = tipo === 'zzz' ? '#3d4b8f' : tipo === '?' ? '#ffc93c' : '#e5484d';
  const tinta = tipo === '?' ? TINTA : '#fff';
  let g: string;
  if (tipo === 'zzz') {
    g = compacto ? traco('M-5-5H5L-5 5H5', tinta, 3.2) : traco('M-6-3H1L-6 5H1M3-8H7L3-3.5H7', tinta, 2.2);
  } else if (tipo === '?') {
    g = traco('M-4-3.5C-4-8.5 4-8.5 4-3.5C4-.5 0-.5 0 2.5', tinta, compacto ? 3.4 : 2.8) +
      `<circle cy="7" r="${compacto ? 2.1 : 1.8}" fill="${tinta}"/>`;
  } else {
    g = traco('M-4.5-4.5L4.5 4.5M4.5-4.5L-4.5 4.5', tinta, compacto ? 3.4 : 3);
  }
  return (
    `<g class="mxq-selo"><circle class="mxq-anel" cx="${cx}" cy="${cy}" r="${r}" fill="${fundo}" stroke="#17120e" stroke-width="${compacto ? 4 : 3}"/>` +
    `<g transform="translate(${cx} ${cy}) scale(${r / 14})">${g}</g></g>`
  );
}

/** Gera o SVG do avatar. Seguro para inserir como HTML: ids e textos são sanitizados. */
export function renderAvatar(config: AvatarConfig, opts: RenderOptions): string {
  const c = normalizarConfig(config);
  const estado = estadoPorId(opts.state ?? 'livre');
  const size = Number.isFinite(opts.size) && opts.size > 0 ? Math.round(opts.size) : 32;
  const compacto = size <= COMPACTO_ATE;
  const id = idSeguro(opts.id);
  const cor = corPorId(c.cor);
  const label = opts.label ?? `${c.nome ?? 'Mexeriquinha'}, ${estado.nome}`;
  const manga = ROUPA_COR[c.roupa];
  const comOculos = c.olhos.startsWith('oculos');
  const objetoId = c.objeto ?? 'nenhum';
  const tipoOlho: Olho =
    estado.id === 'dormindo' ? 'fechado' :
    estado.id === 'precisa-resposta' ? 'aberto' :
    estado.id === 'erro' ? 'xx' :
    olhoDaIdentidade(c.olhos);

  const braco = (manga ? `fill="${manga}"` : `fill="url(#${id}-c)"`);
  const folha =
    traco('M63 26C62 20 62.5 16 64.5 12', '#5b7a2a', compacto ? 5 : 4) +
    '<path d="M65 18C70 6 84 3 95 7C90 19 78 24 65 18Z" fill="#4fa03a"/>' +
    (compacto ? '' : traco('M67 17C75 13 83 10 91 8', '#3b7f2a', 1.2));

  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="${size}" height="${size}" ` +
    `class="mxq mxq--${estado.id}${compacto ? ' mxq--compacto' : ''}" role="img" aria-labelledby="${id}-t">` +
    `<title id="${id}-t">${esc(label)}</title>` +
    '<defs>' +
    `<radialGradient id="${id}-c" cx=".36" cy=".3" r=".8"><stop offset="0" stop-color="${cor.clara}"/><stop offset=".55" stop-color="${cor.base}"/><stop offset="1" stop-color="${cor.escura}"/></radialGradient>` +
    `<linearGradient id="${id}-s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".2"/><stop offset=".45" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity=".22"/></linearGradient>` +
    `<clipPath id="${id}-k"><path d="${CORPO}"/></clipPath>` +
    (compacto ? '' : `<pattern id="${id}-p" width="7" height="7" patternUnits="userSpaceOnUse"><circle cx="1.5" cy="1.5" r=".9" fill="#fff" fill-opacity=".05"/><circle cx="5" cy="4.6" r=".8" fill="#000" fill-opacity=".03"/></pattern>`) +
    '</defs>' +
    '<ellipse class="mxq-sombra" cx="64" cy="115" rx="34" ry="5" fill="#000" fill-opacity=".28"/>' +
    '<g class="mxq-salto"><g class="mxq-fig">' +
    `<ellipse cx="24" cy="90" rx="9" ry="11" transform="rotate(-28 24 90)" ${braco}/>` +
    `<ellipse cx="104" cy="90" rx="9" ry="11" transform="rotate(28 104 90)" ${braco}/>` +
    `<path d="${CORPO}" fill="url(#${id}-c)" stroke="${cor.escura}" stroke-opacity=".55" stroke-width="${compacto ? 2.5 : 1.5}"/>` +
    (compacto ? '' : `<path d="${CORPO}" fill="url(#${id}-p)"/>`) +
    (c.roupa === 'nenhuma' ? '' : `<g clip-path="url(#${id}-k)"><g transform="translate(0 4)">${roupa(c.roupa, compacto)}<path d="${GOLA}" fill="url(#${id}-s)"/></g></g>`) +
    (!compacto && objetoId === 'gravata' ? GRAVATA : '') +
    (c.acessorio === 'capuz' ? CAPUZ : '') +
    `<path d="M31.5 82a6.5 4.5 0 1 0 13 0a6.5 4.5 0 1 0-13 0ZM83.5 82a6.5 4.5 0 1 0 13 0a6.5 4.5 0 1 0-13 0Z" fill="#ff7b8a" fill-opacity=".6"/>` +
    `<g class="mxq-olhos">${olhos(tipoOlho, compacto)}</g>` +
    oculos(c.olhos, compacto) +
    sobrancelhas(estado.id, comOculos, compacto) +
    boca(estado.id, compacto) +
    (c.acessorio === 'nenhum' || c.acessorio === 'capuz' ? '' : `<g transform="translate(0 -6)">${acessorio(c.acessorio, compacto)}</g>`) +
    (COBRE_FOLHA.has(c.acessorio) ? '' : folha) +
    (compacto ? '' : objeto(objetoId)) +
    '</g></g>' +
    selo(estado.id, compacto) +
    '</svg>'
  );
}
