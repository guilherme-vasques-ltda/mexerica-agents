/** Etapas mantidas como catálogo local; nenhum evento sai do desktop. */
export type HostSetupStage = 'missing_binary' | 'install' | 'verify' | 'not_responding' | 'environment'

export function noteAnalyticsSetting(_url: string, _method: string, _value: unknown) {}

/** Desativado em todos os builds, inclusive na primeira instalação. */
export function reportHostSetupFailure(_stage: HostSetupStage) {}
