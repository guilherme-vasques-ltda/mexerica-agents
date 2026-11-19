import assert from 'node:assert/strict'
import { test } from 'node:test'
import { calcularH, diferencas, type EntradaConsentimento } from './consentimento.ts'
import { CATALOGOS } from '../../renderer/lib/fork-i18n/index.ts'
import { CHAVE_BLOQUEIO, CHAVE_ESTADO, CHAVE_ESTADO_EVENTO, CHAVE_EVENTO_EXIBIVEL, CHAVE_INCOMPATIVEL, CHAVE_MENSAGEM } from './catalogo.ts'
import { validarRegistroTentativa } from './esquemas.ts'
import { acoesPermitidas, aplicar, ESTADOS, MAX_TENTATIVAS_SENHA, novaTentativa, podeGravarNoServidor, type Estado, type Evento, type Tentativa } from './maquina.ts'
import { avaliarSondagem, type ResultadoSondagem } from './sondagem.ts'
import { analisarSaida, quadroDado, quadroEvento } from './quadros.ts'
import type { CodigoEvento, EstadoEvento } from './esquemas.ts'

const ID = '123e4567-e89b-42d3-a456-426614174000'
const FP1 = 'SHA256:' + 'A'.repeat(43)
const FP2 = 'SHA256:' + 'B'.repeat(43)
const HASH = 'cd'.repeat(32)

const sondagem = (mudar: Partial<ResultadoSondagem> = {}): ResultadoSondagem => ({
  compativel: true, motivos: [], sudo: 'pede-senha', arquitetura: 'x86_64', so: { id: 'debian', versao: '13' }, uidAdmin: 1000, uidHub: null, contaHub: 'ausente', motor: 'ausente',
  hostInstalado: 'ausente', linger: 'ausente', portaOcupada: 'ausente', ...mudar,
})
const entrada = (mudar: Partial<EntradaConsentimento> = {}): EntradaConsentimento => ({
  chaves: [FP1], usuario: 'admin', uid: 1000, so: { id: 'debian', versao: '13' }, arquitetura: 'x86_64', pacoteDigest: HASH, versao: '0.1.0',
  estado: { instalado: false, usuarioMotor: false, linger: false, hostOutroComputador: false, classe812: 'limpo-comprovado' },
  mudancas: ['contahub', 'usuario', 'chaveapp'], ...mudar,
})

/** Aplica eventos e exige que cada um seja aceito. */
function seguir(t: Tentativa, ...eventos: Evento[]): Tentativa {
  for (const e of eventos) {
    const r = aplicar(t, e)
    assert.equal(r.recusa, null, `${t.estado} + ${e.t}`)
    t = r.tentativa
  }
  return t
}
const ate = {
  chaveConfirmada: (t: Tentativa) => seguir(t, { t: 'iniciar' }, { t: 'alcancou', chaves: [FP1] }, { t: 'comparacao', comparou: true }, { t: 'entrou' }),
}
const nova = (extra: { protegido?: boolean; chaveGravada?: string[] | null } = {}) => novaTentativa(ID, { protegido: extra.protegido ?? false, chaveGravada: extra.chaveGravada ?? null })
/** Caminho feliz até o Resumo, com `sudo` que pede senha. */
const noResumo = (s = sondagem(), e = entrada(), t = nova()) => {
  t = ate.chaveConfirmada(t)
  t = seguir(t, { t: 'sondagemA', resultado: s })
  if (s.sudo === 'pede-senha') t = seguir(t, { t: 'senhaEnviada' })
  return seguir(t, { t: 'sondagemRoot', entrada: e })
}

test('caminho feliz com senha: até Pronta, e só grava no servidor depois da aprovação do H', () => {
  let t = ate.chaveConfirmada(nova())
  assert.equal(t.estado, 'Verificando')
  t = seguir(t, { t: 'sondagemA', resultado: sondagem() })
  assert.equal(t.estado, 'PedindoSenha')
  t = seguir(t, { t: 'senhaEnviada' }, { t: 'sondagemRoot', entrada: entrada() })
  assert.equal(t.estado, 'Resumo')
  assert.equal(t.resumo?.h, calcularH(entrada()))
  assert.deepEqual(t.resumo?.acoes, ['instalar'])
  assert.equal(podeGravarNoServidor(t), false)
  t = seguir(t, { t: 'aprovar', h: t.resumo!.h, acao: 'instalar' })
  assert.equal(t.estado, 'PedindoSenha')
  assert.equal(podeGravarNoServidor(t), false, 'na tela de senha ainda nada é gravado')
  t = seguir(t, { t: 'senhaEnviada' })
  assert.equal(t.estado, 'Enviando')
  assert.equal(podeGravarNoServidor(t), true)
  t = seguir(t, { t: 'transferenciaCompleta' }, { t: 'autenticado' }, { t: 'instalado' }, { t: 'saudeOk' }, { t: 'conectou' })
  assert.equal(t.estado, 'Pronta')
})

test('V6: `sudo -n` já passou = nenhuma tela de senha, em nenhum momento', () => {
  const estados: Estado[] = []
  let t = ate.chaveConfirmada(nova())
  const passo = (e: Evento) => { t = seguir(t, e); estados.push(t.estado) }
  passo({ t: 'sondagemA', resultado: sondagem({ sudo: 'sem-senha' }) })
  passo({ t: 'sondagemRoot', entrada: entrada() })
  passo({ t: 'aprovar', h: t.resumo!.h, acao: 'instalar' })
  assert.deepEqual(estados, ['SondandoRoot', 'Resumo', 'Enviando'])
  assert.ok(!estados.includes('PedindoSenha'))
})

