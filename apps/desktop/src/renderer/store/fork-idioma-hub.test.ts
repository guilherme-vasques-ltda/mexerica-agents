// D395: o idioma vai ao Hub ao conectar e a cada troca, só por clientes registrados; falha do Hub não aparece.
import assert from 'node:assert/strict'
import test from 'node:test'
import { acompanharIdioma, enviarIdioma, registrarIdioma, type ClienteIdioma } from './fork-idioma-hub.ts'

function cliente(falha = false) {
  const chamadas: Array<[string, Record<string, unknown>]> = []
  const c: ClienteIdioma = {
    call<T>(method: string, body: Record<string, unknown>) {
      chamadas.push([method, body])
      return (falha ? Promise.reject(new Error('unknown method')) : Promise.resolve({})) as Promise<T>
    },
  }
  return { c, chamadas }
}

test('envia o idioma atual ao conectar e a cada troca; sem fonte ou cliente, nada', async () => {
  const a = cliente()
  enviarIdioma(a.c)
  assert.deepEqual(a.chamadas, [], 'sem fonte registrada, nada vai')
  let idioma = 'pt-BR'
  let aviso = () => {}
  const parar = registrarIdioma(() => idioma, (f) => { aviso = f; return () => { aviso = () => {} } })
  enviarIdioma(a.c)
  enviarIdioma(null)
  assert.deepEqual(a.chamadas, [['donnaAppLanguageSave', { language: 'pt-BR' }]])
  const b = cliente(true)
  const sair = acompanharIdioma(() => b.c)
  idioma = 'ar-AE'
  aviso()
  assert.deepEqual(b.chamadas, [['donnaAppLanguageSave', { language: 'ar-AE' }]])
  await Promise.resolve()
  sair()
  aviso()
  assert.equal(b.chamadas.length, 1, 'quem saiu não recebe mais')
  parar()
})
