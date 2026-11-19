import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { EstadoPaginas, useEstadoDaPagina, type EstadoGuardado } from '../views/settings/estado-pagina.ts'

// Rascunhos e escolhas de Ajustes sobrevivem à troca de página (estado-pagina.ts). O ciclo completo (digitar, trocar,
// voltar) é o teste ajustes-rascunhos.spec.ts no build de demonstração; aqui: a restauração pelo hook e o uso nas páginas.

function Sonda({ chave, inicial }: { chave: string; inicial: string }) {
  const [valor] = useEstadoDaPagina(chave, inicial)
  return createElement('span', null, valor)
}
const renderiza = (guardado: EstadoGuardado | null, chave: string, inicial: string) =>
  renderToStaticMarkup(createElement(EstadoPaginas.Provider, { value: guardado }, createElement(Sonda, { chave, inicial })))

test('fora de Ajustes (sem o provedor) o hook é um useState: começa do valor inicial', () => {
  assert.equal(renderiza(null, 'k', 'inicial'), '<span>inicial</span>')
})

test('ao voltar à página, o valor guardado no pai vence o inicial', () => {
  const guardado: EstadoGuardado = new Map([['k', 'digitado antes']])
  assert.equal(renderiza(guardado, 'k', 'inicial'), '<span>digitado antes</span>')
})

test('chaves diferentes não se misturam', () => {
  const guardado: EstadoGuardado = new Map([['outra', 'x']])
  assert.equal(renderiza(guardado, 'k', 'inicial'), '<span>inicial</span>')
})

const fonte = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8')

test('o valor editado é gravado no mapa do pai a cada mudança', () => {
  assert.match(fonte('../views/settings/estado-pagina.ts'), /useEffect\(\(\) => \{ guardado\?\.set\(atual\.chave, atual\.valor\) \}, \[guardado, atual\]\)/)
})

test('se a chave muda com o componente vivo, o hook devolve o estado da chave nova (não o da anterior)', () => {
  const hook = fonte('../views/settings/estado-pagina.ts')
  assert.match(hook, /if \(estado\.chave !== chave\) \{\n\s+atual = abrirEstado\(guardado, chave, inicial\)\n\s+setEstado\(atual\)\n\s+\}/)
})

test('o provedor do mapa fica em SettingsView, acima de todas as páginas', () => {
  const view = fonte('../views/settings/SettingsView.tsx')
  assert.match(view, /const \[guardado\] = useState<EstadoGuardado>\(\(\) => new Map\(\)\)/)
  assert.match(view, /<EstadoPaginas\.Provider value=\{guardado\}>/)
})

test('cada página com rascunho ou escolha local guarda o estado no pai', () => {
  const paginas: Array<[string, string[]]> = [
    ['../views/fork/delegacao/CampoRegras.tsx', ['delegacao/regras/']],
    ['../views/fork/memoria-ajustes/MemoriaPage.tsx', ['memoria/sobre-voce/', "'memoria/computador'"]],
    ['../views/fork/nunca-fazer/NuncaFazer.tsx', ['nunca-fazer/${guardarComo}/novo', 'nunca-fazer/${guardarComo}/rascunhos', "'nunca-fazer/computador'", 'guardarComo={`${store.computer.id}/${model.escopo.spaceId}`}']],
    ['../views/fork/onde-roda/LugaresPage.tsx', ['chaveSelecionado(hubId)', 'chaveRascunho(hubId, place.id)']],
    ['../views/fork/onde-roda/CadastrarLugar.tsx', ["chaveCadastro(hubId, 'nome')", "chaveCadastro(hubId, 'recurso')"]],
    ['../views/fork/bot-de-fundo/BotDeFundoPage.tsx', ["'bot-de-fundo/computador'", 'bot-de-fundo/${host.computer.id}/${spaceId', '/preparo`']],
    ['../views/fork/atividade/AtividadePage.tsx', ["'atividade/computador'", "'atividade/filtro'", 'atividade/${host.computer.id}/aberta', 'atividade/${host.computer.id}/pagina']],
    ['../views/fork/custo/CustoUso.tsx', ['uso/${store.computer.id}/mes']],
  ]
  for (const [rel, chaves] of paginas) {
    const src = fonte(rel)
    assert.match(src, /useEstadoDaPagina[<(]/, `${rel} usa o estado guardado`)
    for (const chave of chaves) assert.ok(src.includes(chave), `${rel} guarda ${chave}`)
  }
})

test('a Delegação guarda as regras por Hub, época e computador (a chave muda quando o Hub muda)', () => {
  assert.ok(fonte('../views/fork/delegacao/DelegacaoPage.tsx').includes("guardarComo={`${store.computer.id}|${store.hello?.hostId ?? ''}|${store.principalEpoch}`}"))
})

test('a Memória guarda o rascunho pela identidade do Hub (computador, Hub e dono)', () => {
  assert.match(fonte('../views/fork/memoria-ajustes/MemoriaPage.tsx'), /useEstadoDaPagina\(`memoria\/sobre-voce\/\$\{chave\}`/)
})

test('com rascunho sujo do modelo de regras, a releitura do Hub não apaga o texto digitado', () => {
  assert.match(fonte('../views/fork/delegacao/CampoRegras.tsx'), /void modelo\.ler\(modelo\.salvo !== null && modelo\.sujo\)/)
})
