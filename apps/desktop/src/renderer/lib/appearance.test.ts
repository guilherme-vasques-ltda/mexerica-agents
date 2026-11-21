import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'
import { Intents } from './intents.ts'
import * as legibility from './legibility.ts'
import * as sidebarLayout from './sidebar-layout.ts'
import * as transparencyAreas from './transparency-areas.ts'
import { ptBR } from './fork-i18n/pt-BR.ts'
import { CATALOGOS, traduzir } from './fork-i18n/index.ts'
import * as appearanceChoices from './appearance-choices.ts'
import * as toquesHover from './toques-hover.ts'

const designTokensFile = readFileSync(new URL('../../../../../apps/desktop/design/tokens.json', import.meta.url))
const designTokens = JSON.parse(designTokensFile.toString()).color.tokens as { name: string; value: Record<string, string> }[]
const abstractMeasurements = JSON.parse(readFileSync(new URL('../../../../../apps/desktop/design/fundos/medidas.json', import.meta.url), 'utf8')) as {
  tokensSha256: string
  images: { id: string; dark: boolean; file: string; width: number; height: number; bytes: number; sha256: string; bounds: legibility.Bounds; mean?: legibility.RGB; floor: number; primaryFloor: number }[]
}
// PR g: os fundos da Mexerica (WebP, 1672 × 941), medidos com a mesma receita e os mesmos tokens.
const mexericaMeasurements = JSON.parse(readFileSync(new URL('../../../../../apps/desktop/design/fundos-mexerica/medidas.json', import.meta.url), 'utf8')) as typeof abstractMeasurements
const measurements = { ...abstractMeasurements, images: [...mexericaMeasurements.images, ...abstractMeasurements.images] }
const mexericaIds = ['aurora-suave', 'janela-da-cidade', 'jardim-citrico', 'ondas-pastel', 'montanhas-suaves', 'linho-fino', 'folhas-botanicas', 'nuvens-suaves', 'canto-aconchegante', 'vidro-citrico'] as const
const abstractIds = ['cafeteria', 'cafe', 'campo', 'lago', 'skyline', 'escritorio', 'ondas', 'circulos', 'blocos', 'jardim'] as const
const newIds = [...mexericaIds, ...abstractIds] as const
/** DON-909: IDs estáveis dos fundos em código (prints, perf e preferência salva dependem deles). */
const codeIds = ['aurora', 'carbono', 'geada', 'nascer-do-sol', 'donna-petroleo'] as const
const tokenValue = (name: string, dark: boolean) => designTokens.find((t) => t.name === name)?.value[dark ? 'dark' : 'light']!
const tokenRGB = (name: string, dark: boolean): legibility.RGB => {
  const value = tokenValue(name, dark)
  return (value.startsWith('#') ? [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16)) : value.match(/[\d.]+/g)!.slice(0, 3).map(Number)) as unknown as legibility.RGB
}

// D165 F5-R2/F6: execute the real appearance/prefs modules with synthetic browser boundaries.
// Native decode/IndexedDB and the actual React controls are also exercised by the PR's browser probes.
function fixture(initial: Record<string, string> = {}, opts: { realTokens?: boolean; pixels?: boolean; reduceTransparency?: boolean; moreContrast?: boolean; listaTokens?: boolean } = {}) {
  let leiturasDeEstilo = 0
  const react = { useSyncExternalStore: (_subscribe: unknown, get: () => unknown) => get() }
  const imageStore = new Map<string, Blob>()
  let decodedHeic = false
  let releaseDecode = () => {}
  let enteredDecode = () => {}
  const entered = new Promise<void>((resolve) => { enteredDecode = resolve })
  const gate = new Promise<void>((resolve) => { releaseDecode = resolve })
  const storage = new Map<string, string>(Object.entries(initial))
  const windowEvents = new Map<string, ((event: unknown) => void)[]>()
  // The variables apply() writes on <html>, for the Transparência por área case (D169).
  const vars = new Map<string, string>()
  const style = { setProperty: (name: string, value: string) => { vars.set(name, value) }, removeProperty: (name: string) => { vars.delete(name) } }
  const context = {
    Blob, console,
    window: { addEventListener(name: string, listener: (event: unknown) => void) { windowEvents.set(name, [...(windowEvents.get(name) ?? []), listener]) }, matchMedia: (query: string) => ({ matches: query.includes('reduced-transparency') ? !!opts.reduceTransparency : query.includes('contrast') ? !!opts.moreContrast : false, addEventListener() {} }) },
    localStorage: { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => storage.set(k, v) },
    getComputedStyle: (el: { dataset: { theme?: string } }) => {
      leiturasDeEstilo++
      const getPropertyValue = (name: string) => opts.realTokens ? tokenValue(name.slice(2), el.dataset.theme === 'dark') ?? '' : '#fbfaf7'
      // Como no Chromium, o estilo computado lista as variáveis (MX-93: o retrato de cada tema).
      return opts.listaTokens ? Object.assign(designTokens.map((t) => `--${t.name}`), { getPropertyValue }) : { getPropertyValue }
    },
    URL: { createObjectURL: () => `blob:synthetic-${++urlId}`, revokeObjectURL() {} },
    Image: class {
      src = ''
      naturalWidth = 1920
      naturalHeight = 1200
      async decode() { if (!opts.pixels) throw new Error('Sem pixels nesta fixture') }
    },
    createImageBitmap: async (file: File) => {
      if (file.name === 'pending.png') { enteredDecode(); await gate }
      if (file.name === 'invalid.png') throw new Error('The source image could not be decoded.')
      if (/\.heic$/.test(file.name)) { decodedHeic = true; throw new Error('The source image could not be decoded.') }
      return { width: 64, height: 64, file, close() {} }
    },
    document: {
      documentElement: { dataset: {}, style, append() {} }, addEventListener() {},
      createElement(tag: string) {
        let image: Blob
        let source = ''
        return tag === 'canvas' ? {
          getContext: () => ({
            drawImage: (b: { file?: Blob; src?: string }) => { if (b.file) image = b.file; source = b.src ?? '' },
            // A fronteira entrega os extremos medidos nos JPGs; o algoritmo real lê os dois pixels.
            getImageData: () => {
              const bounds = measurements.images.find((m) => source.endsWith(`/${m.file}`))!.bounds
              return { data: [...bounds.lo, 255, ...bounds.hi, 255] }
            },
          }),
          toBlob: (done: (b: Blob) => void, type: string) => setImmediate(() => done(new Blob([image], { type }))),
        } : { dataset: {}, setAttribute() {} }
      },
    },
    indexedDB: {
      open() {
        const open: any = {}
        // Opening is asynchronous, as in IDB; Remover→invalid is dispatched before this returns.
        setImmediate(() => {
          open.result = {
            close() {},
            transaction(_name: string, mode: string) {
              const ops: (() => void)[] = []
              const tx: any = { objectStore: () => ({
                put: (blob: Blob, key: string) => { ops.push(() => imageStore.set(key, blob)) },
                delete: (key: string) => { ops.push(() => imageStore.delete(key)) },
                get: (key: string) => {
                  const req: any = {}
                  setImmediate(() => { req.result = imageStore.get(key); req.onsuccess() })
                  return req
                },
              }) }
              if (mode === 'readwrite') setImmediate(() => { ops.forEach((op) => op()); tx.oncomplete() })
              return tx
            },
          }
          open.onsuccess()
        })
        return open
      },
    },
  }
  let urlId = 0
  function load(file: string, dependencies: Record<string, unknown>) {
    const exports: any = {}
    const source = readFileSync(new URL(file, import.meta.url), 'utf8')
    const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText
    runInNewContext(code, { ...context, exports, require: (id: string) => {
      // PR g: os fundos em JPEG ou WebP; o módulo recebe a URL do arquivo.
      if (id.endsWith('.jpg') || id.endsWith('.webp')) return id
      assert.ok(id in dependencies, `dependência declarada: ${id}`)
      return dependencies[id]
    } })
    return exports
  }
  const prefs = load('./prefs.ts', { react, './sidebar-layout': sidebarLayout })
  const touches = load('./toques-mexerica.ts', { react, './prefs': prefs })
  const codeWallpapers = load('./fork-wallpapers-code.ts', { './fork-i18n/pt-BR.ts': { ptBR } })
  const a = load('./appearance.ts', { react, './toques-mexerica': touches, './toques-hover': toquesHover, './fork-wallpapers-code': codeWallpapers, './prefs': prefs, './legibility': legibility, './intents': { Intents }, './transparency-areas': transparencyAreas, './appearance-choices': appearanceChoices, './fork-i18n/pt-BR.ts': { ptBR } }) as typeof import('./appearance')
  a.startAppearance()
  const image = (name = 'valid.png') => new File([name], name, { type: 'image/png' })
  const settle = async () => { for (let i = 0; i < 12; i++) await new Promise<void>((r) => setImmediate(r)) }
  return { a, image, entered, releaseDecode, settle, dataset: context.document.documentElement.dataset as Record<string, string>, storageEvent: (event: unknown) => windowEvents.get('storage')?.forEach(fn => fn(event)), stored: () => imageStore.get('custom') ?? null, vars, storage, decodedHeic: () => decodedHeic, leiturasDeEstilo: () => leiturasDeEstilo }
}

