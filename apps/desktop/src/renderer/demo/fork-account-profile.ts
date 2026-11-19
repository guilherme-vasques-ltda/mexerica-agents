import type { AccountProfile, AccountProfileBridge, ProfileResult } from '@shared/fork-account-profile'

// E6b, só na demo: uma conta de exemplo em memória, sem Clerk nem rede. O código 123456 confirma o
// autenticador; qualquer outro é recusado como o Clerk recusaria. Nada é gravado em disco.

export function demoAccountProfileBridge(): AccountProfileBridge {
  const profile: AccountProfile = {
    firstName: 'Ana', lastName: 'Exemplo', email: 'ana@exemplo.com', hasPassword: true, totp: false, backupCodes: false,
    linked: [{ id: 'eac_demo', provider: 'google', email: 'ana@exemplo.com', verified: true }], linkable: [],
    passwordRules: { minLength: 12, maxLength: null, requireUppercase: false, requireLowercase: false, requireNumbers: false, requireSpecial: false },
  }
  const ok = (): ProfileResult => ({ ok: true, value: structuredClone(profile) })
  const codes = () => ['k7d2m9qa', 'p3x8w1zr', 'h5n6c4vt', 'b2j9e7ls', 'f8q1y6ud', 'r4t7g3ka', 'm9v2s5ho', 'w6c8n1pe', 'z3k5b7tj', 'e1u4r8gx']
  return {
    load: async () => ok(),
    saveName: async (first, last) => { profile.firstName = first.trim(); profile.lastName = last.trim(); return ok() },
    setPassword: async (current, next) => {
      if (profile.hasPassword && current !== 'senha-atual-demo') return { ok: false, failure: 'password_incorrect', message: 'A senha atual não confere.' }
      if (next.length < 12) return { ok: false, failure: "password_rules", message: "Use pelo menos 12 caracteres." }
      profile.hasPassword = true; return ok()
    },
    link: async () => { profile.linked = [{ id: 'eac_demo', provider: 'google', email: profile.email, verified: true }]; profile.linkable = []; return ok() },
    unlink: async () => {
      if (!profile.hasPassword) return { ok: false, failure: 'last_method', message: 'Esse é o seu único jeito de entrar. Crie uma senha antes de desligar o Google.' }
      profile.linked = []; profile.linkable = ['google']; return ok()
    },
    startTotp: async () => ({ ok: true, value: { uri: 'otpauth://totp/Donnabot:ana%40exemplo.com?secret=JBSWY3DPEHPK3PXPJBSWY3DP&issuer=Donnabot', secret: 'JBSWY3DPEHPK3PXPJBSWY3DP' } }),
    confirmTotp: async (code) => {
      if (code.replace(/\s/g, '') !== '123456') return { ok: false, failure: 'code_invalid', message: 'Código incorreto ou vencido. Confira o app autenticador e tente de novo.' }
      profile.totp = true; profile.backupCodes = true; return { ok: true, value: { codes: codes() } }
    },
    disableTotp: async () => { profile.totp = false; profile.backupCodes = false; return ok() },
    regenerateCodes: async () => ({ ok: true, value: { codes: codes() } }),
    saveCodes: async () => 'cancelled',
    startReverify: async () => ({ ok: true, value: { status: 'complete', factors: [] } }),
    sendReverifyCode: async () => ({ ok: true, value: { status: 'needs_first_factor', factors: ['email_code'] } }),
    attemptReverify: async () => ({ ok: true, value: { status: 'complete', factors: [] } }),
  }
}
