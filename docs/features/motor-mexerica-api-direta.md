# Motor Mexerica: API direta (chaves de API)

O Motor Mexerica conversa direto com o fornecedor usando uma chave de API cadastrada pelo dono do Hub. Esta é a
parte «API direta» de Conexões de IA. O motor continua atrás de `MEXERICA_MOTOR=1`, desligado por padrão.

## Fornecedores

| Fornecedor | Conversa | Lista de modelos |
|---|---|---|
| Anthropic | Messages (`api.anthropic.com`) | `GET /v1/models` |
| OpenAI | Responses (`api.openai.com/v1`) | `GET /v1/models` |
| Google | Gemini (`generativelanguage.googleapis.com/v1beta`) | `GET /v1beta/models` |
| xAI | Chat Completions (`api.x.ai/v1`) | `GET /v1/models` |
| OpenRouter | Chat Completions (`openrouter.ai/api/v1`) | `GET /api/v1/models/user` (só o que a chave pode usar) |
| Meta | Chat Completions (`api.meta.ai/v1`) | `GET /v1/models` |

Nenhum modelo é fixo no código: a lista vem do fornecedor e o modelo escolhido é montado a partir dele
(`motor/mexerica/src/fornecedores.ts`). Modelos que não conversam (voz, imagem, vetores) ficam fora da escolha.

## Cadastro

- Os métodos `motorChaves`, `motorChaveTestar`, `motorChaveSalvar` e `motorChaveRemover` só valem para o app
  deste computador. Um aparelho pareado é recusado.
- «Testar acesso» abre o motor preso ao fornecedor e só lista os modelos. Não abre conversa nem gera custo. O
  resultado é `{ok, modelos}` ou `{ok: false, erro}`, com `erro` entre `chave_invalida`, `sem_permissao`,
  `limite`, `fornecedor_falhou`, `sem_resposta`, `resposta_invalida` e `sem_chave`. O corpo da resposta do
  fornecedor nunca é lido para montar o erro, porque ele pode repetir a chave.
- Salvar testa antes: uma chave recusada não entra no cofre. Ela fica cifrada no slot `motor-mexerica/<fornecedor>`
  e nunca volta ao app, a registro, a log nem a mensagem de erro. A lista de modelos do último teste fica guardada
  sem segredo e alimenta a escolha de modelo do bot (`agentModels` do backend `mexerica`).
- A chave do dono só é entregue a bots do Space pessoal. Isso é conferido a cada leitura do cofre. Bot de outro
  Space ou apagado não recebe chave, e o turno falha fechado.
- Remover apaga a chave e a lista. O pedido seguinte do motor já não acha a chave.

## Endereço próprio

Um serviço qualquer que fale Chat Completions, Responses ou Anthropic Messages, com endereço, chave e, quando o
serviço não lista modelos, o modelo digitado pela pessoa (método `motorEnderecoSalvar`, só deste computador).

- **Um registro só.** Chave e destino ficam juntos num registro cifrado no slot `motor-mexerica/proprio-<id>`. O índice
  dos endereços (só os ids) fica fora do cofre. O app recebe nome, endereço, formato e a opção local, nunca a chave.
- **Portão de rede no motor** (`motor/mexerica/src/destino.ts`), em cada teste e em cada conexão:
  - só HTTPS; HTTP só com a opção «serviço nesta máquina ou na minha rede»;
  - sem usuário e senha, consulta ou fragmento no endereço;
  - cada endereço que o nome resolve é conferido, em IPv4 e IPv6 (inclusive IPv4 dentro de IPv6), e o endereço da
    conexão aberta é conferido de novo;
  - rede privada, a própria máquina e CGNAT só com a opção local;
  - link-local (onde ficam os metadados de nuvem), endereços de metadados conhecidos, multicast e reservados nunca;
  - redirecionamento é erro: chave, cabeçalho e conversa não seguem para outro endereço.
- **Lista de modelos.** `GET {base}/models` nos formatos OpenAI; `GET {base}/v1/models` com `x-api-key` e
  `anthropic-version` no formato Messages. Serviço sem lista (404/405) responde `sem_lista`: o endereço só é salvo com
  o modelo digitado, no estado «acesso ainda não confirmado».
- **Estados do «Testar acesso».** Todo resultado traz `estado`:
  - `modelos_no_catalogo`: a lista veio;
  - `acesso_nao_confirmado`: o serviço não lista modelos;
  - `recusada`: o fornecedor recusou a chave;
  - `nao_verificado`: qualquer outra falha, inclusive `destino_recusado`.
- **A conversa não troca de destino sozinha.** Trocar o destino de um endereço muda a geração da credencial, e a
  conversa guardada recomeça. Trocar o modelo de um bot para um endereço próprio que ainda não recebeu aquela conversa
  também a recomeça lá: nada da conversa anterior é transferido sem consentimento.
- **Testes:** `motor/mexerica/test/endereco-proprio.test.ts` (portão de rede, os três formatos contra serviços locais
  de mentira, redirecionamento), `host/src/fork/motor_mexerica/tests_chaves.rs` (salvar só depois do teste, sem lista,
  destino recusado, o app nunca vê a chave) e `tests_link.rs` (a conversa não segue para o endereço novo).
## Entrar com OpenRouter no navegador

Na linha do OpenRouter, «Entrar com OpenRouter» é uma alternativa a colar a chave (OAuth PKCE do fornecedor). O Hub gera
o verificador (S256) e um `state` aleatório de uso único, que valem dez minutos e só existem em memória; o navegador
volta ao ouvinte local do app (`127.0.0.1`, uma das duas portas fixas), que entrega `{state, code}` ao Hub. O Hub
confere o `state`, consome a tentativa antes de qualquer rede e troca o código pela chave sem seguir redirecionamento,
com prazo e corpo limitados. A chave segue pelo mesmo caminho da chave colada: testada com a lista de modelos, só
gravada se o fornecedor aceitar, cifrada no cofre e nunca devolvida ao app. Só este computador faz o login, e só com o
motor ligado. Um `state` que não confere não derruba o login em andamento.

## Testes

- `motor/mexerica/test/fornecedores.test.ts`: com servidores de mentira, cada fornecedor passa pelos cenários de
  chave válida, inválida, sem permissão, resposta lenta, erro do fornecedor, resposta quebrada e modelo sem
  permissão numa conversa. Em todos, a chave não aparece na saída nem no stderr do motor.
- `host/src/fork/motor_mexerica/tests_chaves.rs`: salvar só a chave aceita, cifrada; listar e testar sem devolver
  a chave; remover; chave só para bot do Space pessoal; métodos só deste computador.
- `host/src/fork/motor_mexerica/tests_entrar_openrouter.rs`: URL de autorização com o desafio do vetor do RFC 7636, `state`
  errado ou expirado, uso único, troca recusada (4xx, 5xx, corpo sem chave, corpo grande, redirecionamento) sem
  vazar código nem chave, fluxo feliz até o cofre e métodos só deste computador.
- `apps/desktop/src/renderer/views/fork/contas-ia/openrouter-login.test.ts` e `apps/desktop/src/main/fork-oauth-loopback.test.ts`:
  o fluxo da tela, o cancelamento e o ouvinte aceitando `callback_url` só com o retorno exato do app.
