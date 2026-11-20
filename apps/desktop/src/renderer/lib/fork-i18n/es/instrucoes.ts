import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/instrucoes.ts'

// Instruções do dono (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "instrucoes.pagina": "Instrucciones",
  "instrucoes.pagina.lead": "Escribe una vez cómo quieres que trabajen tus bots. Vale para todos, sin tocar bot por bot.",
  "instrucoes.geral.titulo": "Para todos mis bots",
  "instrucoes.geral.ajuda": "Vale para todo bot de cualquier Space de este ordenador.",
  "instrucoes.geral.exemplo": "Ej.: Respuestas cortas y en lenguaje sencillo. No me preguntes detalles de ejecución: elige lo razonable y cuéntamelo en una línea.",
  "instrucoes.space.titulo": "En este Space",
  "instrucoes.space.ajuda": "Vale solo para los bots del Space abierto, además del texto de todos los bots.",
  "instrucoes.space.exemplo": "Ej.: Aquí el trabajo es para clientes: tono formal y sin jerga.",
  "instrucoes.contador": "{n}/{max}",
  "instrucoes.estado.salvo": "Guardado. Vale desde el siguiente mensaje de cada bot, sin reiniciar nada.",
  "instrucoes.estado.vazio": "Vacío. No se añade nada a las instrucciones de los bots.",
  "instrucoes.estado.naoSalvo": "Sin guardar.",
  "instrucoes.estado.salvando": "Guardando…",
  "instrucoes.estado.lendo": "Leyendo las instrucciones…",
  "instrucoes.estado.conflito": "El texto cambió en otro lugar. Reintentar lo lee de nuevo y guarda tu texto sobre la versión nueva.",
  "instrucoes.acao.salvar": "Guardar",
  "instrucoes.acao.tentarDeNovo": "Reintentar",
  "instrucoes.falha.envio": "No se pudo guardar ahora. Inténtalo de nuevo.",
  "instrucoes.falha.leitura": "No se pudieron leer las instrucciones ahora.",
  "instrucoes.falha.limite": "El texto superó el límite de caracteres.",
  "instrucoes.falha.invalido": "El texto tiene un carácter invisible o de control. Bórralo e inténtalo de nuevo.",
  "instrucoes.falha.spaceSumiu": "Este Space ya no existe.",
  "instrucoes.falha.soDono": "Solo el dueño, en este ordenador, cambia las instrucciones.",
  "instrucoes.falha.hubAntigo": "Actualiza el Hub para usar las instrucciones.",
  "instrucoes.semConexao": "Sin conexión con el Hub. Las instrucciones aparecen cuando vuelva.",
  "instrucoes.ordem.titulo": "Orden:",
  "instrucoes.ordem.texto": "lo que escribes en el propio bot vale más que «En este Space», que vale más que «Para todos mis bots». Son orientaciones: no dan permiso, herramienta, modelo ni cuenta.",
}
