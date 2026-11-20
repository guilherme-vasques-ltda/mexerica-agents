import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/instrucoes.ts'

// Instruções do dono (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "instrucoes.pagina": "Instructions",
  "instrucoes.pagina.lead": "Write once how you want your bots to work. It applies to all of them, no need to edit each bot.",
  "instrucoes.geral.titulo": "For all my bots",
  "instrucoes.geral.ajuda": "Applies to every bot in any Space on this computer.",
  "instrucoes.geral.exemplo": "E.g. Keep answers short and in plain language. Don't ask me about execution details: pick the sensible default and tell me in one line.",
  "instrucoes.space.titulo": "In this Space",
  "instrucoes.space.ajuda": "Applies only to the bots of the open Space, on top of the text for all bots.",
  "instrucoes.space.exemplo": "E.g. Work here is for clients: formal tone, no slang.",
  "instrucoes.contador": "{n}/{max}",
  "instrucoes.estado.salvo": "Saved. It applies from each bot's next message, with nothing to restart.",
  "instrucoes.estado.vazio": "Empty. Nothing is added to the bots' instructions.",
  "instrucoes.estado.naoSalvo": "Not saved.",
  "instrucoes.estado.salvando": "Saving…",
  "instrucoes.estado.lendo": "Reading the instructions…",
  "instrucoes.estado.conflito": "The text changed somewhere else. Try again reads it anew and saves your text over the new version.",
  "instrucoes.acao.salvar": "Save",
  "instrucoes.acao.tentarDeNovo": "Try again",
  "instrucoes.falha.envio": "Couldn't save right now. Try again.",
  "instrucoes.falha.leitura": "Couldn't read the instructions right now.",
  "instrucoes.falha.limite": "The text is over the character limit.",
  "instrucoes.falha.invalido": "The text has an invisible or control character. Delete it and try again.",
  "instrucoes.falha.spaceSumiu": "This Space no longer exists.",
  "instrucoes.falha.soDono": "Only the owner, on this computer, can change the instructions.",
  "instrucoes.falha.hubAntigo": "Update the Hub to use instructions.",
  "instrucoes.semConexao": "No connection to the Hub. The instructions show up when it is back.",
  "instrucoes.ordem.titulo": "Order:",
  "instrucoes.ordem.texto": "what you write on the bot itself outweighs \"In this Space\", which outweighs \"For all my bots\". They are guidance: they don't grant permission, tools, models or accounts.",
}
