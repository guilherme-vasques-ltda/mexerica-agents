# Marketplace: ações perigosas desligadas

O Hub filtra as ferramentas de toda conta **de ficha do catálogo** (campo `servico` preenchido), em `host/src/market/ferramentas.rs`, sem depender da tela. O filtro age em `tools/list` (o que o bot não pode usar não aparece) e em cada `tools/call` (nome de ferramenta escondida ou desconhecida é recusado como `ruleDenied`). Conta do catálogo também não roda como comando local.

- **Padrão fail-closed.** Ferramenta que escreve, envia, publica, apaga ou mexe em dinheiro, e toda ferramenta nova ou alterada pelo fornecedor, nasce **desligada por conta e por bot**. Sem lista completa vista, com ficha divergente ou vencida, nome repetido ou nome fora de ASCII seguro, nada passa.
- **Leitura automática só pela ficha.** Só passa sozinha a ferramenta cujo par nome + digest está em `curadas` no grupo ligado da ficha, sem verbo de escrita ou dinheiro no nome e sem dica destrutiva do fornecedor. O fornecedor pode restringir, nunca conceder. Nenhuma ficha traz `curadas` hoje: toda ação nasce desligada.
- **O perfil é o teto.** Com lista de ferramentas no grupo ligado, nome fora dela não existe para a conta. Grupo sem lista não deixa nada passar sozinho: a pessoa liga, uma a uma, o que quiser naquele bot.
- **Digest.** Cobre nome, título, descrição, entrada, saída e anotações; só o hash é guardado. Qualquer mudança desliga a leitura aceita e apaga a liberação da pessoa. Uma liberação vale só para o digest que a pessoa viu.
- **Inventário.** Só entra o que o Guard deixou passar. Só uma lista completa (pedido sem cursor, resposta sem `nextCursor`) substitui o inventário, numa transação. Lista parcial ou paginada e `notifications/tools/list_changed` invalidam o inventário. Cada conta tem uma geração que avança a cada invalidação e ao desconectar; a resposta de um pedido antigo não instala nada.
- **Admissão final.** A ponte remota pergunta ao Hub de novo (`validateOnly`) imediatamente antes de enviar um `tools/call`. Desligar depois disso ainda barra o envio; um pedido já admitido pode sair, e o Hub não promete cancelar o POST em andamento.
- **Só a pessoa liga**, pelo app: `marketFerramentaLigar { botId, conta, ferramenta, digest, confirmo: true }` e `marketFerramentaDesligar`; `marketFerramentas { botId, conta }` lista o estado. Os métodos de ligar não estão na ponte do bot: conteúdo de fornecedor ou de página não consegue chamá-los.
- **Apagar junto.** Tirar a conta do bot ou desconectar a conta apaga as liberações na mesma transação. Ligar de novo pede nova confirmação.
- **Recibos.** Cada liberação, desligamento e recusa gera recibo local (`mexerica_ferramenta_recibos`) só com identificadores, nome técnico da ferramenta e motivo.
- **Fora do filtro:** conta sem ficha (MCP adicionado pelo próprio usuário), decisão em aberto do produto; aplicar o mesmo padrão desligaria as ferramentas de quem já usa um conector próprio.

Tabelas SQLite: `mexerica_conta_ferramentas`, `mexerica_conta_inventario`, `mexerica_ferramenta_liberacoes` e `mexerica_ferramenta_recibos`.

Na tela (Contas do bot), a conta ligada mostra "Ações de <conta> que dependem de você": as ações desligadas, cada uma com **Ligar** (confirmação que diz que só aquele bot poderá fazê-la) ou **Desligar**. O catálogo e a ficha só listam as ações como desligadas, sem chave.
