// SPDX-License-Identifier: Apache-2.0
import assert from 'node:assert/strict'
import test from 'node:test'
import { DEFAULT_CONFIG, confirmAvatar, normalizedAvatar, validAvatar } from './fork-avatar-config.ts'
import { schemaMexeriquinhaSuportado, avatarDoBot, camposAvatarDoBot, avatarMexeriquinhaHubNotice, equivalenciaAvatar, normalizarAvatarDoBot, validBotAvatar, type MexeriquinhaConfig } from './fork-mexeriquinha-config.ts'
import schema from './fork-mexeriquinha.schema.json' with { type: 'json' }
import data from './fork-mexeriquinha-data.json' with { type: 'json' }
import { MODELOS } from '../renderer/lib/mexeriquinhas/modelos.ts'
import { BASES_VISUAIS } from '../renderer/lib/mexeriquinhas/bases.ts'
import { CONJUNTOS } from '../renderer/lib/mexeriquinhas/conjuntos.ts'
import { CORES, OLHOS, ROUPAS, ACESSORIOS, OBJETOS } from '../renderer/lib/mexeriquinhas/pecas.ts'
import { draftOf, normalizeBot } from './models.ts'

const simples: MexeriquinhaConfig = { v:2, estilo:'simples', modeloId:'berga', aparencia:{cor:'laranja',olhos:'ponto',roupa:'blazer',acessorio:'coroa',objeto:'gravata'} }
const pelucia: MexeriquinhaConfig = { v:2, estilo:'3d', modeloId:'codaba', conjuntoId:'developer' }

test('contrato compartilhado corresponde aos 119 modelos,22 conjuntos e partes aprovadas', () => {
  assert.deepEqual(Object.keys(data.modelos), MODELOS.map(m=>m.id))
  assert.deepEqual(Object.keys(data.conjuntos), CONJUNTOS.map(c=>c.id))
  for (const base of BASES_VISUAIS) {
    const appearance=Object.fromEntries(Object.keys(data.padrao).map((key)=>[key,base.config[key as keyof typeof base.config]??'nenhum']))
    assert.deepEqual(data.conjuntos[base.id],appearance)
  }
  const parts = schema.oneOf[0]!.properties.aparencia!.properties
  for (const [key, values] of Object.entries({cor:CORES,olhos:OLHOS,roupa:ROUPAS,acessorio:ACESSORIOS,objeto:OBJETOS})) {
    assert.deepEqual(parts[key as keyof typeof parts].enum,values.map(x=>x.id))
  }
})

test('v2 fechado: aceita somente IDs reais, rejeita injeção e payloads fora do limite', () => {
  for (const value of [DEFAULT_CONFIG,simples,pelucia]) assert.equal(validBotAvatar(value),true)
  const missing = structuredClone(simples) as unknown as {aparencia:Record<string,unknown>}
  delete missing.aparencia.objeto
  for (const value of [null,[],{}, { ...simples,v:3 }, {...simples,estilo:'pelucia'}, {...simples,modeloId:'desconhecido'}, {...simples,html:'<svg/>'}, {...simples,aparencia:{...simples.aparencia,nome:'extra'}}, {...simples,aparencia:{...simples.aparencia,cor:'red'}}, missing, {...pelucia,conjuntoId:'https://evil.test/image.svg'}, {...pelucia,conjuntoId:'base/../../secret'}, {...pelucia,conjuntoId:'data:image/png;base64,AAAA'}, {...pelucia,conjuntoId:'a'.repeat(1025)}, {...pelucia,aparencia:simples.aparencia}]) {
    assert.equal(validBotAvatar(value),false,JSON.stringify(value))
  }
})

