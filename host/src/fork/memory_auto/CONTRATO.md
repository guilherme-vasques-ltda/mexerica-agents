# Contrato da memória automática do Hub

Descreve o comportamento normativo do código; o histórico fica no git. Módulos: `host/src/fork/memory_auto/`;
extrator: `host/src/fork/background_memory_auto.rs`; despertar: `host/src/fork/memory/dream_worker.rs`;
API: `host/src/chat/memory/api.rs`; interface: `apps/desktop/src/renderer/views/fork/memoria-auto/`.

## 1. O que é e o que não é

- Depois de um turno concluído, grava lembranças duráveis sobre o dono, a partir de mensagens humanas
  do pedido. O modelo só propõe um plano; quem decide e grava é o Hub. Cada gravação tem recibo para desfazer.
- O Hub é a fonte de verdade (estado, origem, natureza, fila, persistência). A interface só apresenta.
- Não é fala do dono por padrão. Só vira "Você disse" com fala literal comprovada ou edição humana explícita.
- Não concede autoridade, não aceita resposta do bot como prova e não é histórico de chat nem sessão do motor.
- Não grava segredo. Texto recusado pelo scanner só volta com liberação explícita do dono.

## 2. Formatos

### Origem (`origin`)

`{schema:1, kind: user|conversation|external|unknown, sources:[{messageId, kind, detail?}], external: bool}`

- Derivada pelo Hub (`origin.rs`). O modelo não envia origem.
- Sem origem conhecida: `unknown`. Valor padrão (`Origin::default`): `kind: unknown`, `external: true`.

### Natureza (`nature`)

`said | inferred | conflict | unknown | confirmed`

- `said`: só com fala literal validada (`origin::literal_owner`) ou edição humana explícita.
- `inferred`: fato novo não literal (padrão).
- `conflict`: versão ativa anterior permanece; a alternativa vai em `conflictText`.
- `confirmed`: o dono aprovou sugestão que o Hub achou sustentada por mensagem dele. Nunca "o dono disse".
- Dica do modelo aceita só `inferred` ou `conflict`. `natureBot` é emitido pelo Hub, nunca pelo modelo.

### Plano do extrator (`ExtractionPlan`)

```json
{"operations":[{"action":"remember","text":"…","shared":false,"evidence":["<messageId>"],"nature":"inferred"}]}
```

- Ações do extrator: `remember` (`text`, `shared`), `revise` (`entry`, `expectedRev`, `text`),
  `archive` (`entry`, `expectedRev`), `note` (`file`, `content`, `shared`, `expectedHash`) e
  `notDurable` (só `evidence`).
- `confirm` e `reclassify` são exclusivos do Hub. Vindos do extrator, são ignorados sem falhar o lote, e o pedido
  fica com prova `unproven`. Nas propostas MCP, a chamada é recusada com erro (`memory_confirm_is_hub_only`).
- Campos desconhecidos são recusados (`deny_unknown_fields`). Toda operação cita `evidence`.
- Limites: 32 operações por plano do extrator; 64 somando propostas MCP do pedido; entrada de texto
  até 96.000 bytes; prompt mais entrada até 128.000 bytes; saída até 128.000 bytes (acima: `OutputLimit`, sem nova tentativa).

### Entrada do extrator (`ExtractionInput`)

`{jobId, botId, chatId, spaceId, memoryId, scopeRevision, messages:[{id, revision, fromUser, text, origin}], existing:[Entry], notes:[{file, content}]}`

Texto e memória existente são redigidos (`codync_mem::redact`) antes de sair do Hub.

### Dados no Entry (`data.memory`)

Chaves gravadas pelo Hub: `action, text, shared, entry, rev, previous, previousOrigin, previousRev,
previousAt, previousNature, previousNatureBot, previousConflictText, origin, nature, natureBot, conflictText,
at, message, messageIds, responseIds, closingTurn, status, version, jobId, confirmation`.

- O bloco começa com os campos da própria operação, que variam por ação (`expectedRev`, `expectedHash`, `file`,
  `content`, `evidence`, `nature` etc.). O Hub depois sobrescreve as chaves acima com os valores gravados.

- ID do aviso: `memory-auto-<jobId>-<hash da operação>`, com status `done`.
- `previous*` é snapshot do Hub. Data `0` = "data não registrada".

### Não salvos (`memory.nonSaved`)

Até 50 itens, do mais recente ao mais antigo, sem paginação. Campos: `id, jobId, reason, status, at,
messageIds, responseIds, availability`. As referências são só IDs do próprio escopo: sem texto, hash,
alvo de duplicata nem bot de destino. São ocorrências passadas, não o estado atual da fila.