test('instalação nova começa em Aurora suave sem gravar preferência', () => {
  const f = fixture()
  assert.equal(f.a.DEFAULT_WALLPAPER, 'aurora-suave')
  assert.equal(f.a.appearancePrefs.wallpaper.get(), 'aurora-suave')
  assert.ok(f.vars.get('--wp-image')!.endsWith('/aurora-suave.webp")'))
  assert.equal(f.storage.has('appearanceWallpaper'), false)
})

test('preferência vazia usa Aurora suave e mantém o armazenamento intacto', () => {
  for (const raw of ['', '""', 'null', '"   "']) {
    const f = fixture({ appearanceWallpaper: raw })
    assert.equal(f.a.appearancePrefs.wallpaper.get(), 'aurora-suave')
    assert.equal(f.storage.get('appearanceWallpaper'), raw)
  }
})

test('MX-143: fundo donna-* salvo cai no padrão existente, sem erro e sem regravar a preferência', () => {
  for (const id of ['donna-azul', 'donna-vinho', 'donna-cafe', 'donna-vinil', 'donna-gravata']) {
    const raw = JSON.stringify(id)
    const f = fixture({ appearanceWallpaper: raw })
    assert.equal(f.a.appearancePrefs.wallpaper.get(), f.a.DEFAULT_WALLPAPER, id)
    assert.equal(f.storage.get('appearanceWallpaper'), raw, `${id}: a preferência não é reescrita`)
    assert.ok(f.vars.get('--wp-image')!.endsWith(`/${f.a.DEFAULT_WALLPAPER}.webp")`), id)
    assert.equal(f.a.HOUSE_WALLPAPERS.some((w) => (w.id as string) === id), false, `${id}: fora do catálogo`)
  }
})

test('escolhas salvas, inclusive Nenhum e imagem própria, sobrevivem ao novo padrão', () => {
  for (const id of ['none', 'serra', 'noite', 'custom', 'aurora-suave'] as const) {
    const raw = JSON.stringify(id)
    const f = fixture({ appearanceWallpaper: raw })
    assert.equal(f.a.appearancePrefs.wallpaper.get(), id)
    assert.equal(f.storage.get('appearanceWallpaper'), raw)
    if (id === 'none') assert.equal(f.vars.has('--wp-image'), false)
  }
})

// MX-93: tokens.css é estático; reler a sonda entre as escritas no <html> forçava recalcular o documento inteiro.
test('MX-93: os tokens de cada tema são lidos numa leitura de estilo só, e com os mesmos valores', () => {
  for (const dark of [false, true]) {
    const lista = fixture({ appearanceTheme: JSON.stringify(dark ? 'dark' : 'light') }, { realTokens: true, listaTokens: true })
    const direta = fixture({ appearanceTheme: JSON.stringify(dark ? 'dark' : 'light') }, { realTokens: true })
    lista.a.apply(); lista.a.apply()
    direta.a.apply(); direta.a.apply()
    assert.equal(lista.leiturasDeEstilo(), 1)
    assert.ok(direta.leiturasDeEstilo() > 10, 'sem a lista, cai na leitura direta por token')
    assert.deepEqual([...lista.vars], [...direta.vars])
  }
})

// D529: só a ausência de escolha usa Mexerica; abrir não grava appearanceAccent.
test('D529: sem escolha salva o destaque é Mexerica nos dois temas, sem gravar preferência', () => {
  for (const dark of [false, true]) {
    const f = fixture({ appearanceTheme: JSON.stringify(dark ? 'dark' : 'light'), appearanceWallpaper: '"none"' }, { realTokens: true })
    assert.equal(f.a.DEFAULT_ACCENT, 'mexerica')
    assert.equal(f.a.appearancePrefs.accent.get(), 'mexerica')
    assert.equal(f.storage.has('appearanceAccent'), false)
    const actual = f.vars.get('--legible-accent')!.match(/[\d.]+/g)!.map(Number) as unknown as legibility.RGB
    assert.deepEqual(actual, legibility.readableForeground(tokenRGB('accent-mexerica', dark), tokenRGB('ink-2', dark), dark))
  }
})

test('D529: escolhas salvas, inclusive Verde e Violeta, prevalecem sobre o novo padrão nos dois temas', () => {
  for (const dark of [false, true]) for (const id of ['mexerica', 'folha', 'verde', 'violeta', 'jade', 'azul', 'ambar', 'rosa', 'grafite'] as const) {
    const raw = JSON.stringify(id)
    const f = fixture({ appearanceAccent: raw, appearanceTheme: JSON.stringify(dark ? 'dark' : 'light') })
    assert.equal(f.a.appearancePrefs.accent.get(), id)
    assert.equal(f.storage.get('appearanceAccent'), raw)
  }
})

test('D529: cor salva desconhecida ou corrompida usa o padrão nos dois temas sem regravar a preferência', () => {
  for (const dark of [false, true]) for (const raw of ['"desconhecida"', '""', '" verde "', 'null', '42', 'true', '[]', '{}', '', '{', 'verde']) {
    const f = fixture({ appearanceAccent: raw, appearanceTheme: JSON.stringify(dark ? 'dark' : 'light'), appearanceWallpaper: '"none"' }, { realTokens: true })
    assert.equal(f.a.appearancePrefs.accent.get(), f.a.DEFAULT_ACCENT, raw)
    assert.equal(f.storage.get('appearanceAccent'), raw, 'não regrava a preferência inválida ao abrir')
    const actual = f.vars.get('--legible-accent')!.match(/[\d.]+/g)!.map(Number) as unknown as legibility.RGB
    assert.deepEqual(actual, legibility.readableForeground(tokenRGB(`accent-${f.a.DEFAULT_ACCENT}`, dark), tokenRGB('ink-2', dark), dark), raw)
  }
})

