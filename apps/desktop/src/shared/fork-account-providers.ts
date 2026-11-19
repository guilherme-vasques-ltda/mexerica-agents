import type { SignInProvider } from './analytics'

/** D177 (dono, 01/11/2026): Apple, X e Google, nessa ordem, ao lado de e-mail e senha. */
export const SOCIAL_PROVIDERS = ['apple', 'x', 'google'] as const satisfies readonly SignInProvider[]

/** Na ordem da D177, só os provedores que a instância Clerk tem ligados (lista do Main, fail-closed). */
export function visibleProviders(enabled: readonly SignInProvider[]): SignInProvider[] {
  return SOCIAL_PROVIDERS.filter(p => enabled.includes(p))
}

/**
 * E7c: o caminho para quem não tem conta. "Criar conta" só com o cadastro público e as regras do Main;
 * "Entrar na lista de espera" só com o modo `waitlist` (ou a recusa do Clerk por lista de espera);
 * modo desconhecido ou ilegível → nenhum dos dois (fail-closed).
 */
export function newAccountOffer(mode: 'public' | 'waitlist' | null, signUp: { minPasswordLength: number } | null, refused = false): 'signup' | 'waitlist' | null {
  if (mode === 'waitlist' || refused) return 'waitlist'
  return mode === 'public' && signUp ? 'signup' : null
}
