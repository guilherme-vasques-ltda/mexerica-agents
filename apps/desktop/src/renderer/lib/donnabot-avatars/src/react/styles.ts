export const BOTFACE_CSS = `
@keyframes dbfBob{0%,100%{transform:translateY(0)}50%{transform:translateY(-0.22em)}}
@keyframes dbfBreath{0%,100%{transform:translateY(0.1em) scale(1,1)}50%{transform:translateY(0.1em) scale(1.035,1.02)}}
@keyframes dbfHop{0%,100%{transform:translateY(0) scale(1,1)}35%{transform:translateY(-0.75em) scale(0.97,1.04)}70%{transform:translateY(0) scale(1.05,0.94)}}
@keyframes dbfNod{0%,100%{transform:rotate(0deg) translateY(0)}50%{transform:rotate(2.5deg) translateY(0.12em)}}
@keyframes dbfWave{0%,100%{opacity:0.15}50%{opacity:0.9}}
@keyframes dbfScan{from{transform:translateX(0)}to{transform:translateX(var(--dbf-scan-dx,0px))}}
@keyframes dbfZ{0%{transform:translate(0,0.6em) scale(0.6);opacity:0}25%{opacity:1}100%{transform:translate(1.1em,-1.6em) scale(1.1);opacity:0}}
@keyframes dbfDot{0%,80%,100%{transform:translateY(0);opacity:0.35}40%{transform:translateY(-0.35em);opacity:1}}
@media (prefers-reduced-motion: reduce){.dbf-root,.dbf-root *{animation:none!important;transition:none!important}}
`;

let injected = false;

/** Injeta os keyframes uma única vez no <head>. Seguro para SSR. */
export function ensureBotFaceStyles(): void {
  if (injected || typeof document === 'undefined') return;
  if (document.getElementById('dbf-styles')) { injected = true; return; }
  const el = document.createElement('style');
  el.id = 'dbf-styles';
  el.textContent = BOTFACE_CSS;
  document.head.appendChild(el);
  injected = true;
}
