import assert from 'node:assert/strict'
import test from 'node:test'
import {
  RELEASE_TAG_URL_PREFIX,
  compareSemver,
  isNewerRelease,
  parseLatestReleasePayload,
  parseStableSemver,
  shouldShowReleaseBanner,
  validateRedirectUrl,
  validateReleaseUrl,
  type ReleaseNoticeState,
} from './fork-release-notice.ts'

const TAG_URL = (tag: string) => `${RELEASE_TAG_URL_PREFIX}${tag}`

test('parseStableSemver: aceita estável com e sem v, e rejeita todo o resto', () => {
  assert.deepEqual(parseStableSemver('v0.2.0'), { major: 0, minor: 2, patch: 0, canonical: '0.2.0' })
  assert.deepEqual(parseStableSemver('1.2.3'), { major: 1, minor: 2, patch: 3, canonical: '1.2.3' })
  assert.equal(parseStableSemver('0.0.0')?.canonical, '0.0.0')
  for (const invalid of [
    'v0.2.0-alpha', 'v0.2.0-beta.1', 'v0.2.0-rc.1', 'v0.2.0+build.7', 'v1.2', 'v1.2.3.4', 'v01.2.3', 'v1.02.3',
    ' v0.2.0', 'v0.2.0 ', '0.2.x', '', 'zero.zero.zero', 'v0.2.0/v0.3.0', 1, null, undefined, {}, ['v0.2.0'],
  ]) {
    assert.equal(parseStableSemver(invalid), null, JSON.stringify(invalid))
  }
})

test('compareSemver: major manda, depois minor, depois patch', () => {
  const v = (raw: string) => parseStableSemver(raw)!
  assert.equal(compareSemver(v('1.0.0'), v('0.9.9')), 1)
  assert.equal(compareSemver(v('1.2.0'), v('1.10.0')), -1)
  assert.equal(compareSemver(v('1.2.3'), v('1.2.10')), -1)
  assert.equal(compareSemver(v('0.2.0'), v('0.2.0')), 0)
})

test('isNewerRelease: só candidato estritamente maior; igual, downgrade, pré-lançamento e ilegível são false', () => {
  assert.equal(isNewerRelease('v0.1.0', 'v0.2.0'), true)
  assert.equal(isNewerRelease('0.1.9', '0.2.0'), true)
  assert.equal(isNewerRelease('v0.2.0', 'v0.2.0'), false)
  assert.equal(isNewerRelease('v0.3.0', 'v0.2.0'), false)
  assert.equal(isNewerRelease('v0.2.0', 'v0.3.0-alpha'), false)
  assert.equal(isNewerRelease('v0.2.0-alpha', 'v0.3.0'), false)
  assert.equal(isNewerRelease('v0.2.0', 'lixo'), false)
  assert.equal(isNewerRelease('lixo', 'v0.3.0'), false)
  assert.equal(isNewerRelease(null, 'v0.3.0'), false)
  assert.equal(isNewerRelease('v0.2.0', null), false)
})

test('validateReleaseUrl: aceita só a página da tag deste repositório, exata e em HTTPS', () => {
  const boa = TAG_URL('v0.2.0')
  assert.equal(validateReleaseUrl(boa), boa)
  assert.equal(validateReleaseUrl(TAG_URL('0.2.0')), TAG_URL('0.2.0'))
  assert.equal(validateReleaseUrl(boa, 'v0.2.0'), boa)
})

test('validateReleaseUrl: rejeita HTTP, outro domínio, subdomínio, credenciais, porta, query e fragmento', () => {
  const boa = TAG_URL('v0.2.0')
  const rejeitadas = [
    boa.replace('https://', 'http://'),
    boa.replace('github.com', 'github.com.evil.example.invalid'),
    boa.replace('://github.com', '://evil.example.invalid'),
    boa.replace('://github.com', '://api.github.com'),
    boa.replace('://github.com', '://user:pass@github.com'),
    `${boa}:8443`,
    `${boa}?x=1`,
    `${boa}#fragmento`,
    boa.replace('v0.2.0', 'v0.2.0/../../v0.9.0'),
    boa.replace('v0.2.0', '..'),
    boa.replace('v0.2.0', 'v0.2.0%2F'),
    boa.replace('v0.2.0', 'v0.2.0 extra'),
    `${boa}\n`,
    boa.replace('v0.2.0', 'v0.2'),
    boa.replace('v0.2.0', 'v0.2.0-beta.1'),
    'file:///Applications/Mexerica.app',
    'javascript:alert(1)',
    'https://github.com/outro-dono/mexerica-agents/releases/tag/v0.2.0',
    '',
    '   ',
    null,
    undefined,
    42,
  ]
  for (const url of rejeitadas) assert.equal(validateReleaseUrl(url), null, JSON.stringify(url))
  assert.equal(validateReleaseUrl(boa, 'v0.3.0'), null, 'tag diferente da esperada')
})

