// SPDX-License-Identifier: Apache-2.0
import assert from 'node:assert/strict'
import test from 'node:test'
import { resolveState } from './mexeriquinhas/estado.ts'
import { CATALOGO_FONTE, CATEGORIAS, GRUPOS, MODELOS, modeloPorId, modelosDaCategoria } from './mexeriquinhas/modelos.ts'
import { BASES_VISUAIS, baseVisualPorId } from './mexeriquinhas/bases.ts'
import { normalizarConfig } from './mexeriquinhas/pecas.ts'
import { ESTADOS } from './mexeriquinhas/estado.ts'
import { CONJUNTOS, conjuntoDoModelo } from './mexeriquinhas/conjuntos.ts'
import { renderAvatar, renderSimples, renderPelucia, QUADROS, type EscolhaAvatar } from './mexeriquinhas/index.ts'
import { CONFIG_BASE } from './mexeriquinhas/pecas.ts'

const OITO_HORAS = 8 * 60 * 60 * 1_000
const AGORA = Date.UTC(2026, 10, 8, 15)

test('dorme somente depois de mais de oito horas sem atividade', () => {
  assert.equal(resolveState({ ultimaAtividade: AGORA - OITO_HORAS + 1 }, AGORA), 'livre')
  assert.equal(resolveState({ ultimaAtividade: AGORA - OITO_HORAS }, AGORA), 'livre')
  assert.equal(resolveState({ ultimaAtividade: AGORA - OITO_HORAS - 1 }, AGORA), 'dormindo')
})

test('trabalho, resposta pendente e erro prevalecem sobre o tempo parado', () => {
  const ultimaAtividade = AGORA - 2 * OITO_HORAS
  assert.equal(resolveState({ ultimaAtividade, ocupado: true }, AGORA), 'trabalhando')
  assert.equal(resolveState({ ultimaAtividade, ocupado: true, precisaResposta: true }, AGORA), 'precisa-resposta')
  assert.equal(resolveState({ ultimaAtividade, ocupado: true, precisaResposta: true, erro: true }, AGORA), 'erro')
})

test('não inventa sono para horário ausente ou futuro', () => {
  assert.equal(resolveState({}, AGORA), 'livre')
  assert.equal(resolveState({ ultimaAtividade: null }, AGORA), 'livre')
  assert.equal(resolveState({ ultimaAtividade: AGORA + OITO_HORAS }, AGORA), 'livre')
  assert.equal(resolveState({ ultimaAtividade: 0 }, OITO_HORAS + 1), 'dormindo')
})