test('sudo que falha por outro motivo (indisponível) ou servidor incompatível: Incompativel, nada alterado', () => {
  let t = seguir(ate.chaveConfirmada(nova()), { t: 'sondagemA', resultado: sondagem({ sudo: 'indisponivel', compativel: false, motivos: ['sudotty'] }) })
  assert.equal(t.estado, 'Incompativel')
  assert.deepEqual(t.motivosIncompativel, ['sudotty'])
  t = seguir(ate.chaveConfirmada(nova()), { t: 'sondagemA', resultado: sondagem({ compativel: false, motivos: ['so'] }) })
  assert.equal(t.estado, 'Incompativel')
  assert.equal(podeGravarNoServidor(t), false)
})

test('D3: sem comparação = Bloqueada; chave diferente da gravada = Bloqueada, nunca sobrescreve', () => {
  let t = seguir(nova(), { t: 'iniciar' }, { t: 'alcancou', chaves: [FP1] })
  assert.equal(t.estado, 'ConfirmandoChave')
  t = seguir(t, { t: 'comparacao', comparou: false })
  assert.equal(t.estado, 'Bloqueada')
  assert.equal(t.motivoBloqueio, 'sem-comparacao')
  assert.equal(aplicar(t, { t: 'comparacao', comparou: true }).recusa, 'fora-de-hora', 'Bloqueada é final')
  t = seguir(nova({ chaveGravada: [FP1] }), { t: 'iniciar' }, { t: 'alcancou', chaves: [FP2] })
  assert.equal(t.estado, 'Bloqueada')
  assert.equal(t.motivoBloqueio, 'chave-mudou')
  assert.deepEqual(t.chavesAceitas, [])
  // Igual à gravada: o dono já a confirmou antes, segue sem repetir a pergunta.
  t = seguir(nova({ chaveGravada: [FP1] }), { t: 'iniciar' }, { t: 'alcancou', chaves: [FP1] })
  assert.equal(t.estado, 'Entrando')
  assert.equal(aplicar(seguir(nova(), { t: 'iniciar' }), { t: 'alcancou', chaves: ['SHA256:curta'] }).recusa, 'dado-invalido')
})

test('aprovação vale só para o H do Resumo; outro H ou ação não oferecida é recusado', () => {
  const t = noResumo()
  assert.equal(aplicar(t, { t: 'aprovar', h: HASH, acao: 'instalar' }).recusa, 'h-diferente')
  assert.equal(aplicar(t, { t: 'aprovar', h: t.resumo!.h, acao: 'desinstalar' }).recusa, 'acao-nao-permitida')
  assert.equal(aplicar(t, { t: 'aprovar', h: t.resumo!.h, acao: 'conectar' }).recusa, 'acao-nao-permitida')
  assert.equal(aplicar(t, { t: 'enviado' as never }).recusa, 'fora-de-hora')
})

test('H muda com qualquer campo, inclusive a sondagem privilegiada, e não confunde fronteiras de campos', () => {
  const base = calcularH(entrada())
  const variantes: Partial<EntradaConsentimento>[] = [
    { chaves: [FP2] }, { chaves: [FP1, FP2] }, { usuario: 'admin2' }, { uid: 1001 }, { arquitetura: 'aarch64' }, { so: { id: 'ubuntu', versao: '24.04' } }, { so: { id: 'debian', versao: '12' } }, { pacoteDigest: 'ef'.repeat(32) }, { versao: '0.1.1' },
    { mudancas: ['usuario', 'contahub', 'chaveapp'] }, { mudancas: ['contahub'] },
    { estado: { ...entrada().estado, classe812: 'concluido-integro' } }, { estado: { ...entrada().estado, linger: null } },
    { estado: { ...entrada().estado, linger: true } }, { estado: { ...entrada().estado, hostOutroComputador: true } },
    { estado: { ...entrada().estado, instalado: true } }, { estado: { ...entrada().estado, usuarioMotor: true } },
  ]
  const vistos = new Set([base])
  for (const v of variantes) {
    const h = calcularH(entrada(v))
    assert.ok(!vistos.has(h), JSON.stringify(v))
    vistos.add(h)
  }
  assert.notEqual(calcularH(entrada({ usuario: 'a', mudancas: ['bc'] })), calcularH(entrada({ usuario: 'ab', mudancas: ['c'] })))
  assert.equal(calcularH(entrada({ chaves: [FP2, FP1] })), calcularH(entrada({ chaves: [FP1, FP2] })), 'a ordem das impressões digitais não importa')
  assert.deepEqual(diferencas(entrada(), entrada({ versao: '0.1.1', uid: 5 })), ['uid', 'pacote'])
  assert.deepEqual(diferencas(entrada(), entrada()), [])
})

const ateAlcancado = (t: Tentativa) => seguir(t, { t: 'alcancou', chaves: [FP1] }, { t: 'comparacao', comparou: true }, { t: 'entrou' })

test('efeitos pendentes acompanham a tentativa inteira: Enviando→cancelar→retomar→Alcancando→cancelar nunca vira Cancelada', () => {
  let t = seguir(aprovadoEm(noResumo()), { t: 'senhaEnviada' })
  assert.equal(t.efeitosPendentes, true)
  t = seguir(t, { t: 'cancelar' })
  assert.equal(t.estado, 'Interrompida')
  t = seguir(t, { t: 'retomar' })
  assert.equal(t.estado, 'Alcancando')
  assert.equal(t.efeitosPendentes, true, 'a retomada não zera o indicador')
  t = seguir(t, { t: 'cancelar' })
  assert.equal(t.estado, 'Interrompida', 'nunca "Cancelada/nada alterado"')
  // Segunda volta completa até o Resumo da retomada: cancelar lá também é interrupção.
  t = seguir(t, { t: 'retomar' })
  t = ateAlcancado(t)
  t = seguir(t, { t: 'sondagemA', resultado: sondagem({ sudo: 'sem-senha' }) }, { t: 'sondagemRoot', entrada: entrada() })
  assert.equal(t.estado, 'Resumo')
  assert.equal(seguir(t, { t: 'cancelar' }).estado, 'Interrompida')
})

