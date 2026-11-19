// MX-277: o texto de cada erro da janela segura. Login novo não manda "pedir de novo ao bot" (não há pedido).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { chaveDoErro } from './erros.ts'
import { ptBR } from '../../../lib/fork-i18n/pt-BR.ts'

test('login novo: a recusa tem texto próprio, sem pedido nem bot; a colisão manda trocar a senha em Extensões → Credenciais', () => {
  assert.equal(chaveDoErro('recusado', true), 'extensoes.cofre.erro.recusadoNovo')
  assert.equal(chaveDoErro('recusado', false), 'extensoes.cofre.erro.recusado', 'o pedido do bot continua como antes')
  for (const novo of [true, false]) assert.equal(chaveDoErro('jaExiste', novo), 'extensoes.cofre.erro.jaExiste')
  const texto = ptBR['extensoes.cofre.erro.recusadoNovo']
  assert.equal(/pedido|pedir|bot/i.test(texto), false, texto)
  assert.equal(ptBR['extensoes.cofre.erro.jaExiste'], 'Já existe um login para este site e usuário. Para trocar a senha, use Extensões → Credenciais.')
  for (const erro of ['vazio', 'conexao', 'fechada'] as const) assert.equal(chaveDoErro(erro, true), chaveDoErro(erro, false))
})