test('avatares lado a lado têm IDs distintos e referências locais completas', () => {
  const primeiro = renderAvatar(CONFIG_BASE, { size: 128 })
  const segundo = renderAvatar(CONFIG_BASE, { size: 128 })
  const ids = (svg: string) => [...svg.matchAll(/\bid="([^"]+)"/g)].map((m) => m[1])
  const a = ids(primeiro)
  const b = ids(segundo)
  assert.ok(a.length > 0)
  assert.equal(new Set([...a, ...b]).size, a.length + b.length)
  for (const svg of [primeiro, segundo]) {
    const locais = ids(svg)
    for (const referencia of svg.matchAll(/url\(#([^)]+)\)/g)) {
      assert.ok(locais.includes(referencia[1]), `referência sem definição: ${referencia[1]}`)
    }
  }
})

test('nome e rótulo acessível não injetam marcação no SVG', () => {
  const texto = '<script>alert("x")</script> & \'teste\''
  for (const svg of [
    renderAvatar({ ...CONFIG_BASE, nome: texto }, { size: 24 }),
    renderAvatar(CONFIG_BASE, { size: 128, label: texto, id: '\"><script>' }),
  ]) {
    assert.doesNotMatch(svg, /<script>/)
    assert.match(svg, /&#60;script&#62;/)
    assert.match(svg, /&#38;/)
    assert.match(svg, /&#34;/)
    assert.match(svg, /&#39;/)
  }
})

test('renderizar os cinco estados preserva a configuração dos modelos', () => {
  const antes = structuredClone(MODELOS)
  for (const modelo of MODELOS) {
    for (const estado of ESTADOS) {
      const svg = renderAvatar(modelo.config, { size: 32, state: estado.id })
      assert.ok(svg.includes(`mxq--${estado.id}`))
      assert.ok(svg.includes(modelo.nome))
    }
  }
  assert.deepEqual(MODELOS, antes)
})

test('D528: renderer usa WebP físicos, estados e nenhuma marcação SVG', () => {
  const antes = structuredClone(MODELOS)
  for (const modelo of MODELOS) {
    const conjunto = conjuntoDoModelo(modelo)
    assert.equal(conjunto.id, modelo.categoriaId === 'diretoria' ? modelo.id : modelo.baseId)
    for (const estado of ESTADOS) {
      const html = renderPelucia(conjunto, { size: 32, state: estado.id, label: modelo.nome })
      assert.match(html, /<img /)
      assert.doesNotMatch(html, /<svg|<path|data:image/)
      assert.ok(html.includes(`mxp--${estado.id}`))
      assert.ok(html.includes(modelo.nome))
      assert.match(html, /loading="lazy" decoding="async"/)
      assert.match(html, /\/64\//)
    }
  }
  assert.deepEqual(MODELOS, antes)
  assert.equal(CONJUNTOS.length, 22)
  assert.match(renderPelucia('desconhecido', {size:128}), /assets\/base\/256\/livre-1.webp/)
  // MX-79: degrau intermediário — 64 px até 32 de exibição, 128 até 64, 256 acima.
  for (const [size, pasta] of [[32, '64'], [33, '128'], [64, '128'], [65, '256']] as const)
    assert.match(renderPelucia('base', { size }), new RegExp(`/assets/base/${pasta}/livre-1\\.webp`), `size ${size}`)
})

test('D528: quadros, pausa, caminhos locais e rótulos são seguros', () => {
  assert.equal(QUADROS.length, 9)
  const texto = '<script>alert("x")</script> & \'teste\''
  const html = renderPelucia({id:'id', nome:texto, baseUrl:'/generated/id/'}, {size:128, paused:true})
  assert.doesNotMatch(html, /<script>/)
  assert.match(html, /&#60;script&#62;/)
  assert.match(html, /mxp--pausado/)
  assert.match(html, /src="\/generated\/id\/256\/livre-1.webp"/)
  assert.match(renderPelucia('base', {size:32,assetRoot:'../../apps/assets/'}), /src="..\/..\/apps\/assets\/base\/64/)
  for (const caminho of ['https://evil.test/', '//evil.test/', 'javascript:alert(1)', '/x/" onerror="x']) {
    const out = renderPelucia({id:'base',nome:'Teste',baseUrl:caminho},{size:32,assetRoot:caminho})
    assert.doesNotMatch(out, /evil.test|javascript:|onerror=/)
    assert.match(out, /src=".\/assets\/base\/64/)
  }
  for (const [state, count] of [['livre',3],['trabalhando',2],['dormindo',2],['erro',1],['precisa-resposta',1]] as const) {
    assert.equal([...renderPelucia('base',{size:32,state}).matchAll(/<img /g)].length,count)
  }
})

test('D526: catálogo conserva as 17 categorias, 119 identidades e cinco diretores do dono', () => {
  assert.equal(CATALOGO_FONTE.commit, '221deef8')
  assert.deepEqual(CATEGORIAS.map((c) => c.nome), [
    'Diretoria', 'Produtividade', 'Conteúdo', 'Tech', 'E-commerce', 'Operação',
    'Marketing & Ads', 'Criadores', 'Atendimento & Vendas', 'Finanças & Pessoas',
    'Mercado Financeiro', 'Design & 3D', 'Negócio & Gestão', 'Vida Pessoal',
    'Profissões', 'Comunidade', 'Bem-estar',
  ])
  assert.equal(MODELOS.length, 119)
  assert.equal(new Set(MODELOS.map((m) => m.id)).size, 119)
  assert.equal(new Set(MODELOS.map((m) => m.nome)).size, 119)
  const berga = modeloPorId('berga')!
  assert.equal(berga.papel, 'Orquestrador')
  assert.equal(berga.reportaA, null)
  assert.deepEqual(modelosDaCategoria('diretoria').filter((m) => m.id !== berga.id).map((m) => [m.nome, m.papel, m.reportaA]), [
    ['Pomarca', 'CEO', 'berga'], ['Colheitor', 'COO', 'berga'], ['Tesourango', 'CFO', 'berga'],
    ['Holofruta', 'CMO', 'berga'], ['Chiperica', 'CIO', 'berga'],
  ])
  assert.equal(GRUPOS.find((g) => g.id === 'direto-com-berga')?.nome, 'Direto com o Berga')
  assert.equal(modeloPorId('futurango')?.categoriaId, 'negocio-gestao')
  assert.equal(modeloPorId('maracuja-lunar')?.categoriaId, 'negocio-gestao')
  assert.equal(modeloPorId('batizador'), undefined)
})

test('D526: cada modelo usa peças válidas e uma cadeia de catálogo que chega ao Berga', () => {
  assert.equal(BASES_VISUAIS.length, 15)
  for (const categoria of CATEGORIAS) assert.ok(modelosDaCategoria(categoria.id).length >= 2)
  for (const modelo of MODELOS) {
    assert.ok(CATEGORIAS.some((c) => c.id === modelo.categoriaId), modelo.id)
    assert.ok(GRUPOS.some((g) => g.id === modelo.grupoId), modelo.id)
    assert.ok(baseVisualPorId(modelo.baseId), modelo.id)
    assert.deepEqual(normalizarConfig(modelo.config), modelo.config)
    const vistos = new Set([modelo.id])
    let atual = modelo
    while (atual.reportaA !== null) {
      const pai = modeloPorId(atual.reportaA)
      assert.ok(pai, `referência de catálogo ausente: ${atual.id}`)
      assert.ok(!vistos.has(pai.id), `ciclo no catálogo: ${modelo.id}`)
      vistos.add(pai.id)
      atual = pai
    }
    assert.equal(atual.id, 'berga')
    if (modelo.grupoId === 'direto-com-berga' && modelo.id !== 'berga') assert.equal(modelo.reportaA, 'berga')
  }
  assert.equal(modeloPorId('cfo')?.id, 'tesourango')
  assert.equal(modeloPorId('dev')?.id, 'codaba')
  assert.equal(modeloPorId(' AMAZON ')?.id, 'amazoja')
  assert.equal(modeloPorId('metuva')?.reportaA, 'anuncia')
  assert.equal(modeloPorId('instapera')?.reportaA, 'polpinha')
  assert.equal(modeloPorId('amazoja')?.reportaA, 'feirinha')
})

test('D528: exports distinguem Simples editável e Pelúcia sem misturar configurações', () => {
  assert.equal(renderAvatar, renderSimples)
  const escolhas: EscolhaAvatar[] = [
    { estilo: 'simples', nome: 'Minha simples', aparencia: { ...CONFIG_BASE } },
    { estilo: 'pelucia', nome: 'Minha pelúcia', conjunto: CONJUNTOS[0]! },
  ]
  const antes = structuredClone(escolhas)
  for (const escolha of escolhas) {
    const html = escolha.estilo === 'simples'
      ? renderSimples(escolha.aparencia, { size: 32, label: escolha.nome })
      : renderPelucia(escolha.conjunto, { size: 32, label: escolha.nome })
    assert.match(html, escolha.estilo === 'simples' ? /<svg[^>]+class="mxq / : /<span class="mxp /)
    assert.doesNotMatch(html, escolha.estilo === 'simples' ? /class="mxp/ : /class="mxq/)
  }
  assert.deepEqual(escolhas, antes)
})