test('validateRedirectUrl: só HTTPS nos dois hosts permitidos, sem porta nem credencial', () => {
  const base = 'https://api.github.com/repos/guilherme-vasques-ltda/mexerica-agents/releases/latest'
  assert.equal(validateRedirectUrl('https://api.github.com/repos/outro/caminho', base), 'https://api.github.com/repos/outro/caminho')
  assert.equal(validateRedirectUrl('https://github.com/leepokai/Codync', base), 'https://github.com/leepokai/Codync')
  assert.equal(validateRedirectUrl('/repos/a/b/releases/2', base), 'https://api.github.com/repos/a/b/releases/2')
  const rejeitados = [
    'http://api.github.com/x', 'https://evil.example.invalid/x', 'https://github.com.evil.example.invalid/x',
    'https://r2.example.invalid/x', 'https://codync.dev/x', 'https://user@api.github.com/x',
    'https://api.github.com:443/x', 'ftp://api.github.com/x', '', '  ', null, 42,
  ]
  for (const location of rejeitados) assert.equal(validateRedirectUrl(location, base), null, JSON.stringify(location))
})

test('parseLatestReleasePayload: rascunho e pré-lançamento não são release a avisar', () => {
  const payload = (extra: Record<string, unknown>) => ({ tag_name: 'v0.2.0', html_url: TAG_URL('v0.2.0'), ...extra })
  assert.deepEqual(parseLatestReleasePayload(payload({ draft: true }), 'v0.1.0'), { kind: 'up-to-date' })
  assert.deepEqual(parseLatestReleasePayload(payload({ prerelease: true }), 'v0.1.0'), { kind: 'up-to-date' })
})

test('parseLatestReleasePayload: payload fora do previsto invalida a resposta inteira', () => {
  const rejeitados: unknown[] = [null, undefined, 42, 'texto', [], [{ tag_name: 'v0.2.0' }], {}, { tag_name: 'v0.2.0' }]
  for (const invalid of rejeitados) {
    assert.deepEqual(parseLatestReleasePayload(invalid, 'v0.1.0'), { kind: 'invalid' }, JSON.stringify(invalid))
  }
  assert.deepEqual(parseLatestReleasePayload({ tag_name: 'v0.2.x', html_url: TAG_URL('v0.2.0') }, 'v0.1.0'), { kind: 'invalid' })
  assert.deepEqual(parseLatestReleasePayload({ tag_name: 'v0.2.0', html_url: 'https://evil.example.invalid/v0.2.0' }, 'v0.1.0'), { kind: 'invalid' })
  assert.deepEqual(parseLatestReleasePayload({ tag_name: 'v0.2.0', html_url: TAG_URL('v0.9.0') }, 'v0.1.0'), { kind: 'invalid' }, 'tag difere da URL')
})

test('parseLatestReleasePayload: versão nova vira available com tag canônica; igual ou menor vira up-to-date', () => {
  assert.deepEqual(parseLatestReleasePayload({ tag_name: 'v0.2.0', html_url: TAG_URL('v0.2.0') }, 'v0.1.0'), {
    kind: 'available', version: '0.2.0', tag: 'v0.2.0', url: TAG_URL('v0.2.0'),
  })
  assert.deepEqual(parseLatestReleasePayload({ tag_name: '0.2.0', html_url: TAG_URL('0.2.0') }, 'v0.1.0'), {
    kind: 'available', version: '0.2.0', tag: 'v0.2.0', url: TAG_URL('0.2.0'),
  })
  assert.deepEqual(parseLatestReleasePayload({ tag_name: 'v0.1.0', html_url: TAG_URL('v0.1.0') }, 'v0.1.0'), { kind: 'up-to-date' })
  assert.deepEqual(parseLatestReleasePayload({ tag_name: 'v0.0.9', html_url: TAG_URL('v0.0.9') }, 'v0.1.0'), { kind: 'up-to-date' })
})

const estado = (mudanca: Partial<ReleaseNoticeState>): ReleaseNoticeState => ({
  supported: true, currentVersion: '0.1.0', status: 'available', availableVersion: '0.2.0', availableTag: 'v0.2.0',
  releaseUrl: TAG_URL('v0.2.0'), dismissedVersion: null, lastCheckAt: null, ...mudanca,
})

test('shouldShowReleaseBanner: só com suporte, versão, tag, URL válida e sem dispensa', () => {
  assert.equal(shouldShowReleaseBanner(estado({})), true)
  assert.equal(shouldShowReleaseBanner(null), false)
  assert.equal(shouldShowReleaseBanner(undefined), false)
  assert.equal(shouldShowReleaseBanner(estado({ supported: false })), false)
  assert.equal(shouldShowReleaseBanner(estado({ availableVersion: null })), false)
  assert.equal(shouldShowReleaseBanner(estado({ availableTag: null })), false)
  assert.equal(shouldShowReleaseBanner(estado({ releaseUrl: null })), false)
  assert.equal(shouldShowReleaseBanner(estado({ dismissedVersion: '0.2.0' })), false)
  assert.equal(shouldShowReleaseBanner(estado({ dismissedVersion: '0.1.9' })), true)
})
