import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { nomeVisivel } from '../../shared/fork-nome-visivel.ts'
import { itensMenuApp, painelSobre } from '../../shared/fork-menu-app.ts'
import { CATALOGOS } from './fork-i18n/index.ts'

// D443 (substitui a guarda da D343/D348): nos textos que a pessoa lê, o app é "Mexerica" ("a Mexerica") e o bot principal é
// "Berga" ("o Berga", masculino desde a D480).
// Esta guarda percorre o renderer, o main e o shared e recusa a marca antiga ("Donna Bot", "DONNA Bot", "Donna Apps") e
// "a Donna" como nome da bot principal, fora de comentários e das exceções técnicas abaixo. No renderer também recusa
// o nome técnico "Donnabot" num texto. Os catálogos dos 6 idiomas não podem ter "Donna" em nenhum valor: desde o
// PR d o macOS mostra o pacote como "Mexerica", e o "Donnabot" entre aspas saiu dos textos.
const src = fileURLToPath(new URL('../../', import.meta.url))

/** Exceções técnicas: o nome que o macOS mostra, o de um pacote ou um código, não um texto da marca. */
const EXCECOES: [RegExp, string][] = [
  [/Donnabot Screen/, 'nome do app auxiliar de tela (DonnabotScreen.app), como aparece nos Ajustes do Sistema'],
  [/Donnabot Dev/, 'nome técnico do build de desenvolvimento (identity.name, pasta de dados e o Donnabot Dev.app de antes do PR d)'],
  [/otpauth:\/\/totp\/Donnabot:|issuer=Donnabot/, 'emissor do código de dois fatores da conta, na demo'],
]
/** Pastas e arquivos fora da guarda, com o motivo. */
const FORA: [string, string][] = [
  ['renderer/lib/donnabot-avatars/', 'biblioteca de avatares, com docs e demo próprios, fora do app (D449a)'],
]
/** Marca antiga e "a Donna" como bot principal (artigo ou preposição do português, "Só a Donna"). */
const MARCA = /Donna Bot|DONNA Bot|Donna Apps|\b(?:[AaOo]|[Dd][ao]|[Nn][ao]|[Pp]el[ao]|à) Donna\b/
/** No renderer, também o nome técnico (sem espaço) num texto. */
const TECNICO = /Donnabot(?![A-Za-z])/

function arquivos(dir: string): string[] {
  return readdirSync(dir).flatMap((nome) => {
    const caminho = join(dir, nome)
    if (statSync(caminho).isDirectory()) return arquivos(caminho)
    return /\.(tsx?|html)$/.test(nome) && !nome.endsWith('.test.ts') ? [caminho] : []
  })
}

