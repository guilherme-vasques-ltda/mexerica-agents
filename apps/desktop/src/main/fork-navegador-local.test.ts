import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  abrirNavegadorLocal, criarCicloFixture, LIMITE_CAPTURA_BYTES, NAVEGADOR_LOCAL_HABILITADO,
} from './fork-navegador-local/sessao.ts'
import type { GateHubLocal, SuperficieFixture } from './fork-navegador-local/sessao.ts'

const alvo = { workspace: 'workspace-a', bot: 'bot-a', geracao: 'geracao-a' }
const origem = 'https://pagina.example.invalid'
const URL_PAGINA = `${origem}/pagina`
const pedidoLocal = Object.freeze({})
const gateLocal: GateHubLocal = {
  autoriza: (_comando, vinculo, pedido) => pedido === pedidoLocal &&
    vinculo.workspace === alvo.workspace && vinculo.bot === alvo.bot && vinculo.geracao === alvo.geracao,
}

function fixture(opcoes: Partial<SuperficieFixture> = {}, gate = gateLocal) {
  const chamadas: string[] = []
  const configs: Parameters<Parameters<typeof criarCicloFixture>[0]>[0][] = []
  const sessao = criarCicloFixture((config) => {
    configs.push(config)
    return {
      capturar: async () => Uint8Array.of(1),
      fechar: async () => { chamadas.push('fechar'); return true },
      limparArmazenamento: async () => { chamadas.push('armazenamento'); return true },
      limparCache: async () => { chamadas.push('cache'); return true },
      ...opcoes,
    }
  }, gate)
  return { ...sessao, chamadas, configs }
}

function abrir({ comandos }: ReturnType<typeof fixture>) {
  const abriu = comandos.abrir(pedidoLocal, alvo, URL_PAGINA)
  assert.equal(abriu.estado, 'aberta')
  return abriu.vinculo
}

function pendencia<T>() {
  let resolve!: (valor: T) => void
  const promise = new Promise<T>((resolver) => { resolve = resolver })
  return { promise, resolve }
}

test('a entrada de produção permanece desligada e sem adaptador real', () => {
  assert.equal(NAVEGADOR_LOCAL_HABILITADO, false)
  assert.deepEqual(abrirNavegadorLocal(), { estado: 'indisponivel', motivo: 'desligado' })
})

test('não cria superfície sem decisão afirmativa do gate local', () => {
  const semGate = criarCicloFixture(() => assert.fail('sem gate não pode criar superfície'))
  const comErro = fixture({}, { autoriza: () => { throw new Error() } })
  const local = fixture()
  assert.equal(semGate.comandos.abrir(pedidoLocal, alvo, URL_PAGINA).estado, 'negado')
  assert.equal(comErro.comandos.abrir(pedidoLocal, alvo, URL_PAGINA).estado, 'negado')
  assert.equal(local.comandos.abrir('Caller::Local', alvo, URL_PAGINA).estado, 'negado')
  assert.equal(local.comandos.abrir({ origem: 'remota' }, alvo, URL_PAGINA).estado, 'negado')
  assert.equal(local.comandos.abrir(pedidoLocal, { ...alvo, workspace: 'workspace-b' }, URL_PAGINA).estado, 'negado')
  assert.equal(local.comandos.abrir(pedidoLocal, { ...alvo, geracao: 'geracao-b' }, URL_PAGINA).estado, 'negado')
  assert.equal(local.comandos.abrir(pedidoLocal, alvo, 'file:///pagina').estado, 'negado')
  assert.equal(comErro.configs.length + local.configs.length, 0)
})

test('nega permissões, dispositivos, janelas, downloads e destinos fora da fixture', () => {
  const local = fixture()
  abrir(local)
  const { guardas } = local.configs[0]
  assert.equal(guardas.permitePermissao(), false)
  assert.equal(guardas.permiteDispositivo(), false)
  assert.equal(guardas.permiteJanela(), false)
  assert.equal(guardas.permiteDownload(), false)
  for (const tipo of ['navegacao', 'redirecionamento', 'subrecurso'] as const) {
    const permitido = tipo === 'subrecurso' ? `${origem}/imagem.png` : URL_PAGINA
    assert.equal(guardas.permiteUrl(tipo, permitido), true)
    for (const url of ['https://outra.example.invalid/pagina', 'file:///pagina',
      'http://pagina.example.invalid/pagina', 'https://pagina.example.invalid:444/pagina',
      `${origem}/outra`, `blob:${URL_PAGINA}`, `${permitido}?extra`, 'invalido']) {
      assert.equal(guardas.permiteUrl(tipo, url), false, `${tipo}: ${url}`)
    }
  }
  assert.equal(guardas.permiteUrl('subrecurso', URL_PAGINA), false)
  assert.equal(guardas.permiteUrl('redirecionamento', `${origem}/imagem.png`), false)
})