test('migração determinística de legado mantém aproximação e preserva v2 explícito', () => {
  const old = {...DEFAULT_CONFIG,color:'#f28a1e',eye:'round',head:'headset',badge:'code'} as const
  assert.deepEqual(normalizarAvatarDoBot(old), {v:2,estilo:'simples',aparencia:{cor:'laranja',olhos:'brilhantes',roupa:'moletom',acessorio:'fone',objeto:'notebook'}})
  for (const value of [simples,pelucia]) {
    assert.deepEqual(normalizarAvatarDoBot(value,'outro','outra função'),value)
    assert.notEqual(normalizarAvatarDoBot(value),value)
    assert.equal(validAvatar(normalizedAvatar(value)),true,'API pública segue recebendo apenas schema legado')
  }
  assert.equal(validBotAvatar(normalizarAvatarDoBot(undefined,'Amazon')),true)
})

test('normalização wire e draft conservam união; confirmação compara aparência profundamente', () => {
  for (const value of [simples,pelucia,DEFAULT_CONFIG]) {
    const bot=normalizeBot(JSON.parse(JSON.stringify({id:'bot',name:'Bot',...camposAvatarDoBot(value)})))
    assert.deepEqual(avatarDoBot(bot),value)
    assert.deepEqual(avatarDoBot(draftOf(bot)),value)
    const reordered=JSON.parse(JSON.stringify(value,Object.keys(value).reverse()))
    if ('aparencia' in value) reordered.aparencia={...value.aparencia}
    assert.equal(equivalenciaAvatar(value,reordered),true)
    assert.doesNotThrow(()=>confirmAvatar(reordered,value))
  }
  const changed={...simples,aparencia:{...simples.aparencia,olhos:'brilhantes'}}
  assert.equal(equivalenciaAvatar(simples,changed),false)
  assert.throws(()=>confirmAvatar(changed,simples))
  assert.throws(()=>confirmAvatar(DEFAULT_CONFIG,simples))
})

test('editor Mexeriquinhas exige capability v2, independente de avatarEditing', () => {
  assert.match(avatarMexeriquinhaHubNotice(null)!,/Conectando/)
  assert.ok(avatarMexeriquinhaHubNotice({}))
  assert.ok(avatarMexeriquinhaHubNotice({avatarEditing:true}))
  assert.ok(avatarMexeriquinhaHubNotice({avatarConfigVersion:1}))
  assert.equal(avatarMexeriquinhaHubNotice({avatarConfigVersion:2}),null)
})


test('empate de paleta é resolvido por ID ordinal, igual ao Hub', () => {
  const converted=normalizarAvatarDoBot({...DEFAULT_CONFIG,color:'#0003cc'})
  assert.equal(converted.estilo,'simples')
  if (converted.estilo==='simples') assert.equal(converted.aparencia.cor,'celeste')
})


test('fronteira pública mantém avatarConfig legado e recusa conflito sem ocultar a escolha antiga', () => {
  const fields=camposAvatarDoBot(simples)
  assert.equal(validAvatar(fields.avatarConfig),true)
  assert.deepEqual(fields.mexeriquinha,simples)
  assert.deepEqual(avatarDoBot(fields),simples)
  const conflicting={...fields,avatarConfig:{...fields.avatarConfig,color:'#000000'}}
  assert.deepEqual(avatarDoBot(conflicting),conflicting.avatarConfig)
  assert.deepEqual(normalizeBot({id:'conflict',...conflicting}).mexeriquinha,undefined)
  assert.deepEqual(camposAvatarDoBot(DEFAULT_CONFIG),{avatarConfig:DEFAULT_CONFIG})
})