| reason | status | availability |
| --- | --- | --- |
| duplicate, wrongBot, notDurable | notSaved | null |
| technicalDeferred | deferred | transient |
| noSelection | deferred | noSelection |
| full | notSaved | full |
| expired | notSaved | expired |
| ineligible | notSaved | ineligible |

`wrongBot` não tem produtor nesta versão.

## 3. API

- Leitura: `memory` (inclui `nonSaved` e `memoryRev`) e `memoryFile`.
- Escrita com autoridade do dono (`WRITES` em `api.rs`): `saveMemory`, `pinMemory`, `archiveMemory`,
  `forgetMemory`, `clearMemory`, `undoDream`, `answerMemory`, `undoMemory`, `saveMemoryFile`,
  `deleteMemoryFile`, `restoreMemoryFile`, `releaseMemoryRefusal`.
- Sugestões: `approveMemorySuggestion` e `discardMemorySuggestion` (`{botId, id}`).
- `undoMemory` com `entryId` iniciado em `memory-auto-` vai ao desfazer do memory_auto. Resultado `undone`,
  ou `changed` quando houve edição posterior (preservada). Repetir devolve o mesmo estado.
- `releaseMemoryRefusal` (`{id, messageId, confirm?, botId?, spaceId?}`): sem `confirm` devolve a prévia;
  com `confirm: true` grava pela escrita normal, dispensando só o veredito do scanner.
- MCP: `memoryCall` com `__tools` para anunciar ações. A variável `DONNABOT_MEMORY_TRANSPORT` carrega uma
  capability UUID. Mutações exigem sessão vinculada.

## 4. Invariantes e guardas

- **Autoria**: evidência só de mensagem do dono (`is_user_authored`, sem `sourceBotId` nem `runId`),
  conferida na captura, no claim e na aplicação. Evidência precisa ser mensagem humana do pedido.
- **Sessão exposta**: se a sessão viu conteúdo externo, fala literal não basta (`origin::exposed`).
- **CAS**: `revise` e `archive` exigem `expectedRev`; `note` exige `expectedHash`. Divergência gera
  `memory_revision_changed` ou `memory_note_changed`, sem efeito.
- **Duplicata**: fato que a camada já tem não gera versão nem aviso; vira `duplicate` sem revelar a memória existente.
- **Esquecido**: fato apagado pelo dono (`forgotten`) não volta pela mesma mensagem de evidência: a guarda casa a
  identidade lexical do texto com o ID da evidência. Com outra evidência, o mesmo fato pode voltar. Outro fato da
  mesma mensagem não é bloqueado. Fato bloqueado não vira `duplicate`.
- **Segredo**: recusado antes de qualquer pendência (`check::secrets`). Nunca liberável.
- **Scanner de injeção** (`check::content`, sobre o texto integral): recusa por operação. Ela grava recibo sem texto
  nem hash e uma pendência com o texto para o dono ler, e é ignorada. As demais operações do pedido seguem, e o
  pedido termina `applied` ou `noChange` (com prova `unproven`), não `ineligible`.
- **Liberação**: exige mensagem humana posterior à recusa e origem não externa. A operação é reaplicada pela
  escrita normal, então vale o CAS dela, por tipo: `revise` e `archive` exigem `expectedRev` igual à revisão
  atual; `note` exige `expectedHash`; a revisão capturada na recusa é conferida só para nota; `remember` não tem
  CAS de revisão (vale dedup e conflito). Segredo nunca é liberado. A pendência de texto expira em 30 dias e é apagada.
- **Recibos de recusa e liberação**: seguem a retenção da auditoria `non_saved` (`audit.rs`): 30 dias (`LIFE_MS`),
  teto de 100 por bot + Space e de 1.000 por Hub, mais antigos primeiro; depois do prazo, o teto de todos os pares
  vale antes do teto do Hub. Recibo que sai leva a pendência e as liberações dele; o id fica numa tumba (só id,
  bot, Space e marca `shared`, sem texto nem hash) com a mesma retenção, e a recusa vencida responde
  `memory_refusal_expired`. Banco antigo migra em SAVEPOINT; sem prova de que é pessoal, o registro vira
  compartilhado. A exclusão do bot e o `clearMemory` apagam tudo antes do prazo.
