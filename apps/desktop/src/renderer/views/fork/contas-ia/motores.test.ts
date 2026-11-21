import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Backend } from '../../../../shared/models.ts'
import { folhaNoHub, gruposDeMotores, motorEstado, precisaNode } from './motores.ts'
import { MotoresStore, type MotoresSource } from './motores-store.ts'

// D169 (ContasIa, "Motores"): os motores de Ajustes vêm do que o Hub informa, com o estado real de cada um, e a
// releitura é uma por vez, cercada por Hub, conexão e geração da conta, cancelada ao sair. Nada instala ou entra.

const b = (id: string, extra: Partial<Backend> = {}): Backend => ({ id, name: id, available: false, command: id, installHint: '', ...extra })

test('cada motor mostra só o estado que o Hub informou; Codex e Claude entram pelas contas', () => {
  const lista = [
    b('codex', { installed: true, curated: true, signedIn: false }),
    b('gemini', { installed: true, available: true, signedIn: true }),
    b('cursor', { installed: true, available: true, signedIn: false }),
    b('opencode', { installed: true, available: true }),
    b('pi', { curated: true, canInstall: true }),
    b('copilot', { curated: true, canInstall: false, installHint: 'npm i -g x' }),
    b('qwen', { available: true, node: 'missing' }),
    b('grok', { available: true }),
    b('fantasma'),
  ]
  const estado = Object.fromEntries(lista.map((x) => [x.id, motorEstado(x, 'pt-BR').selo]))
  assert.deepEqual(estado, {
    codex: 'Instalado', gemini: 'Pronto', cursor: 'Sem login', opencode: 'Login não verificado', pi: 'Não instalado',
    copilot: 'Não instalado', qwen: 'Precisa do Node.js', grok: 'Instala no primeiro uso', fantasma: 'Instala no primeiro uso',
  })
  assert.equal(motorEstado(lista[0]!, 'pt-BR').detalhe, 'Login na aba Contas.', 'nunca o login antigo do Hub para Codex/Claude')
  assert.equal(motorEstado(lista[5]!, 'pt-BR').detalhe, 'Instalação manual no Hub.')
  const grupos = gruposDeMotores(lista)
  assert.deepEqual(grupos.instalados.map((x) => x.id), ['codex', 'gemini', 'cursor', 'opencode'])
  assert.deepEqual(grupos.paraInstalar.map((x) => x.id), ['pi', 'copilot', 'qwen', 'grok'], 'o que o Hub não suporta não aparece')
  assert.equal(precisaNode(lista), true)
})

test('o motor que age sem pedir aprovação ganha o selo e a explicação, sem esconder o estado de login e instalação', () => {
  const marcado = motorEstado(b('opencode', { installed: true, available: true, signedIn: false, actsWithoutApproval: true }), 'pt-BR')
  assert.equal(marcado.selo, 'Sem login')
  assert.equal(marcado.detalhe, 'Instalado; falta entrar neste motor.')
  assert.equal(marcado.semAprovacao?.selo, 'Age sem pedir')
  assert.match(marcado.semAprovacao?.texto ?? '', /sem pedir sua aprovação/)
  const naoInstalado = motorEstado(b('pi', { curated: true, canInstall: true, actsWithoutApproval: true }), 'en')
  assert.equal(naoInstalado.selo, 'Not installed')
  assert.equal(naoInstalado.semAprovacao?.selo, 'Acts without asking')
  assert.equal(motorEstado(b('gemini', { installed: true, available: true, signedIn: true, actsWithoutApproval: false }), 'pt-BR').semAprovacao, null)
  assert.equal(motorEstado(b('gemini', { installed: true, available: true, signedIn: true }), 'pt-BR').semAprovacao, null, 'sem o campo, sem aviso')
})

class FakeSource implements MotoresSource {
  hello: { hostId: string; backends: Backend[] } | null = { hostId: 'hub-a', backends: [] }
  connectedClient: object | null = {}
  principalEpoch = 1
  calls: { hostId: string | undefined; resolve: (ok: boolean) => void }[] = []
  refreshBackends() {
    return new Promise<boolean>((resolve) => { this.calls.push({ hostId: this.hello?.hostId, resolve }) })
  }
}
const settle = () => new Promise((r) => setTimeout(r, 0))

test('folha do motor conserva a mesma identidade durante queda e reconexão; outro Hub fecha', () => {
  const source = new FakeSource()
  const folha = { hubId: 'hub-a', terminal: 'processo-capturado' }
  assert.equal(folhaNoHub(folha, source.hello?.hostId), folha)
  source.connectedClient = null
  assert.equal(folhaNoHub(folha, source.hello?.hostId), folha, 'queda não desmonta o terminal')
  source.connectedClient = {}
  source.principalEpoch++
  assert.equal(folhaNoHub(folha, source.hello?.hostId), folha, 're-hello do mesmo Hub preserva a folha')
  source.hello = { hostId: 'hub-b', backends: [] }
  assert.equal(folhaNoHub(folha, source.hello.hostId), null, 'nem o mesmo link permite pintar o Hub anterior')
  source.hello = null
  assert.equal(folhaNoHub(folha, undefined), null)
})

test('releitura: uma por vez, Hub trocado relê uma vez para o atual, sair cancela e erro mantém a lista', async () => {
  const source = new FakeSource()
  const store = new MotoresStore()
  void store.refresh(source)
  void store.refresh(source)
  assert.equal(source.calls.length, 1, 'pedido duplo durante a leitura não chama de novo')
  assert.equal(store.state, 'loading')

  // A resposta chega depois de o store passar a outro Hub: não marca nada e relê uma vez para o atual.
  source.hello = { hostId: 'hub-b', backends: [] }
  source.connectedClient = {}
  source.calls[0]!.resolve(true)
  await settle()
  assert.equal(source.calls.length, 2)
  assert.equal(source.calls[1]!.hostId, 'hub-b')
  assert.equal(store.asOf, null, 'a resposta do hub-a não conta como atualizada')
  source.calls[1]!.resolve(true)
  await settle()
  assert.equal(store.state, 'ready')
  assert.ok(store.asOf)

  // Sair da página com a leitura em voo: a resposta tardia não muda o estado.
  const asOf = store.asOf
  void store.refresh(source)
  store.cancel()
  source.calls[2]!.resolve(true)
  await settle()
  assert.equal(store.state, 'idle')
  assert.equal(store.asOf, asOf)

  void store.refresh(source)
  source.calls[3]!.resolve(false)
  await settle()
  assert.equal(store.state, 'error', 'falha avisa, sem esvaziar a lista do Hub')
})

test('DON-917: o estado do motor sai no idioma pedido', () => {
  assert.equal(motorEstado({ id: 'grok', available: true } as never, 'en').selo, 'Installs on first use')
})