test('modo assistir só expõe abrir, ler e parar e entrega bytes limitados e não confiáveis', async () => {
  const quadro = new Uint8Array(LIMITE_CAPTURA_BYTES)
  const local = fixture({ capturar: async () => quadro })
  const vinculo = abrir(local)
  assert.deepEqual(Object.keys(local.comandos), ['abrir', 'ler', 'parar'])
  const captura = await local.comandos.ler(pedidoLocal, vinculo)
  assert.equal(captura.estado, 'capturada')
  assert.equal(captura.confianca, 'nao-confiavel')
  assert.equal(captura.bytes.length, LIMITE_CAPTURA_BYTES)
  quadro[0] = 1
  assert.equal(captura.bytes[0], 0)
  for (const tamanho of [0, LIMITE_CAPTURA_BYTES + 1]) {
    const local = fixture({ capturar: async () => new Uint8Array(tamanho) })
    assert.deepEqual(await local.comandos.ler(pedidoLocal, abrir(local)), { estado: 'indisponivel' })
  }
})

test('ler e parar não aceitam origem remota, outro workspace, bot, geração ou instância', async () => {
  const local = fixture({ capturar: async () => assert.fail('sem informação para outro alvo') })
  const vinculo = abrir(local)
  const outros = [
    { ...vinculo, workspace: 'workspace-b' }, { ...vinculo, bot: 'bot-b' },
    { ...vinculo, geracao: 'geracao-b' }, { ...vinculo, instancia: 'instancia-b' },
  ]
  for (const comando of [local.comandos.ler, local.comandos.parar]) {
    assert.equal((await comando({ origem: 'remota' }, vinculo)).estado, 'negado')
    for (const outro of outros) assert.equal((await comando(pedidoLocal, outro)).estado, 'negado')
  }
  assert.equal(await local.ciclo.revogar(outros[0]), false)
  assert.deepEqual(local.chamadas, [])
})

test('o gate recebe a ação e a instância exatas; aprovação anterior não autoriza a nova', async () => {
  const aprovacoes = new Map<unknown, { comando: string; instancia: string }>()
  const local = fixture({}, {
    autoriza: (comando, vinculo, pedido) => {
      if (comando === 'abrir') return gateLocal.autoriza(comando, vinculo, pedido)
      const aprovada = aprovacoes.get(pedido)
      return aprovada?.comando === comando && aprovada.instancia === vinculo.instancia
    },
  })
  const anterior = abrir(local)
  const leitura = Object.freeze({})
  aprovacoes.set(leitura, { comando: 'ler', instancia: anterior.instancia })
  assert.equal((await local.comandos.ler(leitura, anterior)).estado, 'capturada')
  assert.equal((await local.comandos.parar(leitura, anterior)).estado, 'negado')
  assert.equal(await local.ciclo.revogar(anterior), true)
  const nova = abrir(local)
  assert.equal((await local.comandos.ler(leitura, nova)).estado, 'negado')
})