test('efeitos pendentes: SenhaErrada→cancelar→Resumo→cancelar não diz "nada foi alterado"', () => {
  let t = seguir(aprovadoEm(noResumo()), { t: 'senhaEnviada' }, { t: 'transferenciaCompleta' }, { t: 'senhaRecusada' })
  assert.equal(t.estado, 'SenhaErrada')
  t = seguir(t, { t: 'cancelar' })
  assert.notEqual(t.estado, 'Resumo', 'já houve gravação')
  assert.equal(t.estado, 'Interrompida')
  assert.equal(t.efeitosPendentes, true)
  // Falha antes de gravar, limpeza comprovada: o indicador some (Limpa) e o resto volta a ser Cancelada.
  let f = seguir(aprovadoEm(noResumo()), { t: 'senhaEnviada' }, { t: 'transferenciaCompleta' }, { t: 'pacoteRecusado' })
  assert.equal(f.efeitosPendentes, true)
  assert.equal(seguir(f, { t: 'limpeza', completa: false }).efeitosPendentes, true, 'limpeza parcial não prova nada')
  f = seguir(f, { t: 'limpeza', completa: true })
  assert.equal(f.estado, 'Limpa')
  assert.equal(f.efeitosPendentes, false)
  // Só a senha da instalação, sem gravar nada: cancelar volta ao Resumo e depois a Cancelada.
  let p = seguir(noResumo(), { t: 'aprovar', h: calcularH(entrada()), acao: 'instalar' }, { t: 'cancelar' })
  assert.equal(p.estado, 'Resumo')
  assert.equal(seguir(p, { t: 'cancelar' }).estado, 'Cancelada')
})

test('retomada da mesma tentativa preserva os contadores de senha: 3 envios, queda, retomar, nova sondagem = sem tentativas', () => {
  let t = noResumo()
  // Três senhas da instalação entregues e recusadas (a 4ª não existe).
  for (let i = 0; i < MAX_TENTATIVAS_SENHA; i++) {
    t = seguir(t.estado === 'Resumo' ? seguir(t, { t: 'aprovar', h: t.resumo!.h, acao: 'instalar' }) : t, { t: 'senhaEnviada' }, { t: 'transferenciaCompleta' }, { t: 'senhaRecusada' })
    if (i < MAX_TENTATIVAS_SENHA - 1) t = seguir(t, { t: 'tentarDeNovo' })
  }
  assert.equal(t.senhaEnvios.instalacao, MAX_TENTATIVAS_SENHA)
  t = seguir(t, { t: 'queda' }, { t: 'retomar' })
  assert.deepEqual(t.senhaEnvios, { sondagem: 1, instalacao: MAX_TENTATIVAS_SENHA })
  assert.deepEqual(t.senhaEnviada, { sondagem: false, instalacao: false }, 'a senha em si nunca é reaproveitada')
  t = ateAlcancado(t)
  t = seguir(t, { t: 'sondagemA', resultado: sondagem() }, { t: 'senhaEnviada' }, { t: 'sondagemRoot', entrada: entrada() })
  assert.equal(t.estado, 'Resumo')
  assert.equal(aplicar(t, { t: 'aprovar', h: t.resumo!.h, acao: 'instalar' }).recusa, 'sem-tentativas')
})

test('desinstalar escolhido no Resumo persiste o objetivo: queda e cancelamento retomam como desinstalação', () => {
  const instalado = () => noResumo(sondagem({ sudo: 'sem-senha', hostInstalado: 'presente', contaHub: 'presente' }))
  const h = calcularH(entrada())
  assert.equal(instalado().objetivo, 'instalar')
  const desinstalando = seguir(instalado(), { t: 'aprovar', h, acao: 'desinstalar' })
  assert.equal(desinstalando.estado, 'Desinstalando')
  assert.equal(desinstalando.objetivo, 'desinstalar')
  const aposQueda = seguir(desinstalando, { t: 'queda' })
  assert.equal(aposQueda.estado, 'Interrompida')
  assert.equal(seguir(aposQueda, { t: 'retomar' }).objetivo, 'desinstalar')
  const retomadaCancelada = seguir(aposQueda, { t: 'retomar' }, { t: 'cancelar' })
  assert.equal(retomadaCancelada.estado, 'DesinstalacaoParcial')
  assert.equal(seguir(retomadaCancelada, { t: 'retomar' }).objetivo, 'desinstalar')
  // Conectar não muda o objetivo.
  assert.equal(seguir(instalado(), { t: 'aprovar', h, acao: 'conectar' }).objetivo, 'instalar')
})

