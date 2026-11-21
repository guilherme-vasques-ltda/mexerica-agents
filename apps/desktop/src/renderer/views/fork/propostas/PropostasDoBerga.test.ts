import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { CATALOGOS, tradutor } from '../../../lib/fork-i18n/index.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import type { TaskProposalsSnapshot } from '../../../../shared/fork-propostas.ts'

const bundle = await build({
  stdin: {
    contents: "export { PropostasCard } from './PropostasDoBerga'; export { createElement } from 'react'; export { renderToStaticMarkup } from 'react-dom/server';",
    resolveDir: new URL('.', import.meta.url).pathname,
    loader: 'ts',
  },
  bundle: true,
  platform: 'node',
  format: 'cjs',
  write: false,
  jsx: 'automatic',
  plugins: [
    {
      name: 'stubs',
      setup(b) {
        b.onResolve({ filter: /\.css$/ }, (args) => ({ path: args.path, namespace: 'empty-css' }))
        b.onLoad({ filter: /.*/, namespace: 'empty-css' }, () => ({ contents: '', loader: 'js' }))
        b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
        b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
      },
    },
  ],
})

const moduleObj = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module: moduleObj,
  exports: moduleObj.exports,
  require: createRequire(import.meta.url),
  process,
  console,
  TextEncoder,
  window: { addEventListener() {}, removeEventListener() {}, matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) },
  localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
  setTimeout,
  clearTimeout,
  queueMicrotask,
})