test('baseline literal imutável da projeção pública: seis cores, sete olhos e 22 conjuntos', () => {
  const expected=(color:string,eye:string,face:string)=>({shape:'oval',color,eye,head:'none',face,ears:'none',chin:'none',neck:'none',cheeks:'none',badge:'none',badgeStyle:'light'})
  for (const [cor,color] of [['laranja','#f28a1e'],['lima','#94c630'],['limao','#f2c41e'],['toranja','#ec5a5e'],['roxo','#8d56d8'],['celeste','#2f9cf0']] as const) {
    assert.deepEqual(camposAvatarDoBot({v:2,estilo:'simples',aparencia:{cor,olhos:'ponto',roupa:'nenhuma',acessorio:'nenhum',objeto:'nenhum'}}).avatarConfig,expected(color,'auto','none'))
  }
  for (const [olhos,eye,face] of [['ponto','auto','none'],['sonolentos','bar','none'],['brilhantes','round','none'],['oculos-redondos','auto','glasses'],['oculos-escuros','auto','shades'],['piscando','cat','none'],['oculos-verdes','auto','shades']] as const) {
    assert.deepEqual(camposAvatarDoBot({v:2,estilo:'simples',aparencia:{cor:'laranja',olhos,roupa:'nenhuma',acessorio:'nenhum',objeto:'nenhum'}}).avatarConfig,expected('#f28a1e',eye,face))
  }
  const sets=[
    ['base','#f28a1e','auto','none'],['ceo','#f28a1e','auto','none'],['assistente','#f28a1e','auto','none'],
    ['financeiro','#94c630','auto','glasses'],['marketing','#ec5a5e','auto','shades'],['developer','#8d56d8','auto','none'],
    ['pesquisa','#f2c41e','auto','none'],['vendas','#2f9cf0','cat','none'],['compras','#94c630','auto','none'],
    ['rh','#ec5a5e','round','none'],['suporte','#f2c41e','auto','none'],['designer','#8d56d8','auto','none'],
    ['amazon','#2f9cf0','auto','none'],['dono','#f28a1e','auto','none'],['chefe','#ec5a5e','auto','none'],
    ['hacker','#f28a1e','auto','shades'],['berga','#f28a1e','auto','none'],['pomarca','#f28a1e','auto','none'],
    ['colheitor','#94c630','auto','none'],['tesourango','#f2c41e','auto','glasses'],['holofruta','#ec5a5e','auto','shades'],['chiperica','#2f9cf0','auto','none']
  ] as const
  assert.equal(sets.length,22)
  for (const [conjuntoId,color,eye,face] of sets) assert.deepEqual(camposAvatarDoBot({v:2,estilo:'3d',conjuntoId}).avatarConfig,expected(color,eye,face),conjuntoId)
})

test('versão numérica 2.0 e 2e0 é semanticamente 2; futuro e string continuam recusados', () => {
  for (const version of ['2','2.0','2e0']) {
    const value:unknown=JSON.parse(`{"v":${version},"estilo":"3d","conjuntoId":"base"}`)
    assert.equal(validBotAvatar(value),true)
    assert.equal(JSON.stringify(normalizarAvatarDoBot(value)),JSON.stringify({v:2,estilo:'3d',conjuntoId:'base'}))
  }
  for (const version of ['2.01','3','"2"']) assert.equal(validBotAvatar(JSON.parse(`{"v":${version},"estilo":"3d","conjuntoId":"base"}`)),false)
})

test('o matcher aceita somente seu subconjunto fechado de schema e discriminantes disjuntos', () => {
  assert.equal(schemaMexeriquinhaSuportado(schema),true)
  const mutations: [string[], unknown][] = [
    [['unknown'],true],[['oneOf','0','minProperties'],1],[['oneOf','0','additionalProperties'],true],
    [['oneOf','0','type'],'array'],[['oneOf','0','properties','modeloId','oneOf'],[{const:'berga'}]],
    [['oneOf','0','properties','modeloId','description'],'metadata aninhada não suportada'],[['oneOf','1','properties','estilo','const'],'simples'],
  ]
  for (const [keys,value] of mutations) {
    const changed=structuredClone(schema)
    let target=changed as unknown as Record<string,unknown>
    for (const key of keys.slice(0,-1)) target=target[key] as Record<string,unknown>
    target[keys.at(-1)!]=value
    assert.equal(schemaMexeriquinhaSuportado(changed),false)
  }
  assert.equal(schemaMexeriquinhaSuportado({...schema,description:'metadata só da raiz'}),true)
})
