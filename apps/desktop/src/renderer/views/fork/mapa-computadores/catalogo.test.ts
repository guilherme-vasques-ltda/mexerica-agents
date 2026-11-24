import { test } from 'node:test'
import assert from 'node:assert/strict'
import { CATALOGOS } from '../../../lib/fork-i18n/index.ts'
import { IDIOMAS_CATALOGO } from '../../../lib/fork-i18n/registro.ts'

const CHAVES_USADAS = [
  'mapaComputadores.nome.esta', 'mapaComputadores.nome.atlas', 'mapaComputadores.nome.boreal', 'mapaComputadores.nome.cedro', 'mapaComputadores.nome.dunas', 'mapaComputadores.nome.fjord',
  'mapaComputadores.titulo', 'mapaComputadores.aviso', 'mapaComputadores.mapa', 'mapaComputadores.lista', 'mapaComputadores.listaVazia',
  'mapaComputadores.filtro.rotulo', 'mapaComputadores.filtro.todos', 'mapaComputadores.filtro.tunel', 'mapaComputadores.filtro.semAutorizacao',
  'mapaComputadores.papel.esta', 'mapaComputadores.papel.demo', 'mapaComputadores.selecionar', 'mapaComputadores.detalhes',
  'mapaComputadores.fato.rota', 'mapaComputadores.fato.tunel', 'mapaComputadores.fato.identidade', 'mapaComputadores.fato.autorizacao',
  'mapaComputadores.est.rota.configurada', 'mapaComputadores.est.rota.nao_configurada', 'mapaComputadores.est.rota.desconhecida',
  'mapaComputadores.est.tunel.em_uso', 'mapaComputadores.est.tunel.parado', 'mapaComputadores.est.tunel.desconhecido',
  'mapaComputadores.est.identidade.verificada', 'mapaComputadores.est.identidade.negada', 'mapaComputadores.est.identidade.desconhecida',
  'mapaComputadores.est.autorizacao.confirmada', 'mapaComputadores.est.autorizacao.negada', 'mapaComputadores.est.autorizacao.desconhecida',
  'mapaComputadores.amostra.titulo', 'mapaComputadores.amostra.cpu', 'mapaComputadores.amostra.memoria', 'mapaComputadores.amostra.disco',
  'mapaComputadores.amostra.valor', 'mapaComputadores.amostra.obsoleta', 'mapaComputadores.amostra.ausente',
  'mapaComputadores.amostra.invalida', 'mapaComputadores.amostra.futura', 'mapaComputadores.alerta.disco', 'mapaComputadores.semAcoes',
]

test('todo idioma do catálogo tem cada chave usada pela demonstração, com texto não vazio', () => {
  for (const idioma of IDIOMAS_CATALOGO) {
    const catalogo = CATALOGOS[idioma] as Record<string, string>
    for (const chave of CHAVES_USADAS) {
      assert.equal(typeof catalogo[chave], 'string', `${idioma}: ${chave}`)
      assert.ok(catalogo[chave].trim().length > 0, `${idioma}: ${chave} vazia`)
    }
  }
})
