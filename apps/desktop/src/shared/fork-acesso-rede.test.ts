import assert from 'node:assert/strict'
import test from 'node:test'
import { normalizarFalhaRemota } from './fork-acesso-rede.ts'

const RECUSAS_RPC = [
  ['auth_invalid', 'renovar_sessao', true, false, false],
  ['device_unpaired', 'parear_de_novo', false, false, false],
  ['device_revoked', 'parear_de_novo', false, true, true],
  ['device_lease_expired', 'reabrir_canal', false, false, false],
  ['workspace_forbidden', 'tirar_workspace', false, true, true],
  ['role_insufficient', 'recusar_acao', false, false, false],
  ['cross_workspace_reference', 'descartar_intencao', false, false, false],
  ['workspace_epoch_mismatch', 'limpar_e_reabrir', false, false, true],
] as const

test('cada recusa estável do RPC/HTTP vira uma decisão de UX com a ação esperada', () => {
  for (const [code, acao, renovarUmaVez, terminal, invalidaEstado] of RECUSAS_RPC) {
    assert.deepEqual(normalizarFalhaRemota({ origem: 'rpc', code }), {
      motivo: code, acao, renovarUmaVez, terminal, invalidaEstado, encerraStream: false,
    }, code)
  }
})

test('canal usa stableCode para o motivo e ignora o code legado', () => {
  const decisao = normalizarFalhaRemota({ origem: 'canal', reject: { code: 'revoked', stableCode: 'device_revoked' } })
  assert.equal(decisao.motivo, 'device_revoked')
  assert.equal(decisao.acao, 'parear_de_novo')
  assert.equal(normalizarFalhaRemota({ origem: 'canal', reject: { code: 'revoked' } }).motivo, 'desconhecido')
})

test('revogado é terminal e invalida estado; não pareado e lease expirado não são', () => {
  const revogado = normalizarFalhaRemota({ origem: 'canal', reject: { stableCode: 'device_revoked' } })
  const naoPareado = normalizarFalhaRemota({ origem: 'rpc', code: 'device_unpaired' })
  const lease = normalizarFalhaRemota({ origem: 'rpc', code: 'device_lease_expired' })
  assert.equal(revogado.terminal, true)
  assert.equal(revogado.invalidaEstado, true)
  assert.equal(naoPareado.terminal, false)
  assert.equal(naoPareado.invalidaEstado, false)
  assert.equal(lease.terminal, false)
  assert.equal(lease.invalidaEstado, false)
})

test('só auth_invalid pede uma renovação; nenhuma recusa manda reexecutar a ação', () => {
  for (const [code] of RECUSAS_RPC) {
    const decisao = normalizarFalhaRemota({ origem: 'rpc', code })
    assert.equal(decisao.renovarUmaVez, code === 'auth_invalid', code)
  }
})

test('recusa de papel mantém a sessão e não invalida estado', () => {
  const decisao = normalizarFalhaRemota({ origem: 'rpc', code: 'role_insufficient' })
  assert.deepEqual([decisao.acao, decisao.invalidaEstado, decisao.terminal], ['recusar_acao', false, false])
})

test('epoch antigo invalida estado sem terminar o stream', () => {
  const decisao = normalizarFalhaRemota({ origem: 'rpc', code: 'workspace_epoch_mismatch' })
  assert.deepEqual([decisao.acao, decisao.invalidaEstado, decisao.encerraStream], ['limpar_e_reabrir', true, false])
})

test('evento terminal fecha o stream, invalida estado e não aplica os seguintes', () => {
  for (const evento of ['workspace_revoked', 'role_changed']) {
    const decisao = normalizarFalhaRemota({ origem: 'evento', evento })
    assert.deepEqual(
      [decisao.motivo, decisao.terminal, decisao.invalidaEstado, decisao.encerraStream],
      [evento, true, true, true],
      evento,
    )
  }
})

test('hub_offline só existe como falha local de transporte', () => {
  assert.deepEqual(normalizarFalhaRemota({ origem: 'transporte' }), {
    motivo: 'hub_offline', acao: 'mostrar_indisponivel', renovarUmaVez: false, terminal: false, invalidaEstado: false, encerraStream: false,
  })
  assert.equal(normalizarFalhaRemota({ origem: 'rpc', code: 'hub_offline' }).motivo, 'desconhecido')
  assert.equal(normalizarFalhaRemota({ origem: 'canal', reject: { stableCode: 'hub_offline' } }).motivo, 'desconhecido')
  assert.equal(normalizarFalhaRemota({ origem: 'evento', evento: 'hub_offline' }).motivo, 'desconhecido')
})

test('código de fechamento e origem de rede não concedem nem mudam a decisão', () => {
  const gateway = normalizarFalhaRemota({ origem: 'canal', reject: { stableCode: 'device_unpaired', code: 4001 }, via: 'gateway' })
  const tailscale = normalizarFalhaRemota({ origem: 'canal', reject: { stableCode: 'device_unpaired', code: 4003 }, via: 'tailscale' })
  assert.deepEqual(gateway, tailscale)
  assert.equal(normalizarFalhaRemota({ origem: 'canal', reject: { code: 4001 }, via: 'tailscale' }).motivo, 'desconhecido')
})

test('entrada malformada, desconhecida ou herdada falha fechada', () => {
  const entradas: unknown[] = [null, undefined, 'auth_invalid', 42, {}, { origem: 'rpc' }, { origem: 'rpc', code: 'constructor' }, { origem: 'rpc', code: { toString: 1 } }, { origem: 'xpto', code: 'auth_invalid' }]
  for (const entrada of entradas) {
    const decisao = normalizarFalhaRemota(entrada)
    assert.deepEqual(
      [decisao.motivo, decisao.acao, decisao.renovarUmaVez, decisao.invalidaEstado, decisao.encerraStream],
      ['desconhecido', 'recusar', false, false, false],
      JSON.stringify(entrada),
    )
    assert.equal(decisao.terminal, true)
  }
})
