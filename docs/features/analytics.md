# Telemetria na Mexerica

A telemetria está desativada no host e no desktop, inclusive `release` e `dev-app`.
Preferências herdadas e `setAnalytics(true)` não a reativam nesses componentes. O host informa
`analytics: false`; seu envio é um no-op sem endpoint ou token.

O desktop descarta `track`, `reportError` e `reportHostSetupFailure`, inclusive na
primeira instalação. iOS e site também não possuem envio ou token PostHog.
O catálogo tipado de eventos é preservado como referência local, sem coleta.


O [catálogo herdado](../reference/analytics-events.md) descreve os nomes e tipos
originais, não uma coleta ativa da Mexerica. Não existe backend de métricas próprio
nesta fase. Logs locais continuam seguindo os mecanismos existentes do host.

No Android, o envio exige ativação nas configurações e uma chave fornecida pelo build; sem chave ou sem ativação, não há envio. O controle de telemetria só aparece quando o build fornece uma chave.
