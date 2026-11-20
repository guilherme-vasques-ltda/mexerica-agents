// Seletor de modelo: o selo diz por qual conexão o modelo roda. Só aparece quando a conexão é certa; na dúvida, sem selo.
// - Motor Mexerica: a chave do fornecedor (`fornecedor/modelo`), grupo «Chaves de API».
// - Codex e Claude Code: o plano, quando a conta é de assinatura; a chave, quando a conta é paga por uso; conta de
//   cobrança não confirmada mostra só o nome da conexão, sem dizer se é plano nem chave.

export type SeloDoModelo =
  | { grupo: 'planos' | 'chaves'; nome: string }
  | { grupo: null; nome: string }

/** Os fornecedores de chave do motor Mexerica, pelo prefixo do id do modelo. */
const FORNECEDORES: Readonly<Record<string, string>> = {
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  google: 'Google',
  xai: 'xAI',
  openrouter: 'OpenRouter',
  meta: 'Meta',
}

const MOTORES_DE_CONTA = new Set(['codex', 'claude'])

export function seloDoModelo(opcoes: {
  backend: string
  modelo: string
  /** O nome do motor como o Hub o chama (`Provedor.name`). */
  nomeDoMotor: string
  /** A cobrança da conta que roda o modelo, quando o cadastro a informa. */
  cobranca?: string | null
}): SeloDoModelo | null {
  if (opcoes.backend === 'mexerica') {
    const nome = Object.hasOwn(FORNECEDORES, opcoes.modelo.split('/')[0]!) ? FORNECEDORES[opcoes.modelo.split('/')[0]!] : undefined
    return nome ? { grupo: 'chaves', nome } : null
  }
  if (!MOTORES_DE_CONTA.has(opcoes.backend)) return null
  if (opcoes.cobranca === 'subscription') return { grupo: 'planos', nome: opcoes.nomeDoMotor }
  if (opcoes.cobranca === 'paid_api') return { grupo: 'chaves', nome: opcoes.nomeDoMotor }
  return { grupo: null, nome: opcoes.nomeDoMotor }
}
