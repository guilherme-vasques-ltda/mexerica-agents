// Gates de Ajustes pelas capacidades do aparelho (shared/fork-capacidades.ts): o app Web/PWA não tem
// perfil da conta, "Abrir ao iniciar sessão", ícone do app, Computadores por SSH nem chaves do computador.
// Sem DOM nem React: quem desenha só pergunta aqui, e nenhuma chave crua da ponte Web chega ao usuário.
import type { Capacidades } from '../../../../shared/fork-capacidades.ts'
import type { SettingsPage } from './tela.ts'

/** O bloco de perfil (nome, senha, contas ligadas, duas etapas) só existe com backend de conta no aparelho. */
export function mostraPerfilConta(cap: Pick<Capacidades, 'perfilConta'>, conta: { isSignedIn: boolean; isExitUnresolved: boolean; isSigningOut: boolean }): boolean {
  return cap.perfilConta && conta.isSignedIn && !conta.isExitUnresolved && !conta.isSigningOut
}

/** Páginas de Ajustes que dependem de recurso nativo; as demais valem em qualquer aparelho. */
export function paginaDisponivel(id: SettingsPage, cap: Pick<Capacidades, 'appIcon'>): boolean {
  return id !== 'app-icon' || cap.appIcon
}

/** Em Voz, a aba Chaves só tem o que mostrar com o cofre do computador ou com algum Hub gerenciado. */
export function abasVisiveis<A extends { id: string }>(page: SettingsPage, abas: readonly A[] | undefined, cap: Pick<Capacidades, 'cofreLocal'>, hubs: number): readonly A[] | undefined {
  return page === 'voice' && !cap.cofreLocal && hubs === 0 ? abas?.filter((a) => a.id !== 'chaves') : abas
}
