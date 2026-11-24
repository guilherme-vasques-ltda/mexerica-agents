// D224: aviso, sem bloqueio, do que a trava de segurança do Hub não cobre em cada motor.
// Um motor novo entra aqui e no catálogo (conversa.avisos.motor.<motor>); o componente e os pontos de exibição não mudam.
const avisos = {
  opencode: 'conversa.avisos.motor.opencode',
  antigravity: 'extensoes.motor.antigravityPermissoes',
} as const
export type ChaveAvisoMotor = (typeof avisos)[keyof typeof avisos]

/** A chave do aviso do motor escolhido, ou `null` quando não há o que avisar. */
export function avisoDoMotor(backend: string): ChaveAvisoMotor | null {
  return Object.hasOwn(avisos, backend) ? avisos[backend as keyof typeof avisos] : null
}