test('toda retomada volta ao Resumo e pede nova aprovação, com e sem mudança no servidor', () => {
  for (const mudou of [false, true]) {
    let t = noResumo()
    t = seguir(t, { t: 'aprovar', h: t.resumo!.h, acao: 'instalar' }, { t: 'senhaEnviada' }, { t: 'queda' })
    assert.equal(t.estado, 'Interrompida')
    assert.equal(podeGravarNoServidor(t), false)
    const hAntes = calcularH(entrada())
    t = seguir(t, { t: 'retomar' })
    assert.equal(t.retomada, true)
    assert.equal(t.aprovadoH, null, 'a aprovação anterior nunca vale')
    t = seguir(t, { t: 'alcancou', chaves: [FP1] }, { t: 'comparacao', comparou: true }, { t: 'entrou' }, { t: 'sondagemA', resultado: sondagem() })
    assert.equal(t.estado, 'PedindoSenha', 'a senha é pedida de novo')
    const nova2 = mudou ? entrada({ pacoteDigest: 'ef'.repeat(32) }) : entrada()
    t = seguir(t, { t: 'senhaEnviada' }, { t: 'sondagemRoot', entrada: nova2 })
    assert.equal(t.estado, 'Resumo')
    assert.equal(t.aprovadoH, null)
    assert.equal(podeGravarNoServidor(t), false, 'nada é gravado antes da nova aprovação')
    assert.deepEqual(t.resumo!.diferencas, mudou ? ['pacote'] : [])
    assert.equal(t.resumo!.h === hAntes, !mudou)
    assert.equal(aplicar(t, { t: 'transferenciaCompleta' }).recusa, 'fora-de-hora')
    t = seguir(t, { t: 'aprovar', h: t.resumo!.h, acao: 'instalar' })
    assert.equal(t.aprovadoH, t.resumo!.h)
  }
})

test('retomada: o H da tentativa anterior não aprova o novo Resumo', () => {
  let t = noResumo()
  const hAntigo = t.resumo!.h
  t = seguir(t, { t: 'queda' }, { t: 'retomar' }, { t: 'alcancou', chaves: [FP1] }, { t: 'comparacao', comparou: true }, { t: 'entrou' }, { t: 'sondagemA', resultado: sondagem({ sudo: 'sem-senha' }) },
    { t: 'sondagemRoot', entrada: entrada({ versao: '0.2.0' }) })
  assert.equal(aplicar(t, { t: 'aprovar', h: hAntigo, acao: 'instalar' }).recusa, 'h-diferente')
})

test('estado do #812: ilegível = Incompativel; incompleto = IntervencaoManual; só limpo ou íntegro chegam ao Resumo', () => {
  const com = (classe: 'ilegivel' | 'incompleto' | 'limpo-comprovado' | 'concluido-integro') => {
    let t = seguir(ate.chaveConfirmada(nova()), { t: 'sondagemA', resultado: sondagem({ sudo: 'sem-senha' }) })
    t = seguir(t, { t: 'sondagemRoot', entrada: entrada({ estado: { ...entrada().estado, classe812: classe } }) })
    return t
  }
  assert.equal(com('ilegivel').estado, 'Incompativel')
  assert.deepEqual(com('ilegivel').motivosIncompativel, ['estado-ilegivel'])
  assert.equal(com('incompleto').estado, 'IntervencaoManual')
  assert.equal(com('limpo-comprovado').estado, 'Resumo')
  assert.equal(com('concluido-integro').estado, 'Resumo')
  const t = seguir(ate.chaveConfirmada(nova()), { t: 'sondagemA', resultado: sondagem({ sudo: 'sem-senha' }) }, { t: 'sondagemRootIlegivel' })
  assert.equal(t.estado, 'Incompativel')
})

test('com senha exigida, a sondagem privilegiada só vale depois de a senha ser enviada', () => {
  const t = seguir(ate.chaveConfirmada(nova()), { t: 'sondagemA', resultado: sondagem() })
  assert.equal(aplicar(t, { t: 'sondagemRoot', entrada: entrada() }).recusa, 'fora-de-hora')
})

test('senha errada: no máximo 3 tentativas por fase, contadas à parte; esgotada, só cancelar', () => {
  let t = seguir(ate.chaveConfirmada(nova()), { t: 'sondagemA', resultado: sondagem() })
  for (let i = 1; i <= MAX_TENTATIVAS_SENHA; i++) {
    t = seguir(t, { t: 'senhaEnviada' }, { t: 'senhaRecusada' })
    assert.equal(t.estado, 'SenhaErrada')
    assert.equal(t.senhaErros.sondagem, i)
    if (i < MAX_TENTATIVAS_SENHA) t = seguir(t, { t: 'tentarDeNovo' })
  }
  assert.equal(aplicar(t, { t: 'tentarDeNovo' }).recusa, 'sem-tentativas')
  assert.equal(seguir(t, { t: 'cancelar' }).estado, 'Cancelada')
  assert.deepEqual(t.senhaErros, { sondagem: 3, instalacao: 0 }, 'a fase da instalação conta à parte')
})

test('cancelar na senha da instalação volta ao Resumo, sem aprovação e sem gravar nada', () => {
  let t = noResumo()
  t = seguir(t, { t: 'aprovar', h: t.resumo!.h, acao: 'instalar' })
  assert.equal(t.estado, 'PedindoSenha')
  t = seguir(t, { t: 'cancelar' })
  assert.equal(t.estado, 'Resumo')
  assert.equal(t.aprovadoH, null)
  assert.equal(podeGravarNoServidor(t), false)
})

test('cancelar vale em todos os estados ativos, menos na fase B', () => {
  let t = seguir(noResumo(), { t: 'aprovar', h: calcularH(entrada()), acao: 'instalar' }, { t: 'senhaEnviada' }, { t: 'transferenciaCompleta' })
  assert.equal(t.estado, 'ConferindoRoot')
  assert.equal(aplicar(t, { t: 'cancelar' }).recusa, 'aguarde-fase-b')
  t = seguir(t, { t: 'autenticado' })
  assert.equal(aplicar(t, { t: 'cancelar' }).recusa, 'aguarde-fase-b')
  assert.equal(seguir(nova(), { t: 'cancelar' }).estado, 'Cancelada')
  assert.equal(seguir(seguir(nova(), { t: 'iniciar' }), { t: 'cancelar' }).estado, 'Cancelada')
  assert.equal(seguir(noResumo(), { t: 'cancelar' }).estado, 'Cancelada')
})

