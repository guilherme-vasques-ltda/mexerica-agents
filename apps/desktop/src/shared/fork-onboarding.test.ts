import assert from 'node:assert/strict'
import test from 'node:test'
import {
  onboardingNavigationKey,
  isLocalPersonalContext,
  mapAiTestOutcome,
  resolveLocalInstallStage,
  shouldShowFirstChatOnboarding,
  STARTER_TASKS,
  summarizeReadiness,
  TOTAL_ONBOARDING_STEPS,
} from './fork-onboarding.ts'

test('a escolha de navegação só reaparece no mesmo contexto em memória', () => {
  const key = onboardingNavigationKey(1, 'account-a', 'computer-a', 'hub-a', 'space-a')
  const choices = { [key]: 'remote' }
  for (const other of [
    onboardingNavigationKey(1, 'account-b', 'computer-a', 'hub-a', 'space-a'),
    onboardingNavigationKey(1, null, 'computer-a', 'hub-a', 'space-a'),
    onboardingNavigationKey(1, 'account-a', 'computer-a', 'hub-b', 'space-a'),
    onboardingNavigationKey(1, 'account-a', 'computer-a', 'hub-a', 'space-b'),
    onboardingNavigationKey(2, 'account-a', 'computer-a', 'hub-a', 'space-a'),
  ]) assert.equal(choices[other], undefined)
  assert.equal(choices[onboardingNavigationKey(1, 'account-a', 'computer-a', 'hub-a', 'space-a')], 'remote')
})

test('fluxo oficial tem 4 etapas e oferece 6 tarefas', () => {
  assert.equal(TOTAL_ONBOARDING_STEPS, 4)
  assert.equal(STARTER_TASKS.length, 6)
})

test('resolveLocalInstallStage exige consentimento e nunca fabrica instalacao quando bloqueado', () => {
  assert.deepEqual(
    resolveLocalInstallStage({
      hostKind: 'notInstalled',
      localHostFeature: true,
      macLocalHubFeature: false,
      devHost: false,
      localHubSupported: true,
      platform: 'linux',
      consentGiven: false,
    }),
    { kind: 'needs_consent' },
  )
  assert.deepEqual(
    resolveLocalInstallStage({
      hostKind: 'notInstalled',
      localHostFeature: true,
      macLocalHubFeature: false,
      devHost: false,
      localHubSupported: true,
      platform: 'linux',
      consentGiven: true,
    }),
    { kind: 'needs_install' },
  )
  assert.deepEqual(
    resolveLocalInstallStage({
      hostKind: 'disabled',
      localHostFeature: false,
      macLocalHubFeature: false,
      devHost: false,
      localHubSupported: false,
      platform: 'darwin',
      consentGiven: true,
    }),
    { kind: 'blocked_platform_disabled' },
  )
  assert.deepEqual(
    resolveLocalInstallStage({
      hostKind: 'missingBinary',
      localHostFeature: true,
      macLocalHubFeature: false,
      devHost: false,
      localHubSupported: true,
      platform: 'win32',
      consentGiven: true,
    }),
    { kind: 'blocked_missing_binary' },
  )
  assert.deepEqual(
    resolveLocalInstallStage({
      hostKind: 'starting',
      localHostFeature: true,
      macLocalHubFeature: false,
      devHost: false,
      localHubSupported: true,
      platform: 'linux',
      consentGiven: true,
    }),
    { kind: 'starting' },
  )
  assert.deepEqual(
    resolveLocalInstallStage({
      hostKind: 'running',
      localHostFeature: true,
      macLocalHubFeature: false,
      devHost: false,
      localHubSupported: true,
      platform: 'linux',
      consentGiven: true,
    }),
    { kind: 'ready' },
  )
})

test('mapAiTestOutcome distingue resultados de chave sem falsificar acesso', () => {
  assert.equal(mapAiTestOutcome({ ok: true, modelosCount: 5 }), 'modelos_no_catalogo')
  assert.equal(mapAiTestOutcome({ ok: false, erro: 'sem_lista' }), 'acesso_nao_confirmado')
  assert.equal(mapAiTestOutcome({ ok: false, erro: 'sem_lista', salvo: true }), 'chave_salva_sem_lista')
  assert.equal(mapAiTestOutcome({ ok: true, modelosCount: 0 }), 'chave_salva_sem_lista')
  assert.equal(mapAiTestOutcome({ ok: false, erro: 'sem_resposta' }), 'nao_verificado')
  assert.equal(mapAiTestOutcome({ ok: false, erro: 'chave_invalida' }), 'recusada')
})

test('a primeira conversa deixa de mostrar o guia após a primeira mensagem', () => {
  assert.equal(
    shouldShowFirstChatOnboarding({
      isPrincipalBot: true,
      isGroup: false,
      hasUserMessages: false,
        dismissedLocally: false,
    }),
    true,
  )
  assert.equal(
    shouldShowFirstChatOnboarding({
      isPrincipalBot: true,
      isGroup: false,
      hasUserMessages: true,
        dismissedLocally: false,
    }),
    false,
  )
  assert.deepEqual(summarizeReadiness(null), { readyCount: 0, totalCount: 0, items: [] })
})

test('Space de outro computador nao pode ser tratado como Space pessoal local', () => {
  assert.equal(isLocalPersonalContext({ activeComputerId: 'remote-computer', localComputerId: 'local-computer', activeSpaceId: 'space-remote' }), false)
  assert.equal(isLocalPersonalContext({ activeComputerId: 'local-computer', localComputerId: 'local-computer', activeSpaceId: 'space-local' }), false)
  assert.equal(isLocalPersonalContext({ activeComputerId: 'remote-computer', localComputerId: null, activeSpaceId: 'space-remote' }), false)
  assert.equal(isLocalPersonalContext({ activeComputerId: 'local-computer', localComputerId: 'local-computer', activeSpaceId: '00000000-0000-4000-8000-000000000001' }), true)
  assert.equal(isLocalPersonalContext({ activeComputerId: null, localComputerId: null, activeSpaceId: null }), true)
})
