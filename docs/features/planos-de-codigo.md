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
- `planoCodigoSalvar { plano, chave }`: testa e só então grava a chave no cofre; com chave já salva no plano, recusa
  sem tocar em nada do que está salvo.
- `planoCodigoRemover { plano }`: recusa («plano_chave_ja_salva: Trocar ou remover a chave do plano ainda não está
  disponível nesta versão.») sem apagar nada.

O resultado do teste é `{ok: true, estado: "modelos_no_catalogo", modelos}` ou `{ok: false, erro, estado}`. `erro` usa os
mesmos códigos do teste das chaves de API (`chave_invalida`, `sem_permissao`, `limite`, `fornecedor_falhou`,
`sem_resposta`, `resposta_invalida`, `sem_chave`), escolhidos só pelo status HTTP: o corpo de um erro nunca é lido, porque o
fornecedor pode repetir a chave nele. `estado` é `recusada` (o fornecedor disse que a chave não vale) ou
`nao_verificado` (falha que não diz nada sobre a chave). A lista de modelos prova que o fornecedor respondeu à
chave, não que há saldo ou permissão de conversar com cada modelo.

## Onde a chave vive

- A chave fica cifrada no cofre do Hub, no slot `plano-codigo/<plano>`. O Hub não a devolve ao app, nem a registra, nem a
  põe em mensagem de erro. O endereço do teste é fixo no catálogo (https, do fornecedor) e o pedido não aceita endereço; o
  redirecionamento não é seguido. Uma resposta de sucesso que repete a chave é recusada, não é devolvida nem guardada.
- O OpenCode não recebe a chave. O Hub mantém um repasse local (`host/src/fork/planos_codigo/repasse.rs`): um servidor
  só em loopback, em porta efêmera. A cada sessão o Hub cria um código aleatório (`mxs_…`) e o entrega ao OpenCode em
  `OPENCODE_API_KEY`, junto com o endereço do repasse no lugar do endereço do fornecedor (`OPENCODE_CONFIG_CONTENT`).
  O repasse confere o código, troca-o pela chave guardada e encaminha o pedido ao endereço fixo do plano.
- O repasse só aceita o método e os caminhos que o fornecedor usa (uma tabela fixa por plano), nunca segue
  redirecionamento, limita tamanho e tempo de cada pedido (a vaga do teto de pedidos só volta no fim da resposta) e não
  responde a `OPTIONS`. Só atende `Host` igual a `127.0.0.1:<porta da sessão>` e recusa pedido com `Origin`: uma página
  da web ou um nome apontado para o loopback não chega ao código. Cabeçalhos e streaming passam sem alteração.
- O repasse recusa resposta que contenha a chave no corpo, nos cabeçalhos ou nos campos que «Testar acesso» guarda,
  inclusive partida entre dois pedaços, com escapes de JSON (`\uXXXX`, misturados ou não) ou em base64 (alfabeto padrão
  e de URL). Limite: uma chave partida em eventos separados do fluxo (o cliente junta os pedaços) ou escrita em outra
  codificação (hexadecimal, rot13, invertida) não é reconhecida; isto fecha o eco acidental e o disfarce comum, não um
  fornecedor que queira vazar a chave.
- Por isso a chave não está no ambiente do OpenCode, nem no de um servidor MCP filho, nem no de um comando de shell:
  eles só enxergam o código de sessão. Conferido com o OpenCode 1.19.6 de verdade e um fornecedor de mentira
  (`repasse_tests.rs`, teste marcado `ignored` porque precisa do programa instalado:
  `cargo test -- --ignored opencode_de_verdade`): as sondas leem só `OPENCODE_API_KEY` e contam as linhas do ambiente
  que contêm a chave (zero). A sonda do próprio processo do OpenCode lê `/proc` e só roda em Linux; nos outros sistemas
  a prova cobre o shell e o MCP filho. Outras versões precisam ser reconferidas.
- O código de sessão é mascarado (`mxs_[oculto]`) onde texto do motor sai do processo do Hub: cada texto e cada nome
  de campo do JSON já interpretado (escapes desfeitos, mensagens de erro inclusive) e a linha de saída que não é JSON
  (só para o log de debug), o texto de um segmento de conversa montado de
  vários pedaços (o fim que ainda pode ser o começo de um código fica retido até o próximo pedaço) e a saída de
  ferramentas. Limite: um código que o motor escreva de outro jeito que não o literal (por exemplo em base64) não é
  reconhecido; ele vale só enquanto o processo dura.
- Os planos de código só funcionam em Linux e macOS nesta versão: no Windows o salvamento e a abertura recusam com
  mensagem própria.
- O OpenCode do plano roda num perfil próprio (`engine_setup_root/planos/<plano>/home`: casa, configuração, dados,
  estado e cache só do plano), com a configuração de projeto desligada (`OPENCODE_DISABLE_PROJECT_CONFIG`). Um bot com
  comando próprio de execução não recebe o ambiente do plano: o lançamento falha. Sem chave, fora do Space pessoal ou
  no executor isolado, também falha: o Hub nunca cai no ambiente do dono.
- Trocar ou remover a chave do plano ainda não está disponível nesta versão: com chave salva, colar outra ou remover
  responde isso e não altera o que está salvo.
- O código de sessão morre com a sessão: quando a concessão sai de escopo (o processo do OpenCode sai), o código é
  revogado e os pedidos em andamento são cortados, também a leitura do corpo — e a revogação nunca se perde, fica
  marcada para quem chegar depois.
- O código vive no ambiente do processo, então vive com ele: quando a última conversa do processo fecha, o processo é
  encerrado (a próxima conversa abre outro, com outro código). Uma exceção: um processo mantido vivo por um comando de
  fundo da própria ferramenta (um servidor de desenvolvimento, por exemplo) segue até esse comando acabar.
- Limites: uma ferramenta que consiga roubar o código de sessão pode usar o plano enquanto o processo durar, só por
  este computador; o código não leva a chave. O repasse escuta em loopback numa porta TCP, que outros usuários do mesmo
  sistema conseguem alcançar, mas sem o código (que só está no ambiente do processo) não passam da porta. Um socket de
  domínio Unix, com a identidade do par, ficaria como passo seguinte.
- Os modelos do plano entram na escolha de modelo do OpenCode (`agentModels`), só no Space pessoal, e continuam
  disponíveis se o OpenCode não responde à descoberta dos próprios modelos.

## Mexerica Open

A versão Open recusa todos esses métodos (a lista fechada em `open_hub.rs`) e o cofre. Nada disso aparece nela.