const aprovadoEm = (t: Tentativa) => seguir(t, { t: 'aprovar', h: calcularH(entrada()), acao: 'instalar' })

test('cancelar depois de efeitos possíveis é interrupção (parcial, verificável), nunca "Cancelada/nada alterado"', () => {
  const emEnviando = () => seguir(aprovadoEm(noResumo()), { t: 'senhaEnviada' })
  assert.equal(emEnviando().estado, 'Enviando')
  const enviando = seguir(emEnviando(), { t: 'cancelar' })
  assert.equal(enviando.estado, 'Interrompida')
  assert.equal(podeGravarNoServidor(enviando), false)
  const retomada = seguir(enviando, { t: 'retomar' })
  assert.equal(retomada.estado, 'Alcancando')
  assert.equal(retomada.aprovadoH, null, 'retomar pede nova aprovação')
  for (const ate of [['autenticado', 'instalado'], ['autenticado', 'instalado', 'saudeOk']]) {
    let t = seguir(emEnviando(), { t: 'transferenciaCompleta' })
    for (const nome of ate) t = seguir(t, { t: nome } as Evento)
    assert.ok(t.estado === 'Saude' || t.estado === 'Conectando', t.estado)
    assert.equal(seguir(t, { t: 'cancelar' }).estado, 'Interrompida', t.estado)
  }
  // Desinstalação em curso: cancelar deixa resultado parcial que a retomada continua.
  let d = seguir(noResumo(sondagem({ sudo: 'sem-senha', hostInstalado: 'presente', contaHub: 'presente' })), { t: 'aprovar', h: calcularH(entrada()), acao: 'desinstalar' })
  assert.equal(d.estado, 'Desinstalando')
  d = seguir(d, { t: 'cancelar' })
  assert.equal(d.estado, 'DesinstalacaoParcial')
  assert.equal(seguir(d, { t: 'retomar' }).objetivo, 'desinstalar')
  // Antes de qualquer efeito, cancelar continua "Cancelada".
  assert.equal(seguir(noResumo(), { t: 'cancelar' }).estado, 'Cancelada')
})

test('limite de 3 senhas vale também na entrada: cancelar→Resumo→aprovar→enviar não zera a conta', () => {
  let t = noResumo()
  for (let i = 1; i <= MAX_TENTATIVAS_SENHA; i++) {
    t = seguir(t, { t: 'aprovar', h: t.resumo!.h, acao: 'instalar' })
    assert.equal(t.estado, 'PedindoSenha')
    if (i < MAX_TENTATIVAS_SENHA) t = seguir(t, { t: 'cancelar' })
  }
  t = seguir(t, { t: 'senhaEnviada' })
  assert.equal(t.senhaEnvios.instalacao, 1)
  // Volta ao Resumo por queda de rede e nova aprovação na mesma tentativa: o contador continua.
  let u = noResumo()
  u = { ...u, senhaEnvios: { sondagem: 1, instalacao: MAX_TENTATIVAS_SENHA } }
  assert.equal(aplicar(u, { t: 'aprovar', h: u.resumo!.h, acao: 'instalar' }).recusa, 'sem-tentativas')
  const pedindo = { ...seguir(noResumo(), { t: 'aprovar', h: calcularH(entrada()), acao: 'instalar' }), senhaEnvios: { sondagem: 1, instalacao: MAX_TENTATIVAS_SENHA } }
  assert.equal(aplicar(pedindo, { t: 'senhaEnviada' }).recusa, 'sem-tentativas')
  // Fluxo real: a senha recusada já passou pela transferência; cancelar é interrupção, e a retomada mantém a conta.
  let r = noResumo()
  r = seguir(r, { t: 'aprovar', h: r.resumo!.h, acao: 'instalar' }, { t: 'senhaEnviada' }, { t: 'transferenciaCompleta' }, { t: 'senhaRecusada' }, { t: 'cancelar' })
  assert.equal(r.estado, 'Interrompida')
  assert.equal(seguir(r, { t: 'retomar' }).senhaEnvios.instalacao, 1)
})

test('senha recusada na desinstalação: transição, contagem e nova tentativa', () => {
  let t = seguir(noResumo(sondagem({ hostInstalado: 'presente', contaHub: 'presente' })), { t: 'aprovar', h: calcularH(entrada()), acao: 'desinstalar' }, { t: 'senhaEnviada' })
  assert.equal(t.estado, 'Desinstalando')
  for (let i = 1; i <= MAX_TENTATIVAS_SENHA; i++) {
    t = seguir(t, { t: 'senhaRecusada' })
    assert.equal(t.estado, 'SenhaErrada')
    assert.equal(t.senhaErros.instalacao, i)
    if (i < MAX_TENTATIVAS_SENHA) t = seguir(t, { t: 'tentarDeNovo' }, { t: 'senhaEnviada' })
  }
  assert.equal(aplicar(t, { t: 'tentarDeNovo' }).recusa, 'sem-tentativas')
  assert.equal(podeGravarNoServidor(t), false)
})

