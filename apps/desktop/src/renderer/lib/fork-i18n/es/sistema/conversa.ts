import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/sistema/conversa.ts'

// Fatia 10b (D454): sistema/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "sistema.conversa.espera.semHora": "Esperando · en cuanto se libere",
  "sistema.conversa.espera.comHora": "Esperando · se reanuda a las {hora}",
  "sistema.conversa.espera.retomada": "se reanuda a las {hora}",
  "sistema.conversa.espera.parou": "Se detuvo por el límite del plan; {volta}",
  "sistema.conversa.espera.parouSemHora": "Se detuvo por el límite del plan; el proveedor no informó cuándo vuelve",
  "sistema.conversa.rapido.ligar": "Actívalo para respuestas más rápidas y más uso",
  "sistema.conversa.rapido.desligar": "Desactívalo y vuelve a la velocidad estándar",
  "sistema.conversa.rapido.ligarTodos": "Actívalo para todos los bots de aquí: respuestas más rápidas y más uso",
  "sistema.conversa.rapido.desligarTodos": "Desactívalo para todos los bots de aquí y vuelve a la velocidad estándar",
  "sistema.conversa.rapido.recusado": "El agente lo desactivó: {motivo}. Desactívalo y vuelve a activarlo para intentarlo de nuevo.",
  "sistema.conversa.rapido.recusadoBot": "{bot}: el agente lo desactivó: {motivo}. Desactívalo y vuelve a activarlo para intentarlo de nuevo.",
  "sistema.conversa.rapido.indisponivel": "no disponible",
  "sistema.conversa.parteIndisponivel": "Esta parte no se cargó. Si sigue igual, guarda lo que escribiste y recarga la página o reabre la app.",
  "sistema.conversa.tentarParte": "Intentar de nuevo",
}
