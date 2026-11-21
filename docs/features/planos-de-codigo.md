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
  redirecionamento não é seguido.
- Na conversa, o Hub abre o OpenCode só para o bot cujo modelo é do plano (`<provedor>/<modelo>`, motor `opencode`), do
  Space pessoal e ainda existente. A chave segue por variável de ambiente, só para esse processo, num perfil próprio do
  plano (`engine_setup_root/planos/<plano>/home`): casa, configuração, dados, estado e cache só do plano. O OpenCode do
  plano não enxerga o login, as chaves nem a configuração do OpenCode do dono, e a chave nunca é escrita em arquivo.
- O OpenCode repassa o próprio ambiente aos comandos de shell que executa. Por isso o Hub escreve no perfil uma extensão
  (`.config/opencode/plugin/mexerica-shell-env.js`, gancho `shell.env`) que zera a variável da chave só para esses
  comandos; o provedor continua recebendo a chave pelo processo. Conferido com o OpenCode 1.19.6 (a versão que o Hub
  instala): sem a extensão um comando de shell via a chave, com ela não. Outras versões precisam ser reconferidas.
- Trocar ou remover a chave muda o carimbo da sessão e reinicia o processo. Sem chave, fora do Space pessoal ou no
  executor isolado, o lançamento falha: o Hub nunca cai no ambiente do dono.
- Os modelos do plano entram na escolha de modelo do OpenCode (`agentModels`), só no Space pessoal, e continuam
  disponíveis se o OpenCode não responde à descoberta dos próprios modelos.

## Mexerica Open

A versão Open recusa todos esses métodos (a lista fechada em `open_hub.rs`) e o cofre. Nada disso aparece nela.