test('falhas da fase B: antes do #812 = Falhou e limpeza; depois = IntervencaoManual, sem retomada automática', () => {
  const emConferindo = () => seguir(noResumo(), { t: 'aprovar', h: calcularH(entrada()), acao: 'instalar' }, { t: 'senhaEnviada' }, { t: 'transferenciaCompleta' })
  assert.equal(seguir(emConferindo(), { t: 'pacoteRecusado' }).estado, 'PacoteRuim')
  assert.equal(seguir(emConferindo(), { t: 'faseBFalhou', tocouNo812: false }).estado, 'Falhou')
  assert.equal(seguir(emConferindo(), { t: 'autenticado' }, { t: 'faseBFalhou', tocouNo812: true }).estado, 'IntervencaoManual')
  assert.equal(seguir(emConferindo(), { t: 'faseBFalhou', tocouNo812: false }, { t: 'limpeza', completa: true }).estado, 'Limpa')
  assert.equal(seguir(emConferindo(), { t: 'faseBFalhou', tocouNo812: false }, { t: 'limpeza', completa: false }).estado, 'Parcial')
  const manual = seguir(emConferindo(), { t: 'autenticado' }, { t: 'faseBFalhou', tocouNo812: true })
  assert.equal(aplicar(manual, { t: 'retomar' }).recusa, 'fora-de-hora')
  assert.equal(aplicar(manual, { t: 'aprovar', h: HASH, acao: 'instalar' }).recusa, 'fora-de-hora')
  assert.equal(seguir(emConferindo(), { t: 'queda' }).estado, 'Interrompida')
})

test('D6: servidor protegido nunca recebe instalar nem desinstalar; Host existente só conecta ou desinstala', () => {
  const tp = nova({ protegido: true })
  assert.deepEqual(acoesPermitidas(tp, sondagem()), [])
  assert.deepEqual(acoesPermitidas(tp, sondagem({ hostInstalado: 'presente' })), ['conectar'])
  const t = noResumo(sondagem(), entrada(), tp)
  assert.deepEqual(t.resumo!.acoes, [])
  assert.equal(aplicar(t, { t: 'aprovar', h: t.resumo!.h, acao: 'instalar' }).recusa, 'acao-nao-permitida')
  const existente = sondagem({ hostInstalado: 'presente' })
  assert.deepEqual(acoesPermitidas(nova(), existente), ['conectar', 'desinstalar'], 'reinstalar fica recusado')
  const aprovar = (protegido: boolean) => aplicar(noResumo(sondagem({ sudo: 'sem-senha' }), entrada(), nova({ protegido })), { t: 'aprovar', h: calcularH(entrada()), acao: 'instalar' })
  assert.equal(aprovar(false).tentativa.estado, 'Enviando')
  assert.equal(aprovar(true).recusa, 'acao-nao-permitida')
  assert.equal(aprovar(true).tentativa.estado, 'Resumo')
})

test('conectar a um Host existente não grava nada no servidor', () => {
  let t = noResumo(sondagem({ sudo: 'sem-senha', hostInstalado: 'presente' }))
  t = seguir(t, { t: 'aprovar', h: t.resumo!.h, acao: 'conectar' })
  assert.equal(t.estado, 'Conectando')
  assert.equal(podeGravarNoServidor(t), false)
})

test('desinstalar: de Pronta passa de novo pelo Resumo e nunca em servidor protegido; parcial retoma pelo Resumo', () => {
  let t = seguir(noResumo(sondagem({ sudo: 'sem-senha' })), { t: 'aprovar', h: calcularH(entrada()), acao: 'instalar' }, { t: 'transferenciaCompleta' }, { t: 'autenticado' }, { t: 'instalado' }, { t: 'saudeOk' }, { t: 'conectou' })
  assert.equal(t.estado, 'Pronta')
  const protegida = { ...t, protegido: true }
  assert.equal(aplicar(protegida, { t: 'desinstalar' }).recusa, 'acao-nao-permitida')
  t = seguir(t, { t: 'desinstalar' }, { t: 'alcancou', chaves: [FP1] }, { t: 'comparacao', comparou: true }, { t: 'entrou' })
  const s = sondagem({ sudo: 'sem-senha', hostInstalado: 'presente', contaHub: 'presente', uidHub: 998 })
  t = seguir(t, { t: 'sondagemA', resultado: s }, { t: 'sondagemRoot', entrada: entrada({ estado: { ...entrada().estado, instalado: true, classe812: 'concluido-integro' } }) })
  assert.deepEqual(t.resumo!.acoes, ['desinstalar'])
  assert.equal(podeGravarNoServidor(t), false)
  t = seguir(t, { t: 'aprovar', h: t.resumo!.h, acao: 'desinstalar' })
  assert.equal(t.estado, 'Desinstalando')
  assert.equal(podeGravarNoServidor(t), true)
  const parcial = seguir(t, { t: 'desinstalado', completo: false })
  assert.equal(parcial.estado, 'DesinstalacaoParcial')
  assert.equal(seguir(parcial, { t: 'retomar' }).estado, 'Alcancando')
  assert.equal(seguir(parcial, { t: 'retomar' }).aprovadoH, null)
  assert.equal(seguir(t, { t: 'desinstalado', completo: true }).estado, 'Desinstalada')
})