test('D346: Verde tem token nos dois temas e passa o contraste dos outros destaques', () => {
  const lum = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((c) => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4).reduce((sum, c, i) => sum + c * [0.2126, 0.7152, 0.0722][i]!, 0)
  const ratio = (a: string, b: string) => { const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m); return (x! + 0.05) / (y! + 0.05) }
  for (const dark of [false, true]) {
    const verde = tokenValue('accent-verde', dark)
    assert.equal(verde, dark ? '#2dd4bf' : '#0c706e')
    assert.ok(ratio(tokenValue('on-accent', dark), verde) >= 5.3, 'texto sobre o botão')
    assert.ok(ratio(verde, tokenValue('canvas', dark)) >= 4.5, 'destaque sobre o fundo')
    assert.ok(ratio(verde, tokenValue('surface', dark)) >= 4.5, 'destaque sobre o cartão')
  }
})

test('D529: Mexerica e Folha abrem a lista, têm tradução e persistem nos dois temas', async () => {
  const { a } = fixture()
  assert.deepEqual(Array.from(a.ACCENTS, (x) => x.id), ['mexerica', 'folha', 'verde', 'violeta', 'azul', 'jade', 'ambar', 'rosa', 'grafite', 'laranja-claro', 'verde-claro'])
  for (const choice of a.ACCENTS) {
    assert.equal(choice.label, ptBR[choice.labelKey])
    for (const catalog of Object.values(CATALOGOS)) assert.ok(catalog[choice.labelKey]?.trim(), choice.labelKey)
    if (choice.id !== 'mexerica' && choice.id !== 'folha') continue
    for (const dark of [false, true]) {
      const color = tokenRGB(`accent-${choice.id}`, dark)
      assert.ok(legibility.contrast(tokenRGB('on-accent', dark), color) >= 5.3, `${choice.id}: texto sobre botão`)
      for (const ground of ['canvas', 'surface']) assert.ok(legibility.contrast(color, tokenRGB(ground, dark)) >= 4.5, `${choice.id}: ${ground}`)
    }
  }
  for (const id of ['mexerica', 'folha'] as const) for (const dark of [false, true]) {
    const f = fixture({ appearanceTheme: JSON.stringify(dark ? 'dark' : 'light'), appearanceWallpaper: '"none"' }, { realTokens: true })
    f.a.appearancePrefs.accent.set(id)
    await f.settle()
    assert.equal(f.storage.get('appearanceAccent'), JSON.stringify(id))
    const restored = fixture(Object.fromEntries(f.storage), { realTokens: true })
    assert.equal(restored.a.appearancePrefs.accent.get(), id)
    assert.match(css('../styles/theme.css'), new RegExp(`\\[data-accent="${id}"\\] \\{ --accent: var\\(--accent-${id}\\); \\}`))
    const foreground = restored.vars.get('--legible-accent')!.match(/[\d.]+/g)!.map(Number) as unknown as legibility.RGB
    for (const ground of ['canvas', 'surface']) assert.ok(legibility.contrast(foreground, tokenRGB(ground, dark)) >= 4.5, `${id}: texto ajustado em ${ground}`)
  }
})

test('destaques claros: mais luminosos que as referências, contraste e persistência nos dois temas', async () => {
  for (const [id, base] of [['laranja-claro', 'mexerica'], ['verde-claro', 'verde']] as const) {
    for (const dark of [false, true]) {
      const color = tokenRGB(`accent-${id}`, dark)
      assert.ok(legibility.luminance(color) > legibility.luminance(tokenRGB(`accent-${base}`, dark)), `${id}: mais claro no mesmo tema`)
      assert.ok(legibility.contrast(tokenRGB('on-accent', dark), color) >= 5.1, `${id}: tinta sobre o preenchimento`)
      for (const ground of ['canvas', 'surface']) assert.ok(legibility.contrast(color, tokenRGB(ground, dark)) >= 4.5, `${id}: ${ground}`)
      const f = fixture({ appearanceTheme: JSON.stringify(dark ? 'dark' : 'light'), appearanceWallpaper: '"none"' }, { realTokens: true })
      f.a.appearancePrefs.accent.set(id)
      await f.settle()
      assert.equal(f.dataset.accent, id)
      assert.equal(f.storage.get('appearanceAccent'), JSON.stringify(id))
      const restored = fixture(Object.fromEntries(f.storage), { realTokens: true })
      assert.equal(restored.a.appearancePrefs.accent.get(), id)
      assert.equal(restored.storage.get('appearanceAccent'), JSON.stringify(id))
      assert.match(css('../styles/theme.css'), new RegExp(`\\[data-accent="${id}"\\] \\{ --accent: var\\(--accent-${id}\\); \\}`))
    }
  }
})

test('Remover persiste antes de recuperar uma escolha inválida; a escolha válida seguinte fica guardada', async () => {
  const f = fixture(), { a, image } = f
  await a.chooseCustomWallpaper(image())
  const removal = a.removeCustomWallpaper()
  await assert.rejects(a.chooseCustomWallpaper(image('invalid.png')), { message: a.CUSTOM_ERRORS.unreadable })
  assert.equal(await removal, true)
  await f.settle()
  assert.equal(f.stored(), null)
  assert.equal(a.useAppearance().customURL, null)
  const nextRemoval = a.removeCustomWallpaper()
  assert.equal(await a.chooseCustomWallpaper(image('new.png')), true)
  await nextRemoval
  assert.equal(await f.stored()?.text(), 'new.png')
  const priorURL = a.useAppearance().customURL
  await assert.rejects(a.chooseCustomWallpaper(image('invalid.png')))
  await f.settle()
  assert.equal(a.useAppearance().customURL, priorURL)
  assert.equal(await f.stored()?.text(), 'new.png')
})

test('Nenhum repetido durante decode vence sem apagar a imagem já guardada', async () => {
  const f = fixture(), { a, image } = f
  await a.chooseCustomWallpaper(image())
  const priorURL = a.useAppearance().customURL
  a.appearancePrefs.wallpaper.set('none')
  const pending = a.chooseCustomWallpaper(image('pending.png'))
  await f.entered
  a.appearancePrefs.wallpaper.set('none')
  f.releaseDecode()
  assert.equal(await pending, false)
  assert.equal(a.appearancePrefs.wallpaper.get(), 'none')
  assert.equal(a.useAppearance().customURL, priorURL)
  assert.equal(await f.stored()?.text(), 'valid.png')
})

test('destaque e sliders durante decode não cancelam a importação', async () => {
  const f = fixture(), { a } = f
  const pending = a.chooseCustomWallpaper(f.image('pending.png'))
  await f.entered
  a.appearancePrefs.accent.set('jade')
  a.appearancePrefs.blur.set(12)
  a.appearancePrefs.transparency.set(80)
  a.appearancePrefs.veil.set(0.3)
  f.releaseDecode()
  assert.equal(await pending, true)
  assert.equal(a.appearancePrefs.wallpaper.get(), 'custom')
  assert.equal(await f.stored()?.text(), 'pending.png')
})

