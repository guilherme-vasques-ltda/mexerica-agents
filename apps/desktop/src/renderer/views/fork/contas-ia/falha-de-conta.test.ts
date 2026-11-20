// Falha de conta/motor: o código do Hub + a lista de contas viram a frase e os botões, nos 6 idiomas, sem vazar segredo.
import assert from 'node:assert/strict'
import test from 'node:test'
import { IDIOMAS_CATALOGO } from '../../../lib/fork-i18n/registro.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import type { AccountUsage } from '../../../../shared/fork-engine-accounts.ts'
import { ehAvisoDePreparo, fraseDaFalha, lerReason, linhaDaConta, resumoUso, type ContaFalha } from './falha-de-conta.ts'
import { whenText } from './regras.ts'

const CODIGOS = ['account_unavailable', 'account_disconnected', 'account_not_found', 'account_incompatible', 'cli_unavailable', 'model_unavailable', 'model_option_unavailable', 'setup_in_progress', 'space_account_selection_required', 'engine_unavailable', 'routing_selection_unavailable', 'engine_choice_execution_mismatch', 'engine_not_ready', 'folder_unavailable', 'quota_near_limit', 'quota_exhausted', 'fallback_no_proof', 'setup_failed']
// Campos que o tipo da conta nunca tem; entram à força para provar que nada além de rótulo e estado chega à frase.
const SEGREDOS = { token: 'sk-ant-SEGREDO-123', email: 'dono@exemplo.com', apiKey: 'chave-super-secreta' }
const conta = (id: string, label: string, state: string, backend = 'claude'): ContaFalha => ({ id, label, state, backend, origin: 'managed', ...SEGREDOS })
const CONTAS = [conta('a1', 'Empresa', 'disconnected'), conta('a2', 'Pessoal', 'connected', 'codex'), conta('a3', 'Reserva', 'connected')]
const CONTAS_ON = [conta('a1', 'Empresa', 'connected'), conta('a2', 'Pessoal', 'connected', 'codex')]
const pt = tradutor('pt-BR')
const ctx = (contas: readonly ContaFalha[] | null, idioma: (typeof IDIOMAS_CATALOGO)[number] = 'pt-BR') => ({ t: tradutor(idioma), idioma, bot: 'Berga', contas })

test('a frase do dono: conta desconectada diz qual bot, qual conta e o que fazer', () => {
  const f = fraseDaFalha({ code: 'account_unavailable', accountId: 'a1', backend: 'claude' }, ctx(CONTAS))
  assert.equal(f.texto, 'O bot Berga usa a conta Claude Code “Empresa”, que não está conectada.')
  assert.equal(f.contaDoBot, '', 'a frase já diz a conta: sem repetir')
  const outra = fraseDaFalha({ code: 'cli_unavailable', accountId: 'a1' }, ctx(CONTAS))
  assert.match(outra.contaDoBot, /Claude Code .Empresa. · desconectada/)
  assert.match(f.contasConectadas, /Pessoal/)
  assert.match(f.contasConectadas, /Reserva/)
  assert.doesNotMatch(f.contasConectadas, /Empresa/)
  assert.deepEqual(f.botoes.map((b) => [b.acao, b.rotulo]), [['contas', 'Reconectar'], ['contas', 'Abrir Contas de IA']])
})

