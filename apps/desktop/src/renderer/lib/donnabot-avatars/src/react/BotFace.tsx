import { memo, useEffect, useMemo, type CSSProperties } from 'react';
import { buildFace, STILL_LIFE } from '../core/model';
import type { AvatarConfig, FacePart, Mood } from '../core/types';
import { useBotLife } from './hooks';
import { ensureBotFaceStyles } from './styles';

export interface BotFaceProps {
  config: AvatarConfig;
  /** Estado visual. Para reações momentâneas use useBotMood().play */
  mood?: Mood;
  /** Lado do avatar em px. O desenho escala inteiro. Padrão 48. */
  size?: number;
  /** Piscadas e olhadas aleatórias. Padrão true. */
  autoLife?: boolean;
  /** Texto acessível (aria-label). Ex.: "Donna, assistente geral" */
  label?: string;
  className?: string;
  style?: CSSProperties;
}

const abs: CSSProperties = { position: 'absolute', left: 0, top: 0, width: '10em', height: '10em' };

const partStyle = (p: FacePart): CSSProperties => ({
  position: 'absolute', boxSizing: 'border-box',
  left: p.l, top: p.t, width: p.w, height: p.h, borderRadius: p.r,
  backgroundColor: p.bg, borderStyle: 'solid', borderWidth: p.bw, borderColor: p.bc,
  transform: p.tf, transformOrigin: p.or, clipPath: p.cp, opacity: p.op, boxShadow: p.sh,
  transition: 'background-color 250ms ease, opacity 300ms ease',
});

const zStyle = (size: string, delay: string): CSSProperties => ({
  position: 'absolute', left: 0, bottom: 0, fontWeight: 800, fontSize: size, lineHeight: 1,
  fontFamily: 'ui-rounded, system-ui, sans-serif', color: '#C9CDD6', opacity: 0,
  animation: `dbfZ 2.4s ease-out ${delay} infinite`,
});

function BotFaceImpl({ config, mood = 'idle', size = 48, autoLife = true, label, className, style }: BotFaceProps) {
  useEffect(ensureBotFaceStyles, []);
  const life = useBotLife(mood, autoLife);
  const f = useMemo(() => buildFace(config, mood, size, autoLife ? life : STILL_LIFE), [config, mood, size, life, autoLife]);
  const dot = (delay: string): CSSProperties => ({
    width: '0.6em', height: '0.6em', borderRadius: '50%', backgroundColor: f.tint, animation: `dbfDot 1.2s ease-in-out ${delay} infinite`,
  });
  const wave = (l: string, t: string, s: string, delay: string): CSSProperties => ({
    position: 'absolute', left: l, top: t, width: s, height: s, boxSizing: 'border-box', borderRadius: '50%',
    borderStyle: 'solid', borderWidth: '0.2em', borderColor: `transparent ${f.tint} transparent transparent`,
    animation: `dbfWave 1.6s ease-in-out ${delay} infinite`,
  });

  return (
    <div
      className={'dbf-root' + (className ? ' ' + className : '')}
      role="img"
      aria-label={label ?? 'Avatar do bot'}
      style={{ position: 'relative', flex: 'none', width: '10em', height: '10em', fontSize: f.fs, ...style }}
    >
      <div style={{ ...abs, transformOrigin: '50% 90%', animation: f.bob }}>
        <div style={{ ...abs, transformOrigin: '50% 80%', transform: f.pose, filter: f.filter, transition: 'transform 420ms cubic-bezier(0.34, 1.56, 0.64, 1), filter 600ms ease' }}>
          {f.back.map((p, i) => <div key={'b' + i} style={partStyle(p)} />)}
          <div
            style={{
              position: 'absolute', left: f.body.l, top: f.body.t, width: f.body.w, height: f.body.h, borderRadius: f.body.r,
              transform: f.body.tf, clipPath: f.body.cp, backgroundColor: f.color, boxShadow: f.body.sh,
              transition: 'background-color 400ms ease',
            }}
          />
          <div
            style={{
              position: 'absolute', overflow: 'hidden', left: f.visor.l, top: f.visor.t, width: f.visor.w, height: f.visor.h,
              borderRadius: f.visor.r, backgroundColor: '#0F1116', boxShadow: 'inset 0 0.3em 0.5em rgba(0, 0, 0, 0.6)',
            }}
          >
            <div
              style={{
                position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center',
                gap: f.gap, transform: f.look, transition: 'transform 260ms cubic-bezier(0.34, 1.4, 0.64, 1)',
              }}
            >
              {f.eyes.map((e, i) => (
                <div
                  key={i}
                  style={{
                    flex: 'none', width: e.w, height: e.h, borderRadius: e.r, backgroundColor: e.bg, boxShadow: e.sh, transform: e.tf,
                    transition: 'transform 110ms ease-out, width 220ms ease, height 220ms ease, border-radius 220ms ease, background-color 500ms ease, box-shadow 500ms ease',
                  }}
                />
              ))}
            </div>
            {f.scan && (
              <div style={{ position: 'absolute', left: '6%', top: 0, width: '0.45em', height: '100%', backgroundColor: f.scanColor, animation: 'dbfScan 1.1s ease-in-out infinite alternate', ['--dbf-scan-dx' as string]: `calc(${f.visor.w} * 0.82)` }} />
            )}
            <div style={{ position: 'absolute', left: '14%', top: '12%', width: '20%', height: '12%', borderRadius: 999, backgroundColor: 'rgba(255, 255, 255, 0.08)' }} />
          </div>
          {f.front.map((p, i) => <div key={'f' + i} style={partStyle(p)} />)}
          {f.badge && (
            <div
              style={{
                position: 'absolute', left: f.badge.l, top: f.badge.t, width: '2.6em', height: '2.6em', borderRadius: '50%',
                backgroundColor: f.badge.bg, color: f.badge.fg, display: 'flex', alignItems: 'center', justifyContent: 'center',
                boxShadow: `0 0 0 0.2em ${f.badge.ring}, 0 0.25em 0.6em rgba(0, 0, 0, 0.35)`,
              }}
            >
              <svg viewBox="0 0 24 24" width="60%" height="60%" fill="none" stroke="currentColor" strokeWidth={size < 64 ? 2.4 : 2.2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                {f.badge.paths.map((d, i) => <path key={i} d={d} />)}
              </svg>
            </div>
          )}
        </div>
      </div>
      {f.zzz && (
        <div style={{ position: 'absolute', left: '7.2em', top: '0.6em', width: '2.4em', height: '2.4em' }}>
          <div style={zStyle('1.5em', '0s')}>z</div>
          <div style={zStyle('1.2em', '0.8s')}>z</div>
          <div style={zStyle('0.95em', '1.6s')}>z</div>
        </div>
      )}
      {f.dots && (
        <div style={{ position: 'absolute', left: '7.3em', top: '0.9em', display: 'flex', gap: '0.28em' }}>
          <div style={dot('0s')} />
          <div style={dot('0.2s')} />
          <div style={dot('0.4s')} />
        </div>
      )}
      {f.waves && (
        <div style={{ position: 'absolute', left: '-1.1em', top: '3.6em', width: '2.4em', height: '2.4em' }}>
          <div style={wave('0.9em', '0.6em', '1.2em', '0.3s')} />
          <div style={wave('0.2em', '0em', '2.4em', '0s')} />
        </div>
      )}
    </div>
  );
}

/** Avatar animado de um bot Donnabot. */
export const BotFace = memo(BotFaceImpl);
