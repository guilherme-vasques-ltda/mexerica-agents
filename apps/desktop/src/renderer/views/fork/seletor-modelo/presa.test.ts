import assert from 'node:assert/strict'
import { test } from 'node:test'
import { posicaoLista } from './posicao.ts'
import { assinarReposicao, mesmaPosicao, noBody, type Ambiente } from './presa.ts'

// D473 R2 (#548). Sem layout real no Node: os rects são simulados. Estes testes provam a lógica (portal no <body>,
// religar e reposicionar enquanto aberto, desligar sem vazar); não provam a pintura nem o backdrop-filter do Chromium,
// conferidos na demo (ensaio 850 → 1100 px, prints no PR).

/** Um ambiente falso: ResizeObserver e janela que registram tudo e disparam sob comando. */
function ambienteFalso() {
  const observados = new Set<object>()
  let aviso: (() => void) | null = null
  const ouvintes = new Map<string, () => void>()
  const quadros = new Map<number, () => void>()
  let id = 0
  const amb: Ambiente = {
    observador: (f) => { aviso = f; return { observe: (e) => void observados.add(e), disconnect: () => { observados.clear(); aviso = null } } },
    janela: {
      addEventListener: (t, f, c) => void ouvintes.set(`${t}:${!!c}`, f),
      removeEventListener: (t, f, c) => { if (ouvintes.get(`${t}:${!!c}`) === f) ouvintes.delete(`${t}:${!!c}`) },
      requestAnimationFrame: (f) => { quadros.set(++id, f); return id },
      cancelAnimationFrame: (n) => void quadros.delete(n),
    },
  }
  const rodarQuadros = () => { const fs = [...quadros.values()]; quadros.clear(); fs.forEach((f) => f()) }
  return { amb, observados, ouvintes, quadros, redimensionou: () => aviso?.(), evento: (t: string, c = false) => ouvintes.get(`${t}:${c}`)?.(), rodarQuadros }
}

test('R2a: o painel é montado no <body>, fora do vidro do campo (backdrop-filter), e não no lugar do chip', () => {
  const body = { nome: 'body' }
  let destino: unknown = null
  const portal = ((no: unknown, alvo: unknown) => { destino = alvo; return no }) as unknown as Parameters<typeof noBody>[1]
  noBody('painel', portal, { body } as unknown as Pick<Document, 'body'>)
  // Sem o portal (render no lugar), nada chega ao body e este teste falha.
  assert.equal(destino, body)
})

test('R2b: aberto, redimensionar move o chip e o painel volta a ficar preso a ele (850 → 1100 px, barra lateral)', () => {
  const f = ambienteFalso()
  const chip = { rect: { top: 760, bottom: 780, left: 104 } }
  const coluna = {}, raiz = {}
  let janela = { largura: 850, altura: 800 }
  let posicao = posicaoLista(chip.rect, janela)
  const desligar = assinarReposicao(chip as unknown as Element, [coluna, raiz] as unknown as Element[], () => { posicao = posicaoLista(chip.rect, janela) }, f.amb)
  assert.ok(f.observados.has(chip) && f.observados.has(coluna) && f.observados.has(raiz), 'observa o chip e os contêineres da barra lateral')
  assert.equal(posicao.left, 104)

  // A janela cresce e a barra lateral abre: o chip vai para x=390 depois do layout.
  janela = { largura: 1100, altura: 800 }
  f.evento('resize')
  chip.rect = { top: 760, bottom: 780, left: 390 }
  f.redimensionou()
  assert.equal(posicao.left, 104, 'mede só no quadro seguinte, com o layout assentado')
  f.rodarQuadros()
  assert.equal(posicao.left, 390, 'preso ao chip')

  // Scroll de um ancestral (captura) também reposiciona.
  chip.rect = { top: 700, bottom: 720, left: 390 }
  f.evento('scroll', true)
  f.rodarQuadros()
  assert.ok('bottom' in posicao && posicao.bottom === 800 - 700 + 6)

  // Fechou: nada continua ouvindo nem agendado.
  f.evento('resize')
  desligar()
  assert.equal(f.observados.size, 0)
  assert.equal(f.ouvintes.size, 0)
  assert.equal(f.quadros.size, 0)
})

test('Kodus: medida igual não troca o estado (rolar sem mover o chip não renderiza o seletor de novo)', () => {
  const a = posicaoLista({ top: 760, bottom: 780, left: 390 }, { largura: 1100, altura: 800 })
  assert.equal(mesmaPosicao(a, posicaoLista({ top: 760, bottom: 780, left: 390 }, { largura: 1100, altura: 800 })), true)
  assert.equal(mesmaPosicao(a, posicaoLista({ top: 760, bottom: 780, left: 391 }, { largura: 1100, altura: 800 })), false)
  assert.equal(mesmaPosicao(a, posicaoLista({ top: 40, bottom: 60, left: 390 }, { largura: 1100, altura: 800 })), false, 'virar de lado conta')
  assert.equal(mesmaPosicao(null, a), false)
  assert.equal(mesmaPosicao({ overflowY: 'auto', left: 1 }, { overflowY: 'auto', left: 1 }), true, 'o estilo do menu da mãozinha')
})
