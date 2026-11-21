# Planos de código pelo OpenCode (chave colada)

Em Conexões de IA, o grupo «Planos» liga um plano de código com a chave que a pessoa cola. O motor é o OpenCode, que o
Hub já conhece como motor de bot. Cada plano é um item do catálogo em `host/src/fork/planos_codigo/mod.rs`; a tela
«Adicionar conexão» só lista o que o método `planosCodigo` devolve.

## Planos

| Plano | Provedor no OpenCode | Variável da chave | Lista de modelos |
|---|---|---|---|
| OpenCode Go | `opencode-go` | `OPENCODE_API_KEY` | `GET https://opencode.ai/inference/go/v1/models` com a chave |

O OpenCode Go é feito para o OpenCode e outros agentes de código, para uso próprio da conta. O uso além do limite do plano
pode consumir o saldo da conta no OpenCode: o item traz o aviso `saldo_extra` para a tela mostrar.

## Métodos (só deste computador)

- `planosCodigo`: a lista de planos, com `configurada`, `programaInstalado`, os modelos do último teste e `avisos`.
- `planoCodigoTestar { plano, chave? }`: lê a lista de modelos. Não abre conversa e não gera custo.
- `planoCodigoSalvar { plano, chave }`: testa e só então grava a chave no cofre.
- `planoCodigoRemover { plano }`: apaga a chave, a lista de modelos e o perfil do plano.

O resultado do teste é `{ok: true, estado: "modelos_no_catalogo", modelos}` ou `{ok: false, erro, estado}`. `erro` usa os
mesmos códigos do teste das chaves de API (`chave_invalida`, `sem_permissao`, `limite`, `fornecedor_falhou`,
`sem_resposta`, `resposta_invalida`, `sem_chave`), escolhidos só pelo status HTTP: o corpo de um erro nunca é lido, porque o
fornecedor pode repetir a chave nele. `estado` é `recusada` (o fornecedor disse que a chave não vale) ou
`nao_verificado` (falha que não diz nada sobre a chave). A lista de modelos prova que o fornecedor respondeu à
chave, não que há saldo ou permissão de conversar com cada modelo.

## Onde a chave vive

- A chave fica cifrada no cofre do Hub, no slot `plano-codigo/<plano>`. Nunca volta ao app, a registro, a log nem a
  mensagem de erro. O endereço do teste é fixo no catálogo (https, do fornecedor) e o pedido não aceita endereço; o
  redirecionamento não é seguido. Uma resposta de sucesso que repete a chave é recusada, não é devolvida nem guardada.
- O OpenCode nunca recebe a chave. O Hub mantém um repasse local (`host/src/fork/planos_codigo/repasse.rs`): um servidor
  só em loopback, em porta efêmera. A cada sessão o Hub cria um código aleatório (`mxs_…`) e o entrega ao OpenCode em
  `OPENCODE_API_KEY`, junto com o endereço do repasse no lugar do endereço do fornecedor (`OPENCODE_CONFIG_CONTENT`).
  O repasse confere o código, troca-o pela chave guardada e encaminha o pedido ao endereço fixo do plano.
- O repasse só aceita o método e os caminhos que o fornecedor usa (uma tabela fixa por plano), nunca segue
  redirecionamento, limita tamanho e tempo de cada pedido, e recusa resposta que contenha a chave (no corpo, inclusive
  partida entre dois pedaços, ou em cabeçalho). Cabeçalhos e streaming passam sem alteração.
- Por isso a chave não está no ambiente do OpenCode, nem no de um servidor MCP filho, nem no de um comando de shell:
  eles só enxergam o código de sessão. Conferido com o OpenCode 1.19.6 de verdade e um fornecedor de mentira
  (`repasse_tests.rs`, roda onde o programa existe). Outras versões precisam ser reconferidas.
- O OpenCode do plano roda num perfil próprio (`engine_setup_root/planos/<plano>/home`: casa, configuração, dados,
  estado e cache só do plano), com a configuração de projeto desligada (`OPENCODE_DISABLE_PROJECT_CONFIG`). Um bot com
  comando próprio de execução não recebe o ambiente do plano: o lançamento falha. Sem chave, fora do Space pessoal ou
  no executor isolado, também falha: o Hub nunca cai no ambiente do dono.
- Trocar ou remover a chave revoga o código na hora, corta os pedidos em andamento e encerra o processo antigo. Uma
  geração por plano fecha a corrida com uma abertura que estava em curso.
- Limite: uma ferramenta que consiga roubar o código de sessão pode usar o plano enquanto a sessão durar, só por este
  computador. O código não leva a chave e deixa de valer ao fim da sessão.
- Os modelos do plano entram na escolha de modelo do OpenCode (`agentModels`), só no Space pessoal, e continuam
  disponíveis se o OpenCode não responde à descoberta dos próprios modelos.

## Mexerica Open

A versão Open recusa todos esses métodos (a lista fechada em `open_hub.rs`) e o cofre. Nada disso aparece nela.
