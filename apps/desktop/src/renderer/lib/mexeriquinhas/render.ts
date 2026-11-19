// SPDX-License-Identifier: Apache-2.0
import { estadoPorId, type EstadoId } from './estado.ts';
import { conjuntoPorId, type ConjuntoAvatar } from './conjuntos.ts';
export interface RenderPeluciaOptions {
  size: number;
  state?: EstadoId;
  assetRoot?: string;
  label?: string;
  paused?: boolean;
}
export const PELUCIA_COMPACTO_ATE = 40;
// Degrau do arquivo-fonte: 64 px até 32 de exibição, 128 até 64, 256 acima (MX-79).
const pastaDaPelucia = (size: number): 64 | 128 | 256 => size <= 32 ? 64 : size <= 64 ? 128 : 256;
export const QUADROS = ['livre-1', 'livre-2', 'piscar', 'trabalhando-1', 'trabalhando-2', 'precisa-resposta', 'erro', 'dormindo-1', 'dormindo-2'] as const;
const esc = (s: string): string => s.replace(/[&<>"']/g, (ch) => `&#${ch.charCodeAt(0)};`);
// Somente caminhos relativos ou absolutos locais: nenhuma URL remota ou protocolo executável.
function caminho(s: string): string | undefined {
  return /^(?:\.?\.?\/|\/)[A-Za-z0-9_./-]*$/.test(s) && !s.startsWith('//') ? s.replace(/\/?$/, '/') : undefined;
}
export function renderPelucia(conjunto: ConjuntoAvatar | string, opts: RenderPeluciaOptions): string {
  const c = typeof conjunto === 'string' ? conjuntoPorId(conjunto) ?? conjuntoPorId('base')! : conjunto;
  const estado = estadoPorId(opts.state ?? 'livre');
  const size = Number.isFinite(opts.size) && opts.size > 0 ? Math.min(1024, Math.round(opts.size)) : 32;
  const pasta = pastaDaPelucia(size);
  const id = /^[a-z0-9-]+$/i.test(c.id) ? c.id : 'base';
  const root = caminho(c.baseUrl ?? '') ?? `${caminho(opts.assetRoot ?? './assets/') ?? './assets/'}${id}/`;
  const quadros = estado.id === 'livre' ? ['livre-1', 'livre-2', 'piscar'] :
    estado.id === 'trabalhando' ? ['trabalhando-1', 'trabalhando-2'] :
    estado.id === 'dormindo' ? ['dormindo-1', 'dormindo-2'] : [estado.id];
  const imagens = quadros.map((q, i) => `<img class="mxp-quadro mxp-quadro--${i}" src="${esc(root)}${pasta}/${q}.webp" width="${size}" height="${size}" alt="" aria-hidden="true" draggable="false" loading="lazy" decoding="async">`).join('');
  const selo = estado.selo ? `<span class="mxp-selo" aria-hidden="true">${estado.selo === 'x' ? '×' : estado.selo}</span>` : '';
  return `<span class="mxp mxp--${estado.id}${size <= PELUCIA_COMPACTO_ATE ? ' mxp--compacto' : ''}${opts.paused ? ' mxp--pausado' : ''}" role="img" aria-label="${esc(opts.label ?? `${c.nome}, ${estado.nome}`)}" style="width:${size}px;height:${size}px"><span class="mxp-fig">${imagens}</span>${selo}</span>`;
}
