import { BrowserHubSlot, BrowserLiveSession } from '@shared/fork-browser-live'

/**
 * O Hub só deixa inscrever a mesma sessão uma vez por canal e guarda UMA inscrição de navegador por canal: inscrever
 * outra sessão derruba a anterior. Fechar o painel então só pausa a sessão (o consumidor local sai e libera o slot de
 * quadros da conexão); abrir de novo religa o consumidor, e se outra sessão da conexão assumiu o Hub nesse meio-tempo
 * a sessão se inscreve de novo. A ficha do Hub é por conexão. Sessão fechada (revogada, canal caído) sai daqui.
 */
const vivas = new Map<string, BrowserLiveSession>()
const fichas = new Map<string, BrowserHubSlot>()

export function obterSessaoAoVivo(conexao: string, sessao: string, criar: (ficha: BrowserHubSlot) => BrowserLiveSession): BrowserLiveSession {
  const chave = `${conexao}\n${sessao}`
  const atual = vivas.get(chave)
  if (atual && atual.state.status !== 'closed') return atual
  let ficha = fichas.get(conexao)
  if (!ficha) fichas.set(conexao, (ficha = new BrowserHubSlot()))
  const nova = criar(ficha)
  vivas.set(chave, nova)
  nova.subscribe({
    onState: (state) => {
      if (state.status === 'closed' && vivas.get(chave) === nova) vivas.delete(chave)
    },
    onFrame: () => {},
  })
  return nova
}
