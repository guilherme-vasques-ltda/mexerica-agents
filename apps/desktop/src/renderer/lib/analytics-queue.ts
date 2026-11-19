import type { AnalyticsEvent } from '@shared/analytics'

export type AnalyticsSender = (event: AnalyticsEvent) => Promise<void>

/** Contrato local preservado; eventos não são enfileirados nem encaminhados. */
export class AnalyticsQueue {
  constructor(_limit = 100) {}

  track(_event: AnalyticsEvent) {}

  connect(_sender: AnalyticsSender | null) {}
}