test('transparência por área: Geral iguala as áreas, Por área separa só a ajustada, sem fundo tira as variáveis', async () => {
  // Valor geral antigo guardado; modo e áreas inválidos voltam ao padrão sem mexer no geral (migração).
  const { a, settle, vars } = fixture({ appearanceTransparency: '0', appearanceTransparencyMode: '"bolhas"', appearanceTransparencyAreas: '{"sidebar":101}' })
  const p = a.appearancePrefs
  assert.equal(p.transparency.get(), 0)
  assert.equal(p.transparencyMode.get(), 'general')
  assert.deepEqual({ ...p.transparencyAreas.get() }, {})
  p.wallpaper.set('serra')
  await settle()
  // Geral: caixa e painel no mesmo vidro geral; placa da conversa 1 − valor (0% → opaca).
  const raised = vars.get('--glass-raised')
  const sidebar = vars.get('--glass-sidebar')
  assert.equal(vars.get('--glass-composer'), raised)
  assert.equal(vars.get('--glass-details'), raised)
  assert.equal(vars.get('--glass-conversation'), 'rgba(251, 250, 247, 1)')
  // Por área: só a caixa e o fundo da conversa ganham valor; a caixa fica presa no piso conservador da Serra (0,96).
  p.transparencyMode.set('custom')
  p.transparencyAreas.set({ composer: 100, conversation: 100 })
  await settle()
  assert.equal(vars.get('--glass-raised'), raised)
  assert.equal(vars.get('--glass-sidebar'), sidebar)
  assert.equal(vars.get('--glass-composer'), 'rgba(251, 250, 247, 0.98)')
  assert.equal(vars.get('--glass-conversation'), 'rgba(251, 250, 247, 0)')
  assert.equal(a.useAppearance().areas.composer.held, true)
  // Voltar a Geral guarda os valores próprios, mas eles deixam de valer.
  p.transparencyMode.set('general')
  await settle()
  assert.deepEqual({ ...p.transparencyAreas.get() }, { composer: 100, conversation: 100 })
  assert.equal(vars.get('--glass-composer'), raised)
  // Sem fundo: as variáveis das áreas saem e o app fica como antes.
  p.wallpaper.set('none')
  await settle()
  for (const name of ['--glass-composer', '--glass-details', '--glass-conversation']) assert.equal(vars.has(name), false)
})

/** PR g: largura e altura de um WebP (VP8 com perdas, VP8L ou VP8X) ou de um JPEG. */
function imageSize(bytes: Buffer): [number, number] {
  if (bytes.subarray(0, 4).toString('latin1') === 'RIFF' && bytes.subarray(8, 12).toString('latin1') === 'WEBP') {
    const chunk = bytes.subarray(12, 16).toString('latin1')
    if (chunk === 'VP8 ') { assert.equal(bytes.readUIntBE(23, 3), 0x9d012a); return [bytes.readUInt16LE(26) & 0x3fff, bytes.readUInt16LE(28) & 0x3fff] }
    if (chunk === 'VP8X') return [bytes.readUIntLE(24, 3) + 1, bytes.readUIntLE(27, 3) + 1]
    if (chunk === 'VP8L') { const b = bytes.readUInt32LE(21); return [(b & 0x3fff) + 1, ((b >> 14) & 0x3fff) + 1] }
    throw new Error('WebP sem dimensões')
  }
  return jpegSize(bytes)
}

function jpegSize(bytes: Buffer): [number, number] {
  assert.equal(bytes.readUInt16BE(0), 0xffd8, 'arquivo JPEG')
  for (let offset = 2; offset + 9 < bytes.length;) {
    const marker = bytes.readUInt16BE(offset)
    assert.equal(marker >> 8, 0xff)
    if (marker === 0xffc0 || marker === 0xffc2) return [bytes.readUInt16BE(offset + 7), bytes.readUInt16BE(offset + 5)]
    if (marker === 0xffda || marker === 0xffd9) break
    offset += 2 + bytes.readUInt16BE(offset + 2)
  }
  throw new Error('JPEG sem dimensões')
}

test('fundos da Mexerica e abstratos abrem o catálogo, preservam os antigos e entregam os pares medidos (WebP ou JPEG mantido)', () => {
  const { a } = fixture()
  assert.deepEqual(Array.from(a.HOUSE_WALLPAPERS, (w) => w.id), [...mexericaIds, ...codeIds, 'serra', 'duna', 'mare', 'noite', 'cidade', 'risca', 'mogno', ...abstractIds])
  assert.equal(measurements.images.length, 40)
  assert.equal(new Set(measurements.images.map((m) => m.file)).size, 40)
  assert.equal(mexericaMeasurements.tokensSha256, measurements.tokensSha256, 'Mexerica medida com os mesmos tokens')
  assert.equal(createHash('sha256').update(designTokensFile).digest('hex'), measurements.tokensSha256, 'medir novamente se os tokens mudarem')
  for (const id of newIds) {
    const wallpaper = a.HOUSE_WALLPAPERS.find((w) => w.id === id)!
    assert.ok(wallpaper.label.length > 0)
    assert.notEqual(wallpaper.light, wallpaper.dark)
    for (const dark of [false, true]) {
      const url = a.wallpaperURL(id, dark)!
      const measured = measurements.images.find((m) => m.id === id && m.dark === dark)!
      const file = measured.file
      assert.match(file, new RegExp(`^${id}${dark ? '-escuro' : ''}\\.(webp|jpg)$`))
      assert.ok(url.endsWith(`/${file}`))
      assert.equal(measured.id, id)
      assert.equal(measured.dark, dark)
      const bytes = readFileSync(new URL(url, import.meta.url))
      const size: [number, number] = (mexericaIds as readonly string[]).includes(id) ? [1672, 941] : [1920, 1200]
      assert.deepEqual(imageSize(bytes), size)
      assert.deepEqual([measured.width, measured.height], size)
      assert.equal(bytes.length, measured.bytes)
      assert.equal(createHash('sha256').update(bytes).digest('hex'), measured.sha256, `${file}: medir novamente após exportar`)
    }
  }
})

test('cada JPG novo mantém piso conservador sem véu e contraste de 4,5:1 nos dois temas', () => {
  const { a } = fixture()
  for (const m of measurements.images) {
    const tokens = (name: string) => tokenRGB(name, m.dark)
    const layers: legibility.GlassLayer[] = [
      { color: tokens('glass-sidebar'), offset: 0, tint: { color: tokens('ink'), alpha: m.dark ? 0.163 : 0.126 } },
      { color: tokens('canvas'), offset: 0 },
      { color: tokens('glass-raised'), offset: 0.02 },
      { color: tokens('glass-strong'), offset: 0.1 },
    ]
    const worst = legibility.worstOf(legibility.saturatedBounds(m.bounds, 1.4), m.dark)
    const wallpaper = a.HOUSE_WALLPAPERS.find((w) => w.id === m.id)!
    const floor = wallpaper.floor[m.dark ? 'dark' : 'light']
    assert.ok(Number.isFinite(floor) && floor >= 0 && floor <= 1)
    assert.equal(floor, m.floor)
    assert.equal(floor, legibility.safeFloor(worst, layers, tokens('ink-2')))
    const primary = a.floorFor(m.bounds, m.dark, 0, tokens)
    assert.equal(primary, m.primaryFloor)
    assert.ok(floor >= primary, `${m.file}: cadastro cobre o piso em execução`)
    for (const layer of layers) {
      assert.ok(legibility.contrast(tokens('ink-2'), legibility.layerGround(worst, layer, floor)) >= 4.5, m.file)
      assert.ok(legibility.contrast(tokens('ink'), legibility.layerGround(worst, layer, Math.max(primary, legibility.glassAlpha(100)))) >= 4.5, m.file)
    }
  }
})

test('selecionar cada fundo novo persiste o ID e troca o JPG e o piso ao mudar o tema', async () => {
  const f = fixture({ appearanceVeil: '0', appearanceTransparency: '100' }, { realTokens: true, pixels: true })
  for (const id of newIds) for (const dark of [false, true]) {
    const m = measurements.images.find((entry) => entry.id === id && entry.dark === dark)!
    f.a.appearancePrefs.theme.set(dark ? 'dark' : 'light')
    f.a.appearancePrefs.wallpaper.set(id)
    assert.equal(f.storage.get('appearanceWallpaper'), JSON.stringify(id))
    assert.ok(f.vars.get('--wp-image')!.endsWith(`/${m.file}")`))
    await f.settle()
    assert.equal(f.a.useAppearance().dark, dark)
    assert.equal(f.a.useAppearance().floor, m.primaryFloor)
    assert.equal(f.a.useAppearance().glass, Math.max(legibility.glassAlpha(100), m.primaryFloor))
  }
})

