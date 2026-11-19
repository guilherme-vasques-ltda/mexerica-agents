// MX-277 (continua a MX-214): o gancho "abrir cofre para salvar login novo", chamado por "Mover para o cofre" no aviso
// de senha colada. Agora abre a JANELA SEGURA (MX-209) vazia, pelo main: só o id do computador vai no IPC, NUNCA o
// trecho escondido (que o chat já descartou). A pessoa cola a senha de novo, direto na janela segura.
import type { RespostaAbrir } from '@shared/fork-cofre-seguro'

/** `computador`: o computador (Hub) da conversa em que a pessoa colou. Sem valor, sem texto, sem nome do bot. */
export type AbrirCofreParaLoginNovo = (computador: string) => Promise<RespostaAbrir>

export const abrirCofreParaLoginNovo: AbrirCofreParaLoginNovo = (computador) =>
  window.codync.forkCofre.abrirNovo({ computerId: computador }).catch(() => ({ ok: false as const, motivo: 'semConexao' as const }))
