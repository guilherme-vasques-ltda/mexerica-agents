import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/sistema/conversa.ts'

// Fatia 10b (D454): sistema/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "sistema.conversa.espera.semHora": "Waiting · as soon as it frees up",
  "sistema.conversa.espera.comHora": "Waiting · resumes at {hora}",
  "sistema.conversa.espera.retomada": "resumes at {hora}",
  "sistema.conversa.espera.parou": "Stopped at the plan limit; {volta}",
  "sistema.conversa.espera.parouSemHora": "Stopped at the plan limit; the provider didn’t say when it’s back",
  "sistema.conversa.rapido.ligar": "Turn on for faster replies and more usage",
  "sistema.conversa.rapido.desligar": "Turn off and return to standard speed",
  "sistema.conversa.rapido.ligarTodos": "Turn on for every bot here: faster replies and more usage",
  "sistema.conversa.rapido.desligarTodos": "Turn off for every bot here and return to standard speed",
  "sistema.conversa.rapido.recusado": "The agent turned it off: {motivo}. Switch it off and on to try again.",
  "sistema.conversa.rapido.recusadoBot": "{bot}: the agent turned it off: {motivo}. Switch it off and on to try again.",
  "sistema.conversa.rapido.indisponivel": "not available",
  "sistema.conversa.parteIndisponivel": "This part did not load. If it keeps happening, save what you wrote and reload the page or reopen the app.",
  "sistema.conversa.tentarParte": "Try again",
}