test('falha de leitura usa o cadastro dos novos fundos; opções do sistema continuam protegendo o texto', async () => {
  const f = fixture({ appearanceVeil: '0', appearanceTransparency: '100' }, { realTokens: true })
  for (const id of newIds) for (const dark of [false, true]) {
    f.a.appearancePrefs.theme.set(dark ? 'dark' : 'light')
    f.a.appearancePrefs.wallpaper.set(id)
    const floor = f.a.HOUSE_WALLPAPERS.find((w) => w.id === id)!.floor[dark ? 'dark' : 'light']
    assert.equal(f.a.useAppearance().floor, floor, 'proteção síncrona enquanto carrega')
    await f.settle()
    assert.equal(f.a.useAppearance().floor, floor, 'proteção após falhar')
    assert.equal(f.a.useAppearance().glass, floor)
  }
  for (const reduceTransparency of [false, true]) {
    const system = fixture({ appearanceWallpaper: '"lago"', appearanceTransparency: '100' }, { realTokens: true, pixels: true, reduceTransparency, moreContrast: true })
    await system.settle()
    assert.equal(system.a.useAppearance().glass, reduceTransparency ? 1 : legibility.HIGH_CONTRAST_FLOOR)
    system.a.appearancePrefs.wallpaper.set('none')
    await system.settle()
    assert.equal(system.a.wallpaperURL('none', false), null)
    assert.equal(system.vars.has('--wp-image'), false)
  }
})

// --- aparencia-01: defeitos da revisão de QA da Aparência (D70). ---

const css = (file: string) => readFileSync(new URL(file, import.meta.url), 'utf8')
/** The declarations of the rule whose selector is exactly `selector` (comments removed). */
function rule(source: string, selector: string) {
  const clean = source.replace(/\/\*[\s\S]*?\*\//g, '')
  const at = clean.indexOf(`${selector} {`)
  assert.ok(at >= 0, `regra ${selector}`)
  return clean.slice(at, clean.indexOf('}', at))
}

test('P1: na conversa, o Desfoque é o que se vê: a área não aplica o blur fixo do vidro por cima do fundo', () => {
  // O vidro de painel (28px) continua valendo para a barra lateral e os outros painéis…
  assert.match(rule(css('../styles/theme.css'), 'html[data-wallpaper] .glass-panel'), /backdrop-filter: blur\(var\(--blur-glass\)\)/)
  // …mas a conversa (o `.detail` com a `.thread-view`) fica sem backdrop-filter: o fundo aparece com o `--wp-blur` escolhido.
  const conversation = rule(css('../views/thread/thread.css'), 'html[data-wallpaper] .detail:has(> .thread-view)')
  assert.match(conversation, /(^|\s)backdrop-filter: none;/)
  assert.match(conversation, /-webkit-backdrop-filter: none;/)
  // A camada do fundo é a que recebe o valor do controle.
  assert.match(rule(css('../styles/theme.css'), '.app-wallpaper'), /filter: blur\(var\(--wp-blur, 0px\)\)/)
})

test('P1: o piso de legibilidade não depende do Desfoque, então tirar o blur da conversa não reduz o contraste', async () => {
  const { a, settle, vars } = fixture()
  a.appearancePrefs.wallpaper.set('serra')
  await settle()
  const before = new Map(vars)
  a.appearancePrefs.blur.set(40)
  await settle()
  assert.equal(vars.get('--wp-blur'), '40px')
  for (const name of ['--glass-sidebar', '--glass-raised', '--glass-strong', '--glass-plate', '--ink-2', '--ink-3']) assert.equal(vars.get(name), before.get(name), name)
})

test('P2: no escuro o fundo aparece mais com o véu padrão novo, e o texto principal segue a 4,5:1', async () => {
  const { a } = fixture()
  // Tokens reais do tema escuro (tokens.json).
  const source = JSON.parse(css('../../../../../apps/desktop/design/tokens.json')) as { color: { tokens: { name: string; value: string | { dark: string } }[] } }
  const tokens = (name: string): legibility.RGB => {
    const token = source.color.tokens.find((t) => t.name === name)!
    const raw = typeof token.value === 'string' ? token.value : token.value.dark
    const hex = /^#([0-9a-f]{6})$/i.exec(raw)
    if (hex) { const n = parseInt(hex[1]!, 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255] }
    const [r, g, b] = /rgba?\(([^)]+)\)/.exec(raw)![1]!.split(',').map(Number)
    return [r!, g!, b!]
  }
  assert.ok(a.VEIL_DEFAULT.dark < 0.32)
  const veilToken = source.color.tokens.find((t) => t.name === 'wallpaper-veil')!.value as { dark: string }
  assert.equal(Number(/,\s*([\d.]+)\)$/.exec(veilToken.dark)![1]), a.VEIL_DEFAULT.dark, 'token e padrão iguais')
  // Faixas e médias medidas nas versões noturnas da casa (PIL, imagem inteira).
  const pictures = {
    'cidade-escuro': { lo: [0, 0, 0], hi: [255, 218, 194], mean: [16, 21, 37] },
    'noite': { lo: [0, 0, 7], hi: [245, 245, 243], mean: [20, 25, 35] },
    'serra-escuro': { lo: [0, 5, 0], hi: [236, 241, 232], mean: [27, 41, 34] },
    'mogno-escuro': { lo: [8, 0, 0], hi: [117, 71, 40], mean: [48, 25, 14] },
  } as const
  const canvas = tokens('canvas'), veil = tokens('wallpaper-veil')
  const distance = (x: legibility.RGB, y: legibility.RGB) => (Math.abs(x[0] - y[0]) + Math.abs(x[1] - y[1]) + Math.abs(x[2] - y[2])) / 3
  // Sob a conversa no padrão (Geral 50%: placa do canvas a 0,5), o quanto a cor se afasta de Nenhum.
  const shownUnderConversation = (mean: legibility.RGB, alpha: number) => distance(legibility.over(canvas, 0.5, legibility.over(veil, alpha, mean)), canvas)
  for (const [name, p] of Object.entries(pictures)) {
    const old = shownUnderConversation(p.mean, 0.32)
    const now = shownUnderConversation(p.mean, a.VEIL_DEFAULT.dark)
    assert.ok(now >= old * 1.3, `${name}: o fundo aparece mais (${old.toFixed(1)} → ${now.toFixed(1)})`)
    // A legibilidade continua garantida pelo piso: texto principal a 4,5:1 na lateral, no pior ponto da imagem.
    const floor = a.floorFor({ lo: p.lo, hi: p.hi }, true, a.VEIL_DEFAULT.dark, tokens)
    assert.ok(floor < 1, `${name}: ainda dá para ver o fundo pelo vidro`)
    const glass = legibility.effectiveGlass(legibility.glassAlpha(50), floor, { moreContrast: false, reduceTransparency: false })
    const worst = legibility.worstOf(legibility.saturatedBounds(legibility.veiledBounds({ lo: p.lo, hi: p.hi }, veil, a.VEIL_DEFAULT.dark), 1.4), true)
    assert.ok(legibility.contrast(tokens('ink'), legibility.over(tokens('glass-sidebar'), glass, worst)) >= 4.5, `${name}: texto principal legível`)
  }
})

