import { useCallback, useEffect, useRef, useState } from 'react';
import { STILL_LIFE } from '../core/model';
import type { BaseMood, LifeState, MomentAction, Mood } from '../core/types';

const GLANCES: [number, number][] = [[-0.6, 0], [0.6, 0], [-0.45, -0.35], [0.45, -0.35], [0, 0.3]];

/**
 * "Vida automática": piscadas e olhadas aleatórias enquanto o bot está parado.
 * Cada avatar tem seus próprios timers, então eles não piscam sincronizados.
 */
export function useBotLife(mood: Mood, enabled = true): LifeState {
  const [life, setLife] = useState<LifeState>(STILL_LIFE);

  useEffect(() => {
    if (!enabled || mood === 'sleep' || mood === 'drowsy' || mood === 'wake' || mood === 'blink') {
      setLife(STILL_LIFE);
      return;
    }
    let alive = true;
    const timers = new Set<ReturnType<typeof setTimeout>>();
    const later = (ms: number, fn: () => void) => {
      const id = setTimeout(() => { timers.delete(id); if (alive) fn(); }, ms);
      timers.add(id);
    };
    const blink = (twice: boolean) => {
      setLife((s) => ({ ...s, blink: true }));
      later(150, () => {
        setLife((s) => ({ ...s, blink: false }));
        if (twice) later(170, () => blink(false));
      });
    };
    const step = () => {
      const r = Math.random();
      if (r < 0.55 || mood !== 'idle') {
        blink(r < 0.2);
      } else {
        const g = GLANCES[Math.floor(Math.random() * GLANCES.length)];
        setLife((s) => ({ ...s, glancing: true, gx: g[0], gy: g[1] }));
        if (Math.random() < 0.4) blink(false);
        later(700 + Math.random() * 1000, () => setLife((s) => ({ ...s, glancing: false })));
      }
      later(1600 + Math.random() * 2800, step);
    };
    later(400 + Math.random() * 2600, step);
    return () => {
      alive = false;
      timers.forEach(clearTimeout);
      timers.clear();
    };
  }, [mood, enabled]);

  return life;
}

/**
 * Controla o humor de um bot.
 * - setBase: estado contínuo (idle, listen, think, work, sleep).
 * - play: reação momentânea que volta sozinha para a base
 *   (blink, left, right, up, happy, surprise, wake).
 */
export function useBotMood(initial: BaseMood = 'idle') {
  const [base, setBaseState] = useState<BaseMood>(initial);
  const [temp, setTemp] = useState<Mood | null>(null);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);

  const clear = () => {
    timers.current.forEach(clearTimeout);
    timers.current = [];
  };
  useEffect(() => clear, []);

  const setBase = useCallback((m: BaseMood) => {
    clear();
    setTemp(null);
    setBaseState(m);
  }, []);

  const play = useCallback((action: MomentAction) => {
    clear();
    const seq = (steps: [Mood, number][]) => {
      let t = 0;
      steps.forEach(([m, d]) => {
        const at = t;
        timers.current.push(setTimeout(() => setTemp(m), at));
        t += d;
      });
      timers.current.push(setTimeout(() => setTemp(null), t));
    };
    switch (action) {
      case 'blink': seq([['blink', 180]]); break;
      case 'left':
      case 'right':
      case 'up': seq([[action, 1500]]); break;
      case 'happy': seq([['happy', 2000]]); break;
      case 'surprise': seq([['surprise', 1300]]); break;
      case 'wake':
        setBaseState((b) => (b === 'sleep' ? 'idle' : b));
        seq([['drowsy', 650], ['blink', 200], ['wake', 700], ['left', 550], ['right', 550]]);
        break;
    }
  }, []);

  return { mood: (temp ?? base) as Mood, base, setBase, play };
}