test('propriedade: em qualquer sequência de eventos, só se grava no servidor com aprovação do H atual', () => {
  const eventos: Evento[] = [
    { t: 'iniciar' }, { t: 'alcancou', chaves: [FP1] }, { t: 'alcancou', chaves: [FP2] }, { t: 'comparacao', comparou: true }, { t: 'comparacao', comparou: false }, { t: 'entrou' },
    { t: 'sondagemA', resultado: sondagem() }, { t: 'sondagemA', resultado: sondagem({ sudo: 'sem-senha' }) }, { t: 'sondagemA', resultado: sondagem({ compativel: false, motivos: ['so'] }) },
    { t: 'senhaEnviada' }, { t: 'senhaRecusada' }, { t: 'tentarDeNovo' }, { t: 'sondagemRoot', entrada: entrada() }, { t: 'sondagemRoot', entrada: entrada({ versao: '0.3.0' }) },
    { t: 'sondagemRootIlegivel' }, { t: 'sondagemRoot', entrada: entrada({ estado: { ...entrada().estado, classe812: 'incompleto' } }) }, { t: 'aprovar', h: calcularH(entrada()), acao: 'instalar' }, { t: 'aprovar', h: calcularH(entrada({ versao: '0.3.0' })), acao: 'instalar' },
    { t: 'aprovar', h: HASH, acao: 'instalar' }, { t: 'aprovar', h: calcularH(entrada()), acao: 'conectar' }, { t: 'transferenciaCompleta' }, { t: 'pacoteRecusado' }, { t: 'autenticado' },
    { t: 'instalado' }, { t: 'faseBFalhou', tocouNo812: true }, { t: 'faseBFalhou', tocouNo812: false }, { t: 'limpeza', completa: true }, { t: 'saudeOk' }, { t: 'saudeFalhou' },
    { t: 'conectou' }, { t: 'desinstalar' }, { t: 'desinstalado', completo: true }, { t: 'queda' }, { t: 'retomar' }, { t: 'cancelar' },
  ]
  let semente = 20261115
  const rnd = () => { semente = (semente * 1103515245 + 12345) & 0x7fffffff; return semente }
  const alcancados = new Set<Estado>()
  for (let corrida = 0; corrida < 600; corrida++) {
    let t = nova({ protegido: corrida % 7 === 0, chaveGravada: corrida % 5 === 0 ? [FP1] : null })
    for (let passo = 0; passo < 60; passo++) {
      const antes = t
      // 70% dos passos escolhem entre os eventos que fazem sentido no estado atual; o resto é aleatório (fora de hora).
      const interrupcao = rnd() % 8 === 0
      const guiados = eventos.filter((e) => aplicar(t, e).recusa === null && (interrupcao || (e.t !== 'cancelar' && e.t !== 'queda')))
      const r = aplicar(t, rnd() % 10 < 7 && guiados.length > 0 ? guiados[rnd() % guiados.length]! : eventos[rnd() % eventos.length]!)
      t = r.tentativa
      alcancados.add(t.estado)
      if (r.recusa !== null) assert.equal(t, antes, 'recusa não muda nada')
      if (['Enviando', 'ConferindoRoot', 'Instalando', 'Desinstalando'].includes(t.estado)) {
        assert.equal(podeGravarNoServidor(t), true, `${t.estado} sem aprovação`)
        assert.equal(t.aprovadoH, t.resumo?.h)
        assert.ok(t.acaoAprovada === 'instalar' || t.acaoAprovada === 'desinstalar')
        assert.ok(!t.protegido, 'servidor protegido nunca chega a gravar')
        assert.ok(t.chavesAceitas.length > 0, 'só grava depois de a impressão digital ser aceita')
      }
      if (t.estado === 'Resumo' || t.estado === 'Interrompida') assert.equal(t.aprovadoH, null)
      if (t.retomada && t.estado === 'Resumo') assert.equal(t.aprovadoH, null, 'toda retomada pede nova aprovação')
    }
  }
  for (const e of ['Resumo', 'Enviando', 'Pronta', 'Bloqueada', 'Incompativel', 'Interrompida', 'IntervencaoManual', 'Cancelada']) assert.ok(alcancados.has(e as Estado), `o teste alcançou ${e}`)
})

test('Incompativel e Bloqueada não gravam e só aceitam cancelar ou nada', () => {
  const bloqueada = seguir(nova(), { t: 'iniciar' }, { t: 'alcancou', chaves: [FP1] }, { t: 'comparacao', comparou: false })
  for (const e of [{ t: 'entrou' }, { t: 'aprovar', h: HASH, acao: 'instalar' }, { t: 'retomar' }, { t: 'queda' }] as Evento[]) assert.equal(aplicar(bloqueada, e).recusa, 'fora-de-hora', e.t)
})

test('novaTentativa e registro: id precisa ser UUID hexadecimal; registro só com campos conhecidos e sem senha', () => {
  assert.throws(() => novaTentativa('x; rm -rf /', { protegido: false, chaveGravada: null }), RangeError)
  const bom = { id: ID, estado: 'Resumo', lingerJaAtivo: false, hash: HASH }
  assert.deepEqual(validarRegistroTentativa(bom, ESTADOS), bom)
  for (const ruim of [{ ...bom, senha: 'x' }, { ...bom, id: 'abc' }, { ...bom, estado: 'Qualquer' }, { ...bom, hash: 'zz' }, { ...bom, lingerJaAtivo: 'sim' }, null, [], 'texto']) {
    assert.equal(validarRegistroTentativa(ruim, ESTADOS), null, JSON.stringify(ruim))
  }
})