test('P2 HEIC: o seletor não oferece HEIC, e um HEIC é recusado com frase em português, sem decodificar nem mexer no fundo', async () => {
  const f = fixture(), { a } = f
  assert.doesNotMatch(a.CUSTOM_ACCEPT, /hei[cf]/i)
  assert.match(css('../views/settings/AppearanceSettings.tsx'), /accept=\{CUSTOM_ACCEPT\}/)
  await a.chooseCustomWallpaper(f.image())
  const priorURL = a.useAppearance().customURL
  for (const heic of [new File(['x'], 'IMG_0001.heic', { type: 'image/heic' }), new File(['x'], 'IMG_0002.HEIC', { type: '' })]) {
    await assert.rejects(a.chooseCustomWallpaper(heic), { message: a.CUSTOM_ERRORS.heic })
  }
  assert.match(a.CUSTOM_ERRORS.heic, /JPEG, PNG ou WebP/)
  assert.equal(f.decodedHeic(), false)
  await f.settle()
  assert.equal(a.appearancePrefs.wallpaper.get(), 'custom')
  assert.equal(a.useAppearance().customURL, priorURL)
  assert.equal(await f.stored()?.text(), 'valid.png')
})

test('P3: imagem que não abre mostra só a frase em português, nunca o texto do navegador', async () => {
  const { a, image } = fixture()
  await assert.rejects(a.chooseCustomWallpaper(image('invalid.png')), (e: Error) => e.message === a.CUSTOM_ERRORS.unreadable && !/decoded/.test(e.message))
})

test('P3: PNG e WebP são guardados como WebP (transparência mantida sobre o canvas do tema); JPEG segue JPEG', async () => {
  const f = fixture(), { a } = f
  await a.chooseCustomWallpaper(f.image('logo.png'))
  assert.equal(f.stored()?.type, 'image/webp')
  await a.chooseCustomWallpaper(new File(['w'], 'arte.webp', { type: 'image/webp' }))
  assert.equal(f.stored()?.type, 'image/webp')
  await a.chooseCustomWallpaper(new File(['j'], 'foto.jpg', { type: 'image/jpeg' }))
  assert.equal(f.stored()?.type, 'image/jpeg')
  // A área transparente cai sobre o canvas do tema, na tela e na medição do piso.
  assert.match(rule(css('../styles/theme.css'), '.app-wallpaper'), /no-repeat var\(--canvas\)/)
})

