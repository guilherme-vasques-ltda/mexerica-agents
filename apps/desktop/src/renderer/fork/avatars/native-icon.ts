// O menu nativo requer PNG: adaptador estático do modelo da própria biblioteca.
import { buildFace, STILL_LIFE, type AvatarConfig, type FacePart } from '../../lib/donnabot-avatars/src'

const n = (s: string) => parseFloat(s) || 0
export function drawAvatarIcon(ctx: CanvasRenderingContext2D, config: AvatarConfig, size: number) {
  const f = buildFace(config, 'idle', size, STILL_LIFE)
  ctx.save()
  ctx.scale(size / 10, size / 10)
  const rect = (x: number, y: number, w: number, h: number, r: string, color: string) => {
    ctx.beginPath()
    const radius = r.includes('%') ? Math.min(w, h) * n(r) / 100 : n(r)
    ctx.roundRect(x, y, w, h, Math.max(0, Math.min(w / 2, h / 2, radius)))
    ctx.fillStyle = color
    ctx.fill()
  }
  const part = (p: FacePart) => {
    ctx.save()
    ctx.globalAlpha = p.op
    const x = n(p.l), y = n(p.t), w = n(p.w), h = n(p.h)
    const angle = /rotate\(([-\d.]+)deg\)/.exec(p.tf)
    if (angle) { ctx.translate(x + w / 2, y + h / 2); ctx.rotate(n(angle[1]!) * Math.PI / 180); ctx.translate(-x - w / 2, -y - h / 2) }
    if (p.cp.startsWith('polygon(')) {
      const points = p.cp.slice(8, -1).split(',').map((point) => point.trim().split(/\s+/).map(n))
      ctx.beginPath()
      points.forEach(([px = 0, py = 0], i) => i ? ctx.lineTo(x + px * w / 100, y + py * h / 100) : ctx.moveTo(x + px * w / 100, y + py * h / 100))
      ctx.closePath(); ctx.clip()
    }
    rect(x, y, w, h, p.r, p.bg)
    if (n(p.bw)) { ctx.strokeStyle = p.bc; ctx.lineWidth = n(p.bw); ctx.stroke() }
    ctx.restore()
  }
  f.back.forEach(part)
  part({ ...f.body, bg: f.color, bw: '0', bc: 'transparent', op: 1, or: '50% 50%', sh: 'none' })
  rect(n(f.visor.l), n(f.visor.t), n(f.visor.w), n(f.visor.h), f.visor.r, '#0F1116')
  const total = f.eyes.reduce((sum, eye) => sum + n(eye.w), 0) + n(f.gap) * (f.eyes.length - 1)
  let x = n(f.visor.l) + (n(f.visor.w) - total) / 2
  for (const eye of f.eyes) {
    rect(x, n(f.visor.t) + (n(f.visor.h) - n(eye.h)) / 2, n(eye.w), n(eye.h), eye.r, eye.bg)
    x += n(eye.w) + n(f.gap)
  }
  f.front.forEach(part)
  if (f.badge) {
    const { l, t, bg, fg, paths } = f.badge
    rect(n(l), n(t), 2.6, 2.6, '50%', bg)
    ctx.save(); ctx.translate(n(l) + 0.52, n(t) + 0.52); ctx.scale(1.56 / 24, 1.56 / 24)
    ctx.strokeStyle = fg; ctx.lineWidth = 2.4; ctx.lineCap = 'round'; ctx.lineJoin = 'round'
    for (const path of paths) ctx.stroke(new Path2D(path))
    ctx.restore()
  }
  ctx.restore()
}