test('todo código da lista fechada, em todo idioma: frase, botão e nada cru', () => {
  for (const idioma of IDIOMAS_CATALOGO) {
    for (const code of CODIGOS) {
      for (const contas of [CONTAS, null, []] as const) {
        for (const accountId of ['a1', 'a2', 'zzz', undefined]) {
          const f = fraseDaFalha({ code, accountId, backend: 'codex' }, ctx(contas, idioma))
          const tudo = [f.texto, f.contaDoBot, f.contasConectadas, ...f.botoes.map((b) => b.rotulo)].join('\n')
          const rotulo = `${idioma}/${code}/${accountId}/${contas?.length}`
          assert.ok(f.texto.length > 10 && f.botoes.length >= 1, rotulo)
          assert.doesNotMatch(tudo, /[a-z]+_[a-z_]+/, `${rotulo}: código interno na tela`)
          assert.doesNotMatch(tudo, /undefined|\[object|\{[a-z]+\}/, `${rotulo}: placeholder sem preencher`)
          for (const segredo of Object.values(SEGREDOS)) assert.ok(!tudo.includes(segredo), `${rotulo}: vazou ${segredo}`)
        }
      }
    }
  }
})

test('falha fechada: sem lista ou sem a conta nela, não afirma estado e leva a Contas de IA', () => {
  const semLista = fraseDaFalha({ code: 'account_unavailable', accountId: 'a1' }, ctx(null))
  assert.match(semLista.texto, /não consegui confirmar/)
  assert.doesNotMatch(semLista.texto + semLista.contaDoBot, /conectada\b.*Empresa|Empresa/)
  assert.match(semLista.contasConectadas, /Não consegui confirmar quais contas/)
  assert.equal(semLista.botoes[0]!.acao, 'contas')
  const sumiu = fraseDaFalha({ code: 'account_unavailable', accountId: 'zzz' }, ctx(CONTAS))
  assert.match(sumiu.texto, /não existe mais/)
  assert.equal(sumiu.botoes[0]!.acao, 'bot-motor')
  assert.equal(sumiu.contaDoBot, '')
})

test('o botão certo por código: trocar conta, escolher pasta, conectar', () => {
  const acao = (code: string, accountId?: string) => fraseDaFalha({ code, accountId }, ctx(CONTAS)).botoes[0]!
  assert.deepEqual([acao('account_not_found', 'a1').acao, acao('account_incompatible', 'a1').acao, acao('engine_unavailable').acao], ['bot-motor', 'bot-motor', 'bot-motor'])
  assert.deepEqual(acao('folder_unavailable'), { acao: 'bot-pasta', rotulo: 'Escolher pasta' })
  assert.deepEqual(acao('engine_not_ready'), { acao: 'contas', rotulo: 'Conectar' })
  assert.equal(acao('setup_failed').rotulo, 'Abrir Contas de IA')
  // Motor fora dos gerenciados (omp descontinuado): "Conectar" não resolve — a ação é trocar o motor do bot.
  const omp = fraseDaFalha({ code: 'engine_not_ready', backend: 'omp' }, ctx(CONTAS))
  assert.equal(omp.texto, 'O motor de IA escolhido para o bot Berga não está disponível neste Hub.')
  assert.deepEqual(omp.botoes[0], { acao: 'bot-motor', rotulo: 'Trocar o motor do bot' })
  assert.equal(acao('engine_not_ready').acao, 'contas', 'backend ausente (Hub antigo) segue no fluxo de conectar')
})

test('sem conta no turno: diz que o bot usa o acesso do Hub; contas conectadas listadas ou "nenhuma"', () => {
  const f = fraseDaFalha({ code: 'engine_not_ready', backend: 'codex' }, ctx(CONTAS))
  assert.equal(f.texto, 'O Codex do Hub precisa de login ou instalação, e o bot Berga não tem uma conta própria.')
  assert.match(f.contaDoBot, /nenhuma/)
  assert.match(fraseDaFalha({ code: 'setup_failed' }, ctx([conta('x', 'Velha', 'disconnected')])).contasConectadas, /Nenhuma conta de IA está conectada/)
})

test('estado "conectada" com o Hub dizendo indisponível: não contradiz, diz que o Hub não conseguiu usar', () => {
  const f = fraseDaFalha({ code: 'account_unavailable', accountId: 'a3' }, ctx(CONTAS))
  assert.match(f.texto, /não conseguiu usá-la agora/)
})

test('lerReason: só código fechado e ids; descarta o resto (e-mail, token, texto cru)', () => {
  assert.deepEqual(lerReason({ code: 'account_unavailable', accountId: 'a1', backend: 'codex', email: 'x@y.z', token: 'abc' }), { code: 'account_unavailable', accountId: 'a1', backend: 'codex' })
  assert.equal(lerReason({ code: 'Conta desconectada: dono@exemplo.com' }), null)
  assert.equal(lerReason({ code: 'ok', accountId: 'tem espaço e @' })?.accountId, undefined)
  assert.equal(lerReason(undefined), null)
  assert.equal(lerReason('account_unavailable'), null)
})

test('Hub antigo: reconhece só o aviso fixo de preparo', () => {
  assert.ok(ehAvisoDePreparo('Não foi possível preparar o pedido após três tentativas. Confira o erro e reenvie o pedido.'))
  assert.ok(!ehAvisoDePreparo('Webhook bloqueado: controle de cadeia do Hub indisponível.'))
  assert.ok(!ehAvisoDePreparo(undefined))
})

test('linha do cabeçalho: conectada, desconectada, sem conta e "não consegui confirmar"', () => {
  const linha = (accountId: string | null, contas: readonly ContaFalha[] | null, falhou = false) => linhaDaConta({ t: pt, contas, accountId, backend: 'claude', falhou })
  assert.deepEqual(linha('a1', CONTAS), { texto: 'Conta: Empresa · Claude Code · desconectada', ok: false })
  assert.deepEqual(linha('a2', CONTAS), { texto: 'Conta: Pessoal · Codex · conectada', ok: true })
  assert.deepEqual(linha(null, CONTAS), { texto: 'Conta: login padrão deste computador', ok: true, dica: 'Este bot não tem uma conta escolhida: ele usa o login padrão deste computador, se houver um. Para escolher uma conta específica, vá em Contas de IA.' })
  assert.deepEqual(linhaDaConta({ t: pt, contas: CONTAS, accountId: null, backend: 'codex', open: true }), { texto: 'Conta: a do Codex conectada mais recente', ok: true, dica: 'Na Open, os bots respondem com a conta do Codex conectada mais recentemente. Para conectar ou trocar, vá em Contas de IA.' })
  assert.equal(linha('a1', null), null)
  assert.deepEqual(linha('a1', null, true), { texto: 'Conta: não consegui confirmar', ok: false })
  assert.equal(linha('zzz', CONTAS)!.ok, false)
  for (const idioma of IDIOMAS_CATALOGO) {
    const t = tradutor(idioma)
    const texto = linhaDaConta({ t, contas: CONTAS, accountId: 'a1', backend: 'claude' })!.texto
    assert.ok(texto.includes('Empresa'), idioma)
    for (const segredo of Object.values(SEGREDOS)) assert.ok(!texto.includes(segredo))
  }
})

test('árabe: o nome da conta vai isolado (FSI…PDI) dentro da frase', () => {
  const f = fraseDaFalha({ code: 'account_unavailable', accountId: 'a1' }, ctx(CONTAS, 'ar-AE'))
  assert.ok(f.texto.includes('⁨Empresa⁩'))
})

test('MEX-1294/F6-A: a linha da conta ganha o resumo de uso; sem leitura fica como estava', () => {
  const uso = (windows: AccountUsage['windows'], state: 'available' | 'unavailable' = 'available'): AccountUsage => ({
    engineAccountId: 'a1', spaceId: 'pessoal', state, source: 'cli', updatedAt: 1, windows,
  })
  const limpa = (r: { texto: string } | null) => r!.texto.replace(/[⁨⁩]/g, '')
  const agora = new Date(2026, 10, 17, 10, 0).getTime()
  const as = (h: number, m = 0, dias = 0) => new Date(2026, 10, 17 + dias, h, m).getTime()
  const completo = uso([
    { id: 'session', percent: 62, resetsAt: as(14, 30), resetsText: null, nearLimit: false },
    { id: 'week', percent: 40, resetsAt: as(12, 0, 3), resetsText: null, nearLimit: false },
  ])
  const resumo = resumoUso(completo, pt, 'pt-BR', agora)!
  assert.equal(resumo.perto, false)
  assert.equal(limpa(resumo), '62% da sessão, 40% da semana, volta 14:30')
  const linha = linhaDaConta({ t: pt, contas: CONTAS_ON, accountId: 'a1', backend: 'claude', usoResumo: resumo })!
  assert.ok(linha.texto.startsWith('Conta: Empresa · Claude Code · conectada · '), linha.texto)
  assert.ok(linha.texto.replace(/[⁨⁩]/g, '').endsWith('62% da sessão, 40% da semana, volta 14:30'))
  assert.equal(linha.perto, undefined)
  assert.equal(linha.base, 'Conta: Empresa · Claude Code · conectada')
  assert.equal(linha.uso!.replace(/[⁨⁩]/g, ''), '62% da sessão, 40% da semana, volta 14:30')
  // Só sessão, sem reset: metade da frase; sem leitura/indisponível: sem resumo nenhum.
  const soSessao = resumoUso(uso([{ id: 'session', percent: 89, resetsAt: null, resetsText: null }]), pt, 'pt-BR')!
  assert.equal(limpa(soSessao), '89% da sessão')
  assert.equal(resumoUso(uso([], 'unavailable'), pt, 'pt-BR'), null)
  assert.equal(resumoUso(uso([{ id: 'session', percent: 0, resetsAt: null, resetsText: null }], 'unavailable'), pt, 'pt-BR'), null)
  assert.equal(resumoUso(uso([]), pt, 'pt-BR'), null)
  assert.equal(resumoUso('erro', pt, 'pt-BR'), null)
  assert.equal(resumoUso(null, pt, 'pt-BR'), null)
  // O texto cru do provedor ("Sep 26 at 12pm (Asia/Taipei)") nunca vai para a linha.
  const cru = resumoUso(uso([{ id: 'session', percent: 30, resetsAt: null, resetsText: 'Sep 26 at 12pm (Asia/Taipei)' }]), pt, 'pt-BR')!
  assert.ok(!/Sep|Taipei|12pm/.test(cru.texto), cru.texto)
})

test('MEX-1294/F6-A: perto do limite (>= 90%) avisa na linha e "volta" é o da janela apertada', () => {
  const agora = new Date(2026, 10, 17, 10, 0).getTime()
  const as = (h: number, m = 0, dias = 0) => new Date(2026, 10, 17 + dias, h, m).getTime()
  const uso: AccountUsage = {
    engineAccountId: 'a1', state: 'available', source: 'cli', updatedAt: 1,
    windows: [
      { id: 'session', percent: 20, resetsAt: as(14, 30), nearLimit: false },
      { id: 'week', percent: 94, resetsAt: as(9, 0, 3), nearLimit: true },
    ],
  }
  const resumo = resumoUso(uso, pt, 'pt-BR', agora)!
  assert.equal(resumo.perto, true)
  const limpo = resumo.texto.replace(/[⁨⁩]/g, '')
  assert.ok(limpo.startsWith('Perto do limite: 20% da sessão, 94% da semana, volta '), limpo)
  assert.ok(limpo.endsWith('09:00'), `volta é a da semana: ${limpo}`)
  assert.ok(!limpo.includes('14:30'), limpo)
  const linha = linhaDaConta({ t: pt, contas: CONTAS_ON, accountId: 'a1', backend: 'claude', usoResumo: resumo })!
  assert.equal(linha.perto, true)
  assert.equal(linha.ok, true)
  // Conta caída: o uso não aparece e a linha não vira "perto".
  const caida = linhaDaConta({ t: pt, contas: CONTAS, accountId: 'a1', backend: 'claude', usoResumo: resumo })!
  assert.equal(caida.perto, undefined)
  assert.ok(!caida.texto.includes('Perto do limite'))
  // O limiar é do Hub: 90% sem `nearLimit` não vira aviso no app.
  const semCampo = resumoUso({ ...uso, windows: [{ id: 'session', percent: 95, resetsAt: null }] }, pt, 'pt-BR')!
  assert.equal(semCampo.perto, false)
})

test('MEX-1294/F6-A: os 6 idiomas escrevem o resumo, o aviso e a hora da renovação sem código interno', () => {
  const agora = new Date(2026, 10, 17, 10, 0).getTime()
  const uso: AccountUsage = {
    engineAccountId: 'a1', state: 'available', source: 'cli', updatedAt: 1,
    windows: [
      { id: 'session', percent: 92, resetsAt: new Date(2026, 10, 17, 14, 30).getTime(), nearLimit: true },
      { id: 'week', percent: 40, resetsAt: new Date(2026, 10, 20, 12, 0).getTime(), nearLimit: false },
    ],
  }
  const vistos = new Set<string>()
  for (const idioma of IDIOMAS_CATALOGO) {
    const t = tradutor(idioma)
    const r = resumoUso(uso, t, idioma, agora)!
    assert.ok(r.perto, idioma)
    assert.ok(r.texto.startsWith(t('contas.usoPerto')), `${idioma}: ${r.texto}`)
    assert.ok(!/[a-z]+_[a-z_]+|\{[a-z]+\}/.test(r.texto), `${idioma}: ${r.texto}`)
    vistos.add(r.texto)
  }
  assert.equal(vistos.size, IDIOMAS_CATALOGO.length, 'cada idioma tem o próprio texto')
  // A hora: hoje só o relógio, dentro de 6 dias o dia da semana, depois o dia e o mês (sempre com a hora).
  const base = new Date(2026, 10, 17, 10, 0).getTime()
  assert.equal(whenText(new Date(2026, 10, 17, 14, 30).getTime(), 'pt-BR', base), '14:30')
  assert.match(whenText(new Date(2026, 10, 20, 12, 0).getTime(), 'pt-BR', base), /^sex.* 12:00$/)
  assert.match(whenText(new Date(2026, 10, 20, 12, 0).getTime(), 'en', base), /^Fri 12:00 PM$/)
  assert.match(whenText(new Date(2026, 10, 30, 12, 0).getTime(), 'pt-BR', base), /^30 de nov.* 12:00$/)
  assert.match(whenText(new Date(2026, 10, 30, 12, 0).getTime(), 'es', base), /^30 nov.* 12:00$/)
})

test('F6-B: parada por limite sem outra conta livre dá o botão de uso extra; esgotada e sem prova só levam às contas', () => {
  const perto = fraseDaFalha({ code: 'quota_near_limit', accountId: 'a1', backend: 'claude' }, ctx(CONTAS_ON))
  assert.match(perto.texto, /Empresa.*perto do limite.*não há outra conta livre/)
  assert.deepEqual(perto.botoes.map((b) => [b.acao, b.rotulo]), [['uso-extra', 'Permitir uso extra até o limite voltar'], ['contas', 'Abrir Contas de IA']])
  const esgotada = fraseDaFalha({ code: 'quota_exhausted', accountId: 'a1' }, ctx(CONTAS_ON))
  assert.match(esgotada.texto, /acabou e não há outra conta livre/)
  assert.equal(esgotada.botoes[0]!.acao, 'contas')
  const semProva = fraseDaFalha({ code: 'fallback_no_proof', accountId: 'a1' }, ctx(CONTAS_ON))
  assert.match(semProva.texto, /sem prova de acesso\. Nada foi enviado/)
})