/** Tira comentários de bloco e de linha (um `//` depois de `:` é URL e fica). */
const semComentarios = (texto: string) =>
  texto.replace(/\/\*[\s\S]*?\*\//g, '').replace(/<!--[\s\S]*?-->/g, '').replace(/(^|[^:\\])\/\/.*$/gm, '$1')

test('D443: renderer, main e shared não mostram a marca antiga nem "a Donna" como bot principal', () => {
  const achados: string[] = []
  for (const area of ['renderer', 'main', 'shared', 'preload']) {
    for (const arquivo of arquivos(join(src, area))) {
      const rel = relative(src, arquivo)
      if (FORA.some(([pasta]) => rel.startsWith(pasta))) continue
      const proibido = area === 'renderer' ? new RegExp(`${MARCA.source}|${TECNICO.source}`) : MARCA
      semComentarios(readFileSync(arquivo, 'utf8')).split('\n').forEach((linha, i) => {
        if (!proibido.test(linha)) return
        let resto = linha
        for (const [re] of EXCECOES) resto = resto.replace(new RegExp(re.source, 'g'), '')
        if (proibido.test(resto)) achados.push(`${rel}:${i + 1}: ${linha.trim().slice(0, 140)}`)
      })
    }
  }
  assert.deepEqual(achados, [], 'troque por "Mexerica"/"Berga" ou, se for nome técnico, acrescente a exceção com o motivo')
})

/** "Donna" sozinho como literal (fallback de nome, rótulo): só nestes arquivos, com o motivo. */
const DONNA_LITERAL_OK: [string, string][] = [
  ['renderer/demo/fake-armazenamento.ts', 'bot comum chamado Donna na demo: o nome ficou livre para bots novos (D445, item 6)'],
]

test('R1 #552: "Donna" não aparece como literal isolado (fallback ou rótulo do principal) no app', () => {
  const achados: string[] = []
  for (const area of ['renderer', 'main', 'shared', 'preload']) {
    for (const arquivo of arquivos(join(src, area))) {
      const rel = relative(src, arquivo)
      if (FORA.some(([pasta]) => rel.startsWith(pasta)) || DONNA_LITERAL_OK.some(([ok]) => rel === ok)) continue
      semComentarios(readFileSync(arquivo, 'utf8')).split('\n').forEach((linha, i) => {
        if (/(['"`])Donna\1/.test(linha)) achados.push(`${rel}:${i + 1}: ${linha.trim().slice(0, 140)}`)
      })
    }
  }
  assert.deepEqual(achados, [], 'use o nome que o Hub manda ou "Berga" (marca.app.berga); exceção só com motivo')
})

test('D443: nenhum valor dos 6 catálogos diz Donna (as chaves e os códigos donna_* ficam)', () => {
  const achados = Object.entries(CATALOGOS).flatMap(([idioma, catalogo]) =>
    Object.entries(catalogo).filter(([, valor]) => /Donna/.test(String(valor))).map(([chave]) => `${idioma}: ${chave}`))
  assert.deepEqual(achados, [])
})

test('D480: em pt-BR, o Berga é masculino ("o Berga", "do Berga", "ao bot principal"); o app segue "a Mexerica"', () => {
  const feminino = /\b(?:[Aa]|[Dd]a|[Nn]a|[Pp]ela|à|[Ss]ó a) Berga\b|à bot principal|[Aa] bot principal/
  const achados = Object.entries(CATALOGOS['pt-BR']).filter(([, valor]) => feminino.test(String(valor))).map(([chave, valor]) => `${chave}: ${valor}`)
  assert.deepEqual(achados, [])
  assert.match(CATALOGOS['pt-BR']['idioma.passo.subtitulo'], /^O Berga organiza a sua equipe de bots na Mexerica/)
})

test('D443: o nome visível e o menu do Mac dizem Mexerica, com artigo no "Sobre"; o nome técnico de outro app fica', () => {
  assert.equal(nomeVisivel('Donnabot'), 'Mexerica')
  assert.equal(nomeVisivel('Donnabot Dev'), 'Mexerica Dev')
  assert.equal(nomeVisivel('Donnabot Dev (feat-x)'), 'Mexerica Dev (feat-x)')
  assert.equal(nomeVisivel('Codync'), 'Codync')
  assert.equal(nomeVisivel('DonnabotScreen'), 'DonnabotScreen')
  const rotulos = itensMenuApp('Donnabot', () => {}, 'pt-BR').flatMap((i) => ('label' in i && i.label ? [i.label] : []))
  assert.deepEqual(rotulos, ['Sobre a Mexerica', 'Ajustes…', 'Ocultar Mexerica', 'Encerrar Mexerica'])
  // D445c: o painel Sobre diz "Mexerica" com a linha pequena "baseado no Codync".
  assert.deepEqual(painelSobre('Donnabot', 'pt-BR'), { applicationName: 'Mexerica', credits: 'baseado no Codync' })
  assert.deepEqual(painelSobre('Donnabot Dev', 'pt-BR'), { applicationName: 'Mexerica Dev', credits: 'baseado no Codync' })
  // D454/D495: sem idioma escolhido, o padrão inglês.
  assert.deepEqual(painelSobre('Donnabot'), { applicationName: 'Mexerica', credits: 'based on Codync' })
})