test('P3: com imagem própria, o primeiro quadro espera a imagem guardada (no máximo o prazo); com outro fundo, nada espera', async () => {
  const { a, image } = fixture({ appearanceWallpaper: '"none"' })
  await a.appearanceReady(5)
  await a.chooseCustomWallpaper(image())
  // A imagem já está na janela: não há espera.
  const started = Date.now()
  await a.appearanceReady(1000)
  assert.ok(Date.now() - started < 500)
  // D249 E2: a janela de conversa e a de pareamento abrem em views/fork/sob-demanda/janela-app.tsx.
  assert.match(css('../views/fork/sob-demanda/janela-app.tsx'), /appearanceReady\(\)\.then\(\(\) => root\.render\(/)
})

test('P3: a prévia em Ajustes respeita Reduzir transparência (sem desfoque no vidro da lateral)', () => {
  const source = css('../views/settings/appearance.css').replace(/\/\*[\s\S]*?\*\//g, '')
  const media = source.slice(source.indexOf('@media (prefers-reduced-transparency: reduce)'))
  assert.match(media, /\.appearance-preview:not\(\.plain\) \.appearance-preview-sidebar \{\s*-webkit-backdrop-filter: none;\s*backdrop-filter: none;/)
})

test('Noite só no tema escuro: no claro não é oferecido nem desenhado; a escolha volta no escuro', async () => {
  const { a, settle, vars } = fixture({ appearanceTheme: '"light"' })
  assert.equal(a.offeredIn('noite', false), false)
  assert.equal(a.offeredIn('noite', true), true)
  for (const w of a.HOUSE_WALLPAPERS.filter((w) => w.id !== 'noite')) assert.equal(a.offeredIn(w.id, false), true, w.id)
  assert.equal(a.wallpaperURL('noite', false), null)
  assert.ok(a.wallpaperURL('noite', true)?.endsWith('noite.webp'))
  // Escolhido no escuro, no claro o app fica sem fundo (opaco), sem perder a escolha.
  a.appearancePrefs.wallpaper.set('noite')
  await settle()
  assert.equal(a.appearancePrefs.wallpaper.get(), 'noite')
  assert.equal(vars.has('--wp-image'), false)
  assert.equal(a.useAppearance().floor, 0)
  a.appearancePrefs.theme.set('dark')
  await settle()
  assert.match(vars.get('--wp-image') ?? '', /noite\.webp/)
  // Ajustes lista só os fundos do tema e explica a ausência no claro.
  const settings = css('../views/settings/AppearanceSettings.tsx')
  assert.match(settings, /HOUSE_WALLPAPERS\.filter\(\(w\) => offeredIn\(w\.id, appearance\.dark\)\)/)
  // D454 (fatia 4): a frase está no catálogo; a tela a usa pela chave.
  assert.match(settings, /t\('ajustes\.aparencia\.noiteSoEscuro'\)/)
  const { ptBR } = await import('./fork-i18n/pt-BR.ts')
  assert.match(ptBR['ajustes.aparencia.noiteSoEscuro'], /Noite aparece só no tema escuro/)
})

test('MX-79: a grade de Aparência usa miniaturas; o fundo cheio só entra no --wp-image do aplicado', () => {
  const { a } = fixture()
  // Cada fundo de arquivo tem miniatura própria em wallpapers/thumbs/; a grade nunca referencia o
  // cheio. Os SVG de código não buscam nada (data URI) e ficam de fora.
  for (const w of a.HOUSE_WALLPAPERS) {
    for (const dark of [false, true]) {
      const full = a.wallpaperURL(w.id, dark)
      if (!full || full.startsWith('data:')) continue
      const thumb = a.wallpaperThumbURL(w.id, dark)
      assert.ok(thumb && thumb.includes('wallpapers/thumbs/'), `${w.id} (${dark ? 'escuro' : 'claro'}): miniatura ausente ou apontando o arquivo cheio`)
      assert.notEqual(thumb, full)
    }
  }
  // A tela passa wallpaperThumbURL à imagem; wallpaperURL só decide se a escolha é válida.
  const settings = css('../views/settings/AppearanceSettings.tsx')
  assert.match(settings, /image=\{wallpaperThumbURL\(w\.id, appearance\.dark\)/)
  assert.doesNotMatch(settings, /image=\{wallpaperURL\(/)
  // Sem pré-carga: nenhum prefetch/preload nem handler de hover/foco na grade.
  assert.doesNotMatch(settings, /prefetch|preload|onMouseEnter|onMouseOver|onPointerEnter/)
})

test('fonte: DM Sans por padrão, Sistema volta à pilha dos tokens; etiquetas grafite e valor inválido seguro', () => {
  const f = fixture()
  assert.equal(f.a.appearancePrefs.font.get(), 'dmsans')
  assert.equal(f.vars.get('--font-sans'), "'DM Sans', var(--font-system)")
  f.a.appearancePrefs.font.set('sistema')
  assert.equal(f.vars.has('--font-sans'), false)
  f.a.appearancePrefs.font.set('plex')
  assert.equal(f.vars.get('--font-sans'), "'IBM Plex Sans', var(--font-system)")
  assert.equal(f.vars.get('--tag-l1-bg'), 'var(--tag-grafite-bg)')
  assert.equal(f.vars.get('--tag-l0-ink'), 'var(--tag-grafite-ink)')
  f.a.appearancePrefs.sidebarTagColors.set({ l1: 'azul', l2: 'roxo', l3: 'ambar' })
  assert.equal(f.vars.get('--tag-l1-ink'), 'var(--tag-azul-ink)')
  assert.equal(f.vars.get('--tag-l2-bg'), 'var(--tag-grafite-bg)')
  assert.equal(f.vars.get('--tag-l3-bg'), 'var(--tag-ambar-bg)')
  const saved = fixture({ appearanceFont: '"comic"' })
  assert.equal(saved.a.appearancePrefs.font.get(), 'dmsans')
})

// --- DON-909: fundos em código (lib/fork-wallpapers-code.ts). ---

const codeModule = () => import('./fork-wallpapers-code.ts')
const svgOf = (url: string) => decodeURIComponent(url.replace(/^data:image\/svg\+xml,/, ''))
const ceil2 = (n: number) => Math.max(0, Math.ceil(n * 100 - 1e-9) / 100)

test('DON-909: cinco fundos em código (D381), IDs estáveis, SVG estático e autocontido nos dois temas', async () => {
  const { CODE_WALLPAPER_SPECS, W, H } = await codeModule()
  const { a } = fixture()
  assert.deepEqual(CODE_WALLPAPER_SPECS.map((s) => s.id), [...codeIds])
  for (const id of codeIds) {
    const w = a.HOUSE_WALLPAPERS.find((x) => x.id === id)!
    assert.equal(w.labelKey, `aparencia.fundo.${id}`)
    assert.equal(w.label, traduzir('pt-BR', w.labelKey!))
    const english = { aurora: 'Aurora', carbono: 'Carbon', geada: 'Frost', 'nascer-do-sol': 'Sunrise', 'donna-petroleo': 'Petrol' }
    assert.equal(traduzir('en', w.labelKey!), english[id])
    assert.notEqual(w.light, w.dark)
    for (const dark of [false, true]) {
      const url = a.wallpaperURL(id, dark)!
      assert.ok(url.startsWith('data:image/svg+xml,'), id)
      const svg = svgOf(url)
      assert.ok(svg.startsWith(`<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}"`), id)
      // Sem movimento, script, filtro caro nem recurso de fora.
      assert.doesNotMatch(svg, /<animate|<set|<script|<filter|<image|<foreignObject|href=|@keyframes/i, id)
      assert.equal(svg.match(/https?:/g)?.length, 1, `${id}: só o xmlns`)
      assert.ok(url.length < 12_000, `${id}: ${url.length} bytes`)
    }
  }
})

test('DON-909: piso de reserva cobre a caixa de cor da receita e mantém o texto a 4,5:1', async () => {
  const { CODE_WALLPAPER_SPECS, boundsOf } = await codeModule()
  const { a } = fixture()
  for (const spec of CODE_WALLPAPER_SPECS) for (const dark of [false, true]) {
    const tokens = (name: string) => tokenRGB(name, dark)
    const bounds = boundsOf(dark ? spec.dark : spec.light)
    const primary = a.floorFor(bounds, dark, 0, tokens)
    const floor = a.HOUSE_WALLPAPERS.find((w) => w.id === spec.id)!.floor[dark ? 'dark' : 'light']
    assert.equal(floor, ceil2(primary), `${spec.id} ${dark ? 'escuro' : 'claro'}: recalcular CODE_WALLPAPER_FLOORS (${ceil2(primary)})`)
    // A variante pertence ao tema: o vidro não fica preso no opaco.
    assert.ok(floor <= 0.9, `${spec.id} ${dark ? 'escuro' : 'claro'}: piso ${floor}`)
    const worst = legibility.worstOf(legibility.saturatedBounds(bounds, 1.4), dark)
    for (const layer of [{ color: tokens('canvas'), offset: 0 }, { color: tokens('glass-raised'), offset: 0.02 }, { color: tokens('glass-sidebar'), offset: 0, tint: { color: tokens('ink'), alpha: dark ? 0.163 : 0.126 } }]) {
      assert.ok(legibility.contrast(tokens('ink'), legibility.layerGround(worst, layer, floor)) >= 4.5, spec.id)
    }
  }
})

test('DON-909: a caixa de cor contém a base e cada mancha composta sobre ela', async () => {
  const { boundsOf } = await codeModule()
  const b = boundsOf({ base: { kind: 'linear', angle: 180, stops: [[0, '#101010'], [100, '#202020']] }, blooms: [{ left: 0, top: 0, w: 50, h: 50, color: '#FF0000', alpha: 0.5 }] })
  assert.deepEqual(b, { lo: [16, 8, 8], hi: [144, 32, 32] })
})

test('DON-909: escolher cada fundo em código grava o ID, escreve o SVG do tema e protege o texto antes da leitura', async () => {
  const f = fixture({ appearanceVeil: '0', appearanceTransparency: '100' }, { realTokens: true })
  for (const id of codeIds) for (const dark of [false, true]) {
    f.a.appearancePrefs.theme.set(dark ? 'dark' : 'light')
    f.a.appearancePrefs.wallpaper.set(id)
    assert.equal(f.storage.get('appearanceWallpaper'), JSON.stringify(id))
    assert.equal(f.vars.get('--wp-image'), `url("${f.a.wallpaperURL(id, dark)}")`)
    const floor = f.a.HOUSE_WALLPAPERS.find((w) => w.id === id)!.floor[dark ? 'dark' : 'light']
    assert.equal(f.a.useAppearance().floor, floor)
    await f.settle()
    assert.equal(f.a.useAppearance().glass, Math.max(legibility.glassAlpha(100), floor))
  }
  f.a.appearancePrefs.wallpaper.set('none')
  assert.equal(f.vars.has('--wp-image'), false)
})

// PR g: a preferência guarda o ID do fundo (nunca o nome do arquivo), então trocar .jpg por .webp não precisa de
// migração: quem salvou um fundo antes continua com ele, agora com o arquivo novo.
test('PR g: fundo salvo antes da troca para WebP continua selecionado e aparece com o arquivo novo', async () => {
  for (const [id, file] of [['cafe', 'cafe.webp'], ['cidade', 'cidade.webp'], ['duna', 'duna.webp'], ['risca', 'risca.webp']] as const) {
    const f = fixture({ appearanceTheme: '"light"', appearanceWallpaper: JSON.stringify(id) })
    await f.settle()
    assert.equal(f.a.appearancePrefs.wallpaper.get(), id, `${id}: a escolha salva continua`)
    assert.equal(f.storage.get('appearanceWallpaper'), JSON.stringify(id), `${id}: a preferência não é reescrita`)
    assert.ok(f.vars.get('--wp-image')!.endsWith(`/${file}")`), `${id}: ${f.vars.get('--wp-image')}`)
  }
})

// R1 do #524 (barrinha): sobre wallpaper, as marcas ficam num véu `--glass-raised` inteiro até o fim da maior marca.
// Com a pior cor da foto sob esse vidro (sem leitura de pixels: preto no claro, branco no escuro), Suavizar 0 e
// Transparência 100%, o repouso (`--ink-3`), a da tela (`--ink-2`), a atual/hover (`--ink`) e a não lida
// (`--accent-marca`) ficam a pelo menos 3:1 em qualquer ponto de qualquer marca (o R1-1 reproduzia 1,76:1 em x=30 e o
// R1-2, 2,14:1 na não lida com o accent nominal).
test('R1 #524: marcas da barrinha sobre o véu, na pior cor da foto, ficam a 3:1 no claro e no escuro', async () => {
  const parse = (v: string): [legibility.RGB, number] => {
    const [r, g, b, a = '1'] = /rgba?\(([^)]+)\)/.exec(v)![1]!.split(',').map((x) => x.trim())
    return [[Number(r), Number(g), Number(b)], Number(a)]
  }
  for (const dark of [false, true]) for (const accent of fixture().a.ACCENTS.map((x) => x.id)) {
    const f = fixture({ appearanceTheme: JSON.stringify(dark ? 'dark' : 'light'), appearanceAccent: JSON.stringify(accent), appearanceVeil: '0', appearanceTransparency: '100', appearanceWallpaper: '"serra"' }, { realTokens: true })
    await f.settle()
    const worst: legibility.RGB = dark ? [255, 255, 255] : [0, 0, 0]
    const [glass, alpha] = parse(f.vars.get('--glass-raised')!)
    const ground = legibility.over(glass, alpha, worst)
    const label = `${dark ? 'escuro' : 'claro'}, ${accent}`
    assert.ok(f.vars.has('--accent-marca'), `${label}: accent adaptado escrito`)
    for (const [name, min] of [['--ink-3', 3], ['--ink-2', 4.5], ['--accent-marca', 3]] as const) {
      const color = parse(f.vars.get(name)!)[0]
      assert.ok(legibility.contrast(color, ground) >= min, `${label}: ${name} ${legibility.contrast(color, ground).toFixed(2)}:1 sobre o véu`)
    }
  }
  // Revisão do #524: além da pior cor teórica, os extremos reais medidos (lidos pelo algoritmo, `pixels`) de um fundo
  // da Mexerica e de um abstrato, sob o véu, mantêm as mesmas margens.
  for (const id of ['jardim-citrico', 'cafe'] as const) for (const dark of [false, true]) {
    const m = measurements.images.find((entry) => entry.id === id && entry.dark === dark)!
    const f = fixture({ appearanceTheme: JSON.stringify(dark ? 'dark' : 'light'), appearanceVeil: '0', appearanceTransparency: '100', appearanceWallpaper: JSON.stringify(id) }, { realTokens: true, pixels: true })
    await f.settle()
    const [glass, alpha] = parse(f.vars.get('--glass-raised')!)
    for (const photo of [m.bounds.lo, m.bounds.hi, legibility.worstOf(m.bounds, dark)] as legibility.RGB[]) {
      const ground = legibility.over(glass, alpha, photo)
      for (const [name, min] of [['--ink-3', 3], ['--ink-2', 4.5], ['--accent-marca', 3]] as const) {
        const ratio = legibility.contrast(parse(f.vars.get(name)!)[0], ground)
        assert.ok(ratio >= min, `${m.file} [${photo}]: ${name} ${ratio.toFixed(2)}:1 sobre o véu`)
      }
    }
  }
  // Sem wallpaper o accent adaptado sai e vale o do tema.
  const none = fixture({ appearanceVeil: '0', appearanceTransparency: '100', appearanceWallpaper: '"none"' }, { realTokens: true })
  await none.settle()
  assert.equal(none.vars.has('--accent-marca'), false)
})


test('D530: toques ligados sem gravação inicial; valores inválidos não desligam por acidente', () => {
  for (const raw of [undefined, '"false"', 'null', '0', '{}', 'quebrado']) {
    const f = fixture(raw === undefined ? {} : { appearanceMexericaTouches: raw })
    assert.equal(f.a.appearancePrefs.mexericaTouches.get(), true)
    assert.equal(f.dataset.toquesMexerica, 'on')
    assert.equal(f.storage.get('appearanceMexericaTouches'), raw)
  }
})

test('D530: interruptor persiste e atualiza a janela sem alterar accent, vidro ou pisos', () => {
  const f = fixture({ appearanceMexericaTouches: 'false', appearanceAccent: '"azul"' })
  assert.equal(f.dataset.toquesMexerica, 'off')
  const visual = new Map(f.vars)
  f.a.appearancePrefs.mexericaTouches.set(true)
  assert.equal(f.dataset.toquesMexerica, 'on')
  assert.equal(f.storage.get('appearanceMexericaTouches'), 'true')
  f.a.appearancePrefs.mexericaTouches.set(false)
  assert.equal(f.dataset.toquesMexerica, 'off')
  assert.equal(f.storage.get('appearanceMexericaTouches'), 'false')
  assert.equal(f.dataset.accent, 'azul')
  assert.deepEqual(f.vars, visual)
})

test('D530: alteração em outra janela e remoção da preferência são refletidas', () => {
  const f = fixture()
  f.storageEvent({ key: 'appearanceMexericaTouches', newValue: 'false' })
  assert.equal(f.dataset.toquesMexerica, 'off')
  f.storageEvent({ key: 'appearanceMexericaTouches', newValue: null })
  assert.equal(f.dataset.toquesMexerica, 'on')
})


test('D530 R1: calor preserva foco canônico, hover tem escopo e progresso exige opt-in', () => {
  const css = readFileSync(new URL('../views/fork/toques-mexerica/toques.css', import.meta.url), 'utf8')
  assert.doesNotMatch(css, /transition:\s*inset-inline-start/, 'gomo não anima propriedade de layout')
  assert.doesNotMatch(css, /--hover\s*:/, 'não muda o papel global usado pelo restante do app')
  assert.doesNotMatch(css, /(?<!not\()(?<!focus-within):focus-visible\s*\{[^}]*box-shadow/, 'não substitui anel de foco por halo')
  for (const rule of css.matchAll(/([^{}]+)\{([^{}]+)\}/g)) {
    const [_, selector, body] = rule
    if (/background: var\(--mx-hover-fill\)/.test(body!)) {
      assert.match(selector!, /:root\[data-toques-mexerica="on"\]/)
      for (const scope of ['.space-rail-button', '.sidebar', '.icon-button', '.menu-row']) assert.ok(selector!.includes(scope))
      assert.doesNotMatch(selector!, /\.styled-button|\.choice-picker|\.switch|\.slider/)
    }
    if (/\.progress-fill:dir\(rtl\)/.test(selector!)) {
      assert.match(selector!, /:root\[data-toques-mexerica="on"\].*\.mx-task-progress/)
    }
    if (/box-shadow/.test(body!) && /\.styled-button\.primary|\.outlined-card|\.roster-row\.selected/.test(selector!)) {
      assert.match(selector!, /:not\(:focus-visible\)/)
    }
  }
})

test('D530 R1: texto e ícone do hover mantêm 4,5:1 nas cores e temas reais, sem alterar o accent', () => {
  assert.ok(toquesHover.TOQUES_HOVER_ALPHA >= .15 && toquesHover.TOQUES_HOVER_ALPHA <= .2)
  const accents = designTokens.filter(t => t.name.startsWith('accent-') && t.name !== 'accent-soft')
  for (const dark of [false, true]) for (const { name } of accents) {
    const id = name.slice('accent-'.length)
    const f = fixture({ appearanceTheme: JSON.stringify(dark ? 'dark' : 'light'), appearanceAccent: JSON.stringify(id), appearanceWallpaper: '"none"' }, { realTokens: true })
    const foreground = f.vars.get('--mx-hover-ink')!.match(/[\d.]+/g)!.map(Number) as unknown as legibility.RGB
    const ground = legibility.over(tokenRGB(name, dark), toquesHover.TOQUES_HOVER_ALPHA, tokenRGB('surface', dark))
    assert.ok(legibility.contrast(foreground, ground) >= 4.5, `${id}, ${dark ? 'escuro' : 'claro'}`)
    assert.equal(f.vars.get('--mx-hover-fill'), toquesHover.TOQUES_HOVER_FILL)
    assert.equal(f.dataset.accent, id)
  }
})

test('D530 R1: hover continua legível nos extremos RGB, inclusive sobre foto e vidro', () => {
  const corners = [0, 255].flatMap(r => [0, 255].flatMap(g => [0, 255].map(b => [r, g, b] as legibility.RGB)))
  for (const dark of [false, true]) for (const accent of corners) {
    const surface = tokenRGB('surface', dark)
    const ink = toquesHover.toquesHoverInk(accent, surface, dark)
    // O hover tem base opaca só no alvo. Foto/alfa anteriores não participam de seu fundo.
    const ground = legibility.over(accent, toquesHover.TOQUES_HOVER_ALPHA, surface)
    assert.ok(legibility.contrast(ink, ground) >= 4.5)
  }
})
