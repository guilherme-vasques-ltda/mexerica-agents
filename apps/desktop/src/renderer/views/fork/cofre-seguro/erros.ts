// MX-277: qual texto a janela segura mostra para cada erro de salvar. Login novo não tem pedido do bot: a recusa não manda
// "pedir de novo ao bot", e a colisão (já existe login para o site e usuário) manda trocar a senha em Extensões → Credenciais.
import type { ErroSalvar } from '../../../../shared/fork-cofre-seguro.ts'

const chaveErro = {
  vazio: 'extensoes.cofre.erro.vazio', conexao: 'extensoes.cofre.erro.conexao',
  recusado: 'extensoes.cofre.erro.recusado', fechada: 'extensoes.cofre.erro.fechada', jaExiste: 'extensoes.cofre.erro.jaExiste',
} as const satisfies Record<ErroSalvar, string>

export const chaveDoErro = (erro: ErroSalvar, loginNovo: boolean) =>
  loginNovo && erro === 'recusado' ? ('extensoes.cofre.erro.recusadoNovo' as const) : chaveErro[erro]