test('ponta a ponta: saída de quadros da VPS → sondagem → máquina (compatível e incompatível)', () => {
  const eventos = (ok: Partial<Record<CodigoEvento, EstadoEvento>>, uid = '1000 -') => {
    const padrao: Partial<Record<CodigoEvento, EstadoEvento>> = {
      so: 'ok', arquitetura: 'ok', systemd: 'ok', espaco: 'ok', sudo: 'ok', sudotty: 'ok', sudosemsenha: 'ok', libcap: 'ok', nosuid: 'ok', hardlinks: 'ok', relogio: 'ok',
      linger: 'pulado', motor: 'pulado', hostinst: 'pulado', contahub: 'pulado', ehhub: 'pulado', portaocupada: 'pulado', ...ok,
    }
    return Object.entries(padrao).map(([c, e]) => quadroEvento(c as CodigoEvento, e!)).join('') + quadroDado('arq', 'x86_64') + quadroDado('so', 'debian 13') + quadroDado('uid', uid)
  }
  const bom = avaliarSondagem(analisarSaida(Buffer.from(eventos({}))))
  assert.equal(bom.compativel, true)
  assert.equal(bom.sudo, 'sem-senha')
  assert.equal(avaliarSondagem(analisarSaida(Buffer.from(eventos({ sudosemsenha: 'pulado' })))).sudo, 'pede-senha')
  const casos: [Partial<Record<CodigoEvento, EstadoEvento>>, string][] = [
    [{ so: 'falha' }, 'so'], [{ so: 'pulado' }, 'so'], [{ systemd: 'falha' }, 'systemd'], [{ espaco: 'pulado' }, 'espaco'], [{ sudotty: 'falha', sudosemsenha: 'falha' }, 'sudotty'],
    [{ sudosemsenha: 'falha' }, 'sudo-indisponivel'], [{ ehhub: 'ok' }, 'login-e-hub'], [{ contahub: 'falha' }, 'identidade'], [{ portaocupada: 'ok' }, 'porta-ocupada'], [{ nosuid: 'falha' }, 'nosuid'],
  ]
  for (const [mudar, motivo] of casos) {
    const r = avaliarSondagem(analisarSaida(Buffer.from(eventos(mudar))))
    assert.equal(r.compativel, false, motivo)
    assert.ok(r.motivos.includes(motivo as never), `${motivo}: ${r.motivos}`)
  }
  // Saída adulterada (sem o `FIM`) falha fechada; requisito ausente também; UID de login = root.
  assert.ok(avaliarSondagem(analisarSaida(Buffer.from(eventos({}) + 'MEX1 D tam 3\n12'))).motivos.includes('saida-invalida'))
  assert.ok(avaliarSondagem(analisarSaida(Buffer.from(quadroEvento('so', 'ok')))).motivos.includes('systemd'))
  assert.ok(avaliarSondagem(analisarSaida(Buffer.from(eventos({}, '0 -')))).motivos.includes('login-root'))
  assert.ok(avaliarSondagem(analisarSaida(Buffer.from(eventos({ contahub: 'ok' }, '1000 1000')))).motivos.includes('login-e-hub'))
  assert.ok(avaliarSondagem(analisarSaida(Buffer.from(eventos({ contahub: 'ok' }, '1000 0')))).motivos.includes('hub-uid-zero'))
  assert.ok(avaliarSondagem(analisarSaida(Buffer.from(eventos({}) + quadroEvento('so', 'falha')))).motivos.includes('saida-invalida'), 'mesmo código com dois estados')
  // Porta desconhecida (ss falhou) bloqueia; arquitetura/SO `ok` sem o dado não é compatibilidade positiva; dados contraditórios não valem (nem o primeiro).
  const semDado = (tipo: string) => eventos({}).replace(new RegExp(`MEX1 D ${tipo} [^\\n]*\\n[^\\n]*\\nMEX1 FIM ${tipo}\\n`), '')
  const porta = avaliarSondagem(analisarSaida(Buffer.from(eventos({ portaocupada: 'falha' }))))
  assert.ok(!porta.compativel && porta.motivos.includes('porta-desconhecida'))
  const semArq = avaliarSondagem(analisarSaida(Buffer.from(semDado('arq'))))
  assert.ok(!semArq.compativel && semArq.arquitetura === null && semArq.motivos.includes('arquitetura'))
  assert.ok(avaliarSondagem(analisarSaida(Buffer.from(semDado('so')))).motivos.includes('so'))
  for (const extra of [quadroDado('arq', 'aarch64'), quadroDado('uid', '0 -'), quadroDado('so', 'ubuntu 24.04')]) {
    const r = avaliarSondagem(analisarSaida(Buffer.from(eventos({}) + extra)))
    assert.ok(!r.compativel && r.motivos.includes('saida-invalida'), extra)
  }
  assert.equal(avaliarSondagem(analisarSaida(Buffer.from(eventos({}) + quadroDado('arq', 'x86_64')))).compativel, true, 'repetido igual não contradiz')
})

test('catálogo: toda chave usada existe nos seis idiomas, com os mesmos parâmetros do pt-BR', () => {
  const chaves = [...Object.values(CHAVE_ESTADO), ...Object.values(CHAVE_INCOMPATIVEL), ...Object.values(CHAVE_BLOQUEIO), ...Object.values(CHAVE_ESTADO_EVENTO), ...Object.values(CHAVE_MENSAGEM),
    ...Object.values(CHAVE_EVENTO_EXIBIVEL)] as string[]
  assert.equal(new Set(chaves).size, chaves.length, 'nenhuma chave repetida')
  assert.equal(Object.keys(CHAVE_ESTADO).length, ESTADOS.length)
  const parametros = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',')
  for (const [idioma, catalogo] of Object.entries(CATALOGOS)) {
    const c = catalogo as Record<string, string>
    for (const k of chaves) {
      assert.ok(typeof c[k] === 'string' && c[k]!.trim().length > 0, `${idioma}: ${k}`)
      assert.equal(parametros(c[k]!), parametros((CATALOGOS['pt-BR'] as Record<string, string>)[k]!), `${idioma}: parâmetros de ${k}`)
    }
    assert.equal(Object.keys(c).filter((k) => k.startsWith('instalacaoRemota.')).length, chaves.length, `${idioma}: sem chave a mais nem a menos`)
  }
})