for (const evento of ['parar', 'revogar', 'hubCaiu', 'destruirApp', 'mudarGeracao'] as const) {
  test(`${evento} cancela leitura pendente e corta as guardas antes da limpeza`, async () => {
    const captura = pendencia<Uint8Array>()
    let sinal: AbortSignal | undefined
    const local = fixture({ capturar: (s) => { sinal = s; return captura.promise } })
    const vinculo = abrir(local)
    const pendente = local.comandos.ler(pedidoLocal, vinculo)
    assert.ok(sinal)
    assert.equal((await local.comandos.ler(pedidoLocal, vinculo)).estado, 'indisponivel')

    const fim = evento === 'parar' ? local.comandos.parar(pedidoLocal, vinculo)
      : evento === 'revogar' ? local.ciclo.revogar(vinculo) : local.ciclo[evento]()
    assert.equal(sinal.aborted, true)
    assert.equal(local.configs[0].guardas.permiteUrl('subrecurso', `${origem}/imagem.png`), false)
    assert.equal((await pendente).estado, 'indisponivel')
    assert.deepEqual(await fim, evento === 'parar' ? { estado: 'encerrada' } : true)
    assert.deepEqual(local.chamadas, ['fechar', 'armazenamento', 'cache'])

    if (evento === 'parar' || evento === 'revogar') {
      const nova = abrir(local)
      assert.notEqual(nova.instancia, vinculo.instancia)
      const particoes = local.configs.map((c) => c.particao)
      for (const particao of particoes) assert.match(particao, /^navegador-local-[\da-f-]{36}$/)
      assert.notEqual(particoes[0], particoes[1])
      captura.resolve(Uint8Array.of(1))
      assert.equal((await local.comandos.ler(pedidoLocal, vinculo)).estado, 'negado')
      assert.equal(local.configs[0].guardas.permiteUrl('navegacao', URL_PAGINA), false)
    } else {
      captura.resolve(Uint8Array.of(1))
      assert.equal(local.comandos.abrir(pedidoLocal, alvo, URL_PAGINA).estado, 'indisponivel')
    }
  })
}

test('revalida o gate após a captura e descarta quando a autorização deixou de valer', async () => {
  const captura = pendencia<Uint8Array>()
  let autorizado = true
  const local = fixture({ capturar: () => captura.promise }, {
    autoriza: (comando, vinculo, pedido) => autorizado && gateLocal.autoriza(comando, vinculo, pedido),
  })
  const vinculo = abrir(local)
  const pendente = local.comandos.ler(pedidoLocal, vinculo)
  autorizado = false
  captura.resolve(Uint8Array.of(1))
  assert.equal((await pendente).estado, 'negado')
  assert.equal(local.configs[0].guardas.permiteUrl('navegacao', URL_PAGINA), false)
  assert.equal(await local.ciclo.revogar(vinculo), true)
})

test('erro do gate aposenta o ciclo e cancela a leitura em andamento', async () => {
  let falhou = false
  const captura = pendencia<Uint8Array>()
  const local = fixture({ capturar: () => captura.promise }, {
    autoriza: (comando, vinculo, pedido) => {
      if (falhou) throw new Error()
      return gateLocal.autoriza(comando, vinculo, pedido)
    },
  })
  const vinculo = abrir(local)
  const pendente = local.comandos.ler(pedidoLocal, vinculo)
  falhou = true
  assert.equal((await local.comandos.parar(pedidoLocal, vinculo)).estado, 'negado')
  assert.equal((await pendente).estado, 'indisponivel')
  assert.equal(await local.ciclo.hubCaiu(), true)
  captura.resolve(Uint8Array.of(1))
  falhou = false
  assert.equal(local.comandos.abrir(pedidoLocal, alvo, URL_PAGINA).estado, 'indisponivel')
})

test('não reabre durante limpeza nem depois de fechamento ou descarte sem confirmação', async () => {
  const fechar = pendencia<boolean>()
  const local = fixture({
    fechar: () => fechar.promise,
  })
  const vinculo = abrir(local)
  const fim = local.comandos.parar(pedidoLocal, vinculo)
  assert.equal(local.comandos.abrir(pedidoLocal, alvo, URL_PAGINA).estado, 'indisponivel')
  fechar.resolve(false)
  assert.equal((await fim).estado, 'indisponivel')
  assert.deepEqual(local.chamadas, ['armazenamento', 'cache'])
  assert.equal(await local.ciclo.revogar(vinculo), false)
  assert.equal(local.comandos.abrir(pedidoLocal, alvo, URL_PAGINA).estado, 'indisponivel')
  assert.equal(local.configs.length, 1)
})

for (const etapa of ['limparArmazenamento', 'limparCache'] as const) {
  test(`falha de ${etapa} impede reabrir mesmo com fechamento confirmado`, async () => {
    const local = fixture({ [etapa]: async () => { throw new Error() } })
    const vinculo = abrir(local)
    assert.equal((await local.comandos.parar(pedidoLocal, vinculo)).estado, 'indisponivel')
    assert.deepEqual(local.chamadas, ['fechar', etapa === 'limparCache' ? 'armazenamento' : 'cache'])
    assert.equal(local.comandos.abrir(pedidoLocal, alvo, URL_PAGINA).estado, 'indisponivel')
    assert.equal(local.configs.length, 1)
  })
}