- **Commit**: resultado e admissão saem no mesmo checkpoint. Aviso e recibo só existem depois do commit.
- **Falha ou cancelamento**: pedido `ineligible` e propostas descartadas. Pedido `running` interrompido no arranque vira `failed`/`ineligible`.
- **Idempotência**: ID do recibo determinístico (job e operação). Desfazer repetido não duplica.
- **Retenção**: pedido por 30 dias desde a admissão; fila de 100 por bot/Space e 1000 no Hub (`waiting` e `reserved`);
  reserva sem conclusão recuperada após 120 s; não salvos com 100 por bot/Space, 1000 no total e 30 dias.
- **Modo** (`memory_learning::Mode`): `auto` (padrão), `suggest` (fila de sugestões para aprovação) e `off` (sem pedido, sem envio ao modelo).

## 5. Worker, adaptador e despertar

1. `begin`, antes de qualquer execução do turno: captura só IDs e revisões já persistidos das mensagens humanas.
   ID do pedido = hash do contexto; idempotente pelo token do turno.
2. `checkpoint` (Completing), ao concluir o turno: grava `closingTurn` e só então decide a admissão: `waiting`
   (`noSelection`), `notQueued` (`full`) ou `ineligible` (turno elegível concluído com falha, cancelamento ou bloqueio).
3. `reserve`: pega o `waiting` mais antigo do chat, com token de reserva.
4. `prepare_guarded`: valida snapshot e turnos elegíveis, autoriza, redige, deriva origem e lê a memória existente.
5. Adaptador: envia o prompt fixo ao modelo, renova a reserva a cada 30 s e reconfere a seleção antes do envio e antes de aplicar.
6. `complete_guarded`: valida o plano, grava em transação, cria recibos e avisos.

Disposição das falhas:

- `Transient` (SQL, executor ocupado, cota, prazo, plano inválido): `waiting`, ocorrência `technicalDeferred`.
- `NoSelection` (sem conta, seleção, política ou versão): `waiting`, ocorrência `noSelection`.
- `Rejected` (snapshot inválido, `OutputLimit`, recusa propagada do pedido inteiro): `ineligible`. Não é a recusa por operação do scanner.
- `LearningOff`: `waiting` sem ocorrência.

`noSelection` significa que a seleção do extrator não produziu operações aplicáveis. Não quer dizer que nada foi
chamado nem escrito: propostas MCP podem ter sido aplicadas antes da seleção, e a revalidação da seleção pode
falhar depois da chamada ao modelo. Nos dois casos o pedido permanece na fila. A tentativa do extrator registra
a ocorrência `noSelection`; caminhos sem tentativa (checkpoint, ou propostas MCP já aplicadas) deixam o pedido
em `waiting` sem ocorrência. Não há sucesso simulado.

**Despertar**: turno com `end_turn`, pedido sem erro, parada nem bloqueio envia `DreamEvent::Wake` ao dreamer
(`agent/bot/waiting.rs`). O dreamer reavalia a fila em Wake, Flush e na inicialização.
Esperas:

- erro comum: 10 s, 20 s, 40 s, 80 s e 160 s no teto (5 s × 2^falhas);
- limite de assinatura: 15 min, 30 min e 60 min no teto;
- sem seleção (`Later`): de 5 s a 60 s, pelo `poll_after_ms`;
- executor ocupado (`Busy`): 5 s fixos.

A reavaliação periódica de 60 s é sondagem, só quando não há outra espera agendada. Ela não limita o backoff
de erro nem o de assinatura. Wake não encurta a espera de `Busy`.

**Sono**: depois de 30 min parado, o bot tem uma rodada por dia de calendário, puxada pelo dreamer, com fila
própria. Consulta o modelo (`sleep_model::ask`) e cede à extração.

## 6. Limites da interface

- Só apresenta o que o Hub envia. Não deriva autoria, origem nem natureza, e não restaura texto ou origem localmente.
- O desfazer mostra o estado confirmado pelo Hub. Timeout é resultado desconhecido: o cliente relê o `entryId`.
- O aviso se ancora só em `responseIds` explícitos do mesmo chat e turno. Sem alvo, fica em posição neutra, sem fallback por ordem.
- Legado pendente não tem pergunta nem aceitar/recusar.
- Não salvos é só leitura. Campo ausente = histórico indisponível; erro = resultado desconhecido, nunca lista vazia;
  enum desconhecido = linha omitida. Não mostra IDs, referências nem texto recusado.
- Fixtures de demonstração não provam Hub, executor nem modelo.

Fora deste contrato: recall, nomeação de conversa e o painel de bots de fundo.
