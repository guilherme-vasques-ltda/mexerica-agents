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

## Testes

- `motor/mexerica/test/fornecedores.test.ts`: com servidores de mentira, cada fornecedor passa pelos cenários de
  chave válida, inválida, sem permissão, resposta lenta, erro do fornecedor, resposta quebrada e modelo sem
  permissão numa conversa. Em todos, a chave não aparece na saída nem no stderr do motor.
- `host/src/fork/motor_mexerica/tests_chaves.rs`: salvar só a chave aceita, cifrada; listar e testar sem devolver
  a chave; remover; chave só para bot do Space pessoal; métodos só deste computador.
