import assert from 'node:assert/strict'
import { test } from 'node:test'
import { CAMPOS, MARCA, SobreVoceStore, escreverPerfil, lerPerfil, vazio, type Perfil } from './sobre-voce.ts'

const aparado = (p: Perfil): Perfil => Object.fromEntries(CAMPOS.map((c) => [c, p[c].trim()])) as Perfil

test('os cinco campos vão e voltam pela nota', () => {
  const perfil = { quem: 'Ana, Curitiba.', pensa: 'Dados antes.', faz: '', fala: 'Curto.', evitar: 'Nada de emoji.' }
  const nota = escreverPerfil(perfil)!
  assert.ok(nota.startsWith(`${MARCA}\n# Sobre você\n\n## Quem é\nAna, Curitiba.`))
  assert.match(nota, /## Prefiro que evite\nNada de emoji\./)
  assert.doesNotMatch(nota, /O que faz/)
  assert.deepEqual(lerPerfil(nota), perfil)
})

test('ida e volta exata com Markdown, títulos dentro do campo e barras', () => {
  const casos: Perfil[] = [
    { ...vazio(), quem: '## Quem é\nfalso título', pensa: '# Sobre você\n## Como pensa\n### sub' },
    { ...vazio(), faz: '- lista\n- **negrito**\n\n```\n## dentro de código\n```', fala: '\\## já escapado\n\\\\# duas barras' },
    { ...vazio(), evitar: 'linha 1\n\n\nlinha 4\n## O que não fazer\nfim', quem: '#hashtag no começo' },
    { quem: '  espaço  ', pensa: '\n\nquebras\n\n', faz: 'a\r\nb', fala: '#', evitar: '\\' },
  ]
  for (const p of casos) {
    const esperado = aparado({ ...p, faz: p.faz.replace(/\r\n/g, '\n') })
    assert.deepEqual(lerPerfil(escreverPerfil(p)), esperado, JSON.stringify(p))
    // Gravar de novo o que foi lido dá a mesma nota.
    assert.equal(escreverPerfil(lerPerfil(escreverPerfil(p))), escreverPerfil(esperado))
  }
})

test('nota legada (sem marca) é lida literal: \\#tag fica \\#tag e sobrevive a salvar outro campo', async () => {
  const legada = '# Sobre você\n\n## Quem é\n\\#tag do dono\n## O que não fazer\nSem ligação'
  assert.deepEqual(lerPerfil(legada), { ...vazio(), quem: '\\#tag do dono', evitar: 'Sem ligação' }, 'título antigo cai em Prefiro que evite')
  let gravado: string | null = null
  const store = new SobreVoceStore(async () => ({
    call: async <T,>(method: string, body?: unknown): Promise<T> => {
      if (method === 'memoryProfile') return { content: legada, revision: '1', loose: 0 } as T
      gravado = (body as { content: string | null }).content
      return { content: gravado, revision: '2', loose: 0 } as T
    },
  }))
  await store.load()
  store.editar('fala', 'Curto')
  await store.save()
  assert.ok(gravado!.startsWith(MARCA), 'gravada no formato novo')
  assert.equal(store.rascunho.quem, '\\#tag do dono', 'a barra do dono continua lá')
  assert.match(gravado!, /## Quem é\n\\\\#tag do dono/, 'regravada escapada')
  assert.match(gravado!, /## Prefiro que evite\nSem ligação/)
})

test('nota marcada desescapa; a mesma linha sem marca não', () => {
  const corpo = '# Sobre você\n\n## Quem é\n\\## não é título'
  assert.equal(lerPerfil(`${MARCA}\n${corpo}`).quem, '## não é título')
  assert.equal(lerPerfil(corpo).quem, '\\## não é título')
})

test('nota editada à mão: título repetido junta no mesmo campo; texto solto não some', () => {
  const nota = 'Nota antiga\n\n## Quem é\nAna\n## como fala\nDireto\n## Quem é\nMora em Curitiba\n## Outra coisa\nfica no campo'
  assert.deepEqual(lerPerfil(nota), { ...vazio(), quem: 'Nota antiga\n\nAna\n\nMora em Curitiba\n## Outra coisa\nfica no campo', fala: 'Direto' })
  assert.equal(escreverPerfil(vazio()), null)
})

const hub = () => {
  const estado = { online: true, content: '## Quem é\nAna' as string | null, revision: 1, falhaGravar: false }
  const store = new SobreVoceStore(async () => {
    if (!estado.online) throw new Error('memory_connection_unavailable')
    return {
      call: async <T,>(method: string, body?: unknown): Promise<T> => {
        if (method === 'memoryProfile') return { content: estado.content, revision: String(estado.revision), loose: 3 } as T
        if (estado.falhaGravar) throw new Error('timeout')
        const b = body as { content: string | null; base: string }
        if (b.base !== String(estado.revision)) throw new Error('memory_profile_conflict')
        estado.content = b.content
        estado.revision++
        return { content: estado.content, revision: String(estado.revision), loose: 3 } as T
      },
    }
  })
  return { estado, store }
}

test('editar → desconectar → reconectar mantém o rascunho', async () => {
  const { estado, store } = hub()
  await store.load()
  store.editar('fala', 'Curto')
  estado.online = false
  await store.load()
  assert.equal(store.problema, 'leitura')
  assert.equal(store.rascunho.fala, 'Curto')
  estado.online = true
  estado.content = '## Quem é\nAna\n## Como pensa\nmudou noutro lugar'
  await store.load()
  assert.equal(store.problema, null)
  assert.equal(store.rascunho.fala, 'Curto', 'reler não apaga o rascunho')
  assert.equal(store.revisao, '1', 'a base do rascunho continua a lida antes')
  await store.save()
  assert.equal(store.problema, null)
  assert.match(estado.content!, /## Como fala\nCurto/)
})

test('falha de gravação é diferente de leitura e guarda o rascunho; tentar de novo grava', async () => {
  const { estado, store } = hub()
  await store.load()
  store.editar('evitar', 'Sem emoji')
  estado.falhaGravar = true
  await store.save()
  assert.equal(store.problema, 'gravacao')
  assert.equal(store.rascunho.evitar, 'Sem emoji')
  estado.falhaGravar = false
  await store.save()
  assert.equal(store.problema, null)
  assert.equal(store.gravou, true)
})

test('conflito guarda o rascunho; só o Recarregar explícito troca pela versão nova', async () => {
  const { estado, store } = hub()
  await store.load()
  assert.equal(store.soltas, 3)
  store.editar('fala', 'Curto')
  estado.revision = 9
  await store.save()
  assert.equal(store.problema, 'conflito')
  assert.equal(store.rascunho.fala, 'Curto')
  store.descartar()
  await new Promise((r) => setTimeout(r, 0))
  assert.equal(store.rascunho.fala, '')
  assert.equal(store.revisao, '9')
})

test('secret_detected (#698): aviso próprio, rascunho mantido, foco no campo editado; editar tira o aviso', async () => {
  // Forma real do Hub: HTTP 400 com a mensagem do anyhow (`{e:#}`, pode vir com contexto antes), sem repetir o trecho.
  const erro = (m: string) => Object.assign(new Error(m), { status: 400 })
  let recusa = 'secret_detected'
  const store = new SobreVoceStore(async () => ({
    call: async <T,>(method: string): Promise<T> => {
      if (method === 'memoryProfile') return { content: null, revision: null, loose: 0 } as T
      throw erro(recusa)
    },
  }))
  await store.load()
  store.editar('quem', 'Ana')
  store.editar('faz', 'Wi-Fi password: hunter22')
  await store.save()
  assert.equal(store.problema, 'segredo')
  assert.equal(store.rascunho.faz, 'Wi-Fi password: hunter22', 'o rascunho fica')
  assert.equal(store.campoDoSegredo, 'faz', 'o foco vai ao último campo editado')
  store.editar('faz', 'Wi-Fi: pergunte')
  assert.equal(store.problema, null, 'mexer no texto tira o aviso')
  recusa = 'gravando a nota: secret_detected'
  await store.save()
  assert.equal(store.problema, 'segredo', 'o código com contexto antes também é reconhecido')
  recusa = 'saving: memory_profile_conflict'
  store.editar('faz', 'outra')
  await store.save()
  assert.equal(store.problema, 'conflito')
  assert.equal(store.campoDoSegredo, null)
})

test('Hub sem o método: indisponível, não erro', async () => {
  const store = new SobreVoceStore(async () => ({ call: async () => { throw Object.assign(new Error('unknown method'), { status: 404 }) } }))
  await store.load()
  assert.equal(store.problema, 'indisponivel')
})