const m = moduleObj.exports as {
  PropostasCard: (p: Record<string, unknown>) => unknown
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const render = (props: Record<string, unknown>) => m.renderToStaticMarkup(m.createElement(m.PropostasCard as never, props))

const snapshotSample: TaskProposalsSnapshot = {
  version: 1,
  hubId: 'hub-1',
  spaceId: 'space-1',
  limits: { perBot: 1, total: 2 },
  proposals: [
    {
      id: 'p-pending',
      version: 1,
      state: 'pending',
      botId: 'berga',
      projectId: 'proj-1',
      projectName: 'Campanha',
      title: 'Subir página de lançamento',
      createdAt: 1000,
      decidedAt: null,
      items: [
        {
          id: 'i-1',
          title: 'Escrever texto principal',
          description: 'Redigir chamada e subtítulo da dobra inicial',
          acceptance: 'Texto aprovado com até 120 caracteres',
          assigneeBotId: 'b-copy',
          included: true,
          taskId: null,
          taskState: null,
          startState: null,
        },
      ],
    },
    {
      id: 'p-approved',
      version: 2,
      state: 'approved',
      botId: 'berga',
      projectId: 'proj-1',
      projectName: 'Campanha',
      title: 'Ajustes em curso',
      createdAt: 900,
      decidedAt: 950,
      items: [
        {
          id: 'i-2',
          title: 'Configurar DNS',
          description: '',
          acceptance: '',
          assigneeBotId: 'b-ops',
          included: true,
          taskId: 't-2',
          taskState: 'in_progress',
          startState: 'started',
        },
        {
          id: 'i-3',
          title: 'Validar layout',
          description: '',
          acceptance: '',
          assigneeBotId: 'b-design',
          included: true,
          taskId: 't-3',
          taskState: 'in_progress',
          startState: 'started',
        },
        {
          id: 'i-4',
          title: 'Revisar links',
          description: '',
          acceptance: '',
          assigneeBotId: 'b-qa',
          included: true,
          taskId: 't-4',
          taskState: 'review',
          startState: 'started',
        },
        {
          id: 'i-5',
          title: 'Publicar nota',
          description: '',
          acceptance: '',
          assigneeBotId: 'b-pub',
          included: true,
          taskId: 't-5',
          taskState: 'backlog',
          startState: 'scheduled',
        },
      ],
    },
  ],
}

for (const idioma of Object.keys(CATALOGOS) as AppLanguage[]) {
  test(`cartão renderiza sem parâmetros soltos em ${idioma}`, () => {
    const html = render({
      snapshot: snapshotSample,
      write: null,
      stale: false,
      rereadFailed: false,
      canDecide: true,
      botName: (id: string | null) => id ?? '-',
      t: tradutor(idioma),
      idioma,
      onApprove: () => {},
      onReject: () => {},
      onRetry: () => {},
      onDismissWrite: () => {},
    })
    assert.ok(html.length > 0)
    assert.doesNotMatch(html, /\{\w+\}/)
    assert.match(html, /aria-expanded="true"/)
    assert.match(html, /type="checkbox"/)
  })
}

test('cartão em pt-BR mostra aviso antes do ok, regra de início, review aguardando você e Aguardando vaga com 2 ativas', () => {
  const html = render({
    snapshot: snapshotSample,
    write: null,
    stale: false,
    rereadFailed: false,
    canDecide: true,
    botName: (id: string | null) => ({ 'b-copy': 'Redator', 'b-ops': 'Infra', 'b-design': 'Design', 'b-qa': 'Revisor', 'b-pub': 'Publicador' }[id ?? ''] ?? 'Sem responsável'),
    t: tradutor('pt-BR'),
    idioma: 'pt-BR',
    onApprove: () => {},
    onReject: () => {},
    onRetry: () => {},
    onDismissWrite: () => {},
  })
  assert.match(html, /Nada começou ainda/)
  assert.match(html, /Redigir chamada e subtítulo da dobra inicial/)
  assert.match(html, /Critério de aceite/)
  assert.match(html, /Texto aprovado com até 120 caracteres/)
  assert.match(html, /Aprovar começa o trabalho; máx\. 2 ao mesmo tempo, 1 por bot\./)
  assert.match(html, /Em conferência · aguardando você/)
  assert.match(html, /Aguardando vaga/)
})

test('cartão mostra erro de leitura inicial com ação Tentar de novo em todos os idiomas', () => {
  for (const idioma of Object.keys(CATALOGOS) as AppLanguage[]) {
    const html = render({
      snapshot: null,
      write: null,
      stale: false,
      rereadFailed: false,
      canDecide: false,
      readError: 'failed',
      retryingRead: false,
      botName: () => 'Bot',
      t: tradutor(idioma),
      idioma,
      onApprove: () => {},
      onReject: () => {},
      onRetry: () => {},
      onDismissWrite: () => {},
      onRetryRead: () => {},
    })
    assert.ok(html.length > 0)
    assert.doesNotMatch(html, /\{\w+\}/)
    assert.match(html, /role="alert"/)
  }
  const pt = render({
    snapshot: null,
    write: null,
    stale: false,
    rereadFailed: false,
    canDecide: false,
    readError: 'failed',
    retryingRead: false,
    botName: () => 'Bot',
    t: tradutor('pt-BR'),
    idioma: 'pt-BR',
    onApprove: () => {},
    onReject: () => {},
    onRetry: () => {},
    onDismissWrite: () => {},
    onRetryRead: () => {},
  })
  assert.match(pt, /Não foi possível carregar as propostas agora\./)
  assert.match(pt, /Tentar de novo/)
})

test('cartão não renderiza quando só há propostas recusadas ou aprovadas já concluídas', () => {
  const onlyDone: TaskProposalsSnapshot = {
    version: 1,
    hubId: 'hub-1',
    spaceId: 'space-1',
    limits: { perBot: 1, total: 2 },
    proposals: [
      {
        ...snapshotSample.proposals[1]!,
        items: [
          {
            id: 'i-done',
            title: 'Feita',
            description: '',
            acceptance: '',
            assigneeBotId: 'b1',
            included: true,
            taskId: 't1',
            taskState: 'done',
            startState: 'started',
          },
        ],
      },
    ],
  }
  const html = render({
    snapshot: onlyDone,
    write: null,
    stale: false,
    rereadFailed: false,
    canDecide: true,
    botName: () => 'Bot',
    t: tradutor('pt-BR'),
    idioma: 'pt-BR',
    onApprove: () => {},
    onReject: () => {},
    onRetry: () => {},
    onDismissWrite: () => {},
  })
  assert.equal(html, '')
})

test('stream reconectado com snapshot em cache e releitura falha mostra Tentar de novo e mantém ações desabilitadas', () => {
  const html = render({
    snapshot: snapshotSample,
    write: null,
    stale: true,
    rereadFailed: true,
    canDecide: false,
    retryingRead: false,
    botName: () => 'Bot',
    t: tradutor('pt-BR'),
    idioma: 'pt-BR',
    onApprove: () => {},
    onReject: () => {},
    onRetry: () => {},
    onDismissWrite: () => {},
    onRetryRead: () => {},
  })
  assert.match(html, /Não foi possível atualizar agora\. Mostrando a última lista\./)
  assert.match(html, /Tentar de novo/)
  assert.doesNotMatch(html, /Sem conexão agora\. As ações esperam a reconexão\./)
  assert.match(html, /<button[^>]*disabled=""[^>]*>Recusar<\/button>/)
  assert.match(html, /<button[^>]*disabled=""[^>]*>Aprovar<\/button>/)
  assert.match(html, /<input[^>]*type="checkbox"[^>]*disabled=""/)

  const busy = render({
    snapshot: snapshotSample,
    write: null,
    stale: true,
    rereadFailed: true,
    canDecide: false,
    retryingRead: true,
    botName: () => 'Bot',
    t: tradutor('pt-BR'),
    idioma: 'pt-BR',
    onApprove: () => {},
    onReject: () => {},
    onRetry: () => {},
    onDismissWrite: () => {},
    onRetryRead: () => {},
  })
  assert.match(busy, /<button[^>]*disabled=""[^>]*aria-busy="true"[^>]*>Tentar de novo<\/button>/)

  const paused = render({
    snapshot: snapshotSample,
    write: null,
    stale: true,
    rereadFailed: true,
    canDecide: false,
    botName: () => 'Bot',
    t: tradutor('pt-BR'),
    idioma: 'pt-BR',
    onApprove: () => {},
    onReject: () => {},
    onRetry: () => {},
    onDismissWrite: () => {},
  })
  assert.match(paused, /Sem conexão agora\. As ações esperam a reconexão\./)
  assert.doesNotMatch(paused, /Tentar de novo/)
})

test('caixa de inclusão usa aria-describedby único por bloco de detalhes e omite quando não há detalhes', () => {
  const multi: TaskProposalsSnapshot = {
    version: 1,
    hubId: 'hub-1',
    spaceId: 'space-1',
    limits: { perBot: 1, total: 2 },
    proposals: [
      {
        id: 'p-1',
        version: 1,
        state: 'pending',
        botId: 'berga',
        projectId: 'proj-1',
        projectName: 'Campanha',
        title: 'Lista mista',
        createdAt: 1000,
        decidedAt: null,
        items: [
          {
            id: 'item-a',
            title: 'Com descrição e aceite',
            description: 'Detalhe A',
            acceptance: 'Aceite A',
            assigneeBotId: 'b1',
            included: true,
            taskId: null,
            taskState: null,
            startState: null,
          },
          {
            id: 'item-b',
            title: 'Só aceite',
            description: '   ',
            acceptance: 'Aceite B',
            assigneeBotId: 'b2',
            included: true,
            taskId: null,
            taskState: null,
            startState: null,
          },
          {
            id: 'item-c',
            title: 'Sem detalhes',
            description: '   ',
            acceptance: '',
            assigneeBotId: 'b3',
            included: true,
            taskId: null,
            taskState: null,
            startState: null,
          },
        ],
      },
    ],
  }

  const baseProps = {
    snapshot: multi,
    write: null,
    stale: false,
    rereadFailed: false,
    canDecide: true,
    botName: () => 'Bot',
    t: tradutor('pt-BR'),
    idioma: 'pt-BR' as const,
    onApprove: () => {},
    onReject: () => {},
    onRetry: () => {},
    onDismissWrite: () => {},
  }

  const html = render(baseProps)
  const inputs = [...html.matchAll(/<input[^>]*type="checkbox"[^>]*>/g)].map((r) => r[0])
  assert.equal(inputs.length, 3)

  const descIdA = inputs[0]!.match(/aria-describedby="([^"]+)"/)?.[1]
  const descIdB = inputs[1]!.match(/aria-describedby="([^"]+)"/)?.[1]
  const descIdC = inputs[2]!.match(/aria-describedby="([^"]+)"/)?.[1]
  assert.ok(descIdA, 'item A deve ter aria-describedby')
  assert.ok(descIdB, 'item B deve ter aria-describedby')
  assert.notEqual(descIdA, descIdB)
  assert.equal(descIdC, undefined, 'item sem detalhes não deve ter aria-describedby')

  const detailIds = [...html.matchAll(/<div class="prp-item-details" id="([^"]+)">/g)].map((r) => r[1])
  assert.deepEqual(detailIds, [descIdA, descIdB])

  // Dois cartões renderizados juntos (como no Lado a lado) mantêm ids únicos entre si.
  const twoCardsHtml = m.renderToStaticMarkup(
    m.createElement('div', null, m.createElement(m.PropostasCard as never, baseProps), m.createElement(m.PropostasCard as never, baseProps)),
  )
  const allDetailIds = [...twoCardsHtml.matchAll(/<div class="prp-item-details" id="([^"]+)">/g)].map((r) => r[1])
  assert.equal(allDetailIds.length, 4)
  assert.equal(new Set(allDetailIds).size, 4)
})
