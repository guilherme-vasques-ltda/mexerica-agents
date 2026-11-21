# Bot collaboration

Create two or more bots normally, giving each a clear name, description, project folder and agent. Then message one:

> Ask Reviewer to inspect the changes in /path/to/project. Have it report bugs without editing files, then summarize its findings for me.

Every bot receives the built-in `team` MCP server. `list_bots` returns the other visible bots' IDs, descriptions, agents, folders and status. `ask_bot(botId, message)` sends a self-contained request and waits for the recipient's final text reply. The requesting bot incorporates that answer into its response to you. `message_bot(botId, message)` queues a request and returns immediately; the recipient reports to you in its own chat. The same tools work across ACP backends that support MCP servers.

The host records each exchange as one notice in both chats. Every client shows it as a compact row (centered in the apps, one ` · ` line in the terminal UI), "Messaged [avatar] Owen" in the sender's chat and "Message from [avatar] Egan" in the recipient's, with no request text. In a chat a run of consecutive exchanges with the same peer, in either direction, is one row, "3 messages with [avatar] Owen"; anything else the chat shows (a message, another notice, a card, an exchange with another peer) ends the run, trace entries do not, and the run is marked failed if any exchange failed. Only a failure is marked (danger color and a warning mark). Tapping the row opens a read-only "Egan ⇄ Owen" conversation sheet with the pair's recent history (the newest 200 notices, plus what the live chat already holds): author-labelled grey bubbles for each request and reply, time separators, "Waiting for Owen…" or the failure detail under a pending or failed exchange. It is laid out like the Full conversation sheet, with the pair as its title and an X to close. Tool details also remain in the trace. There is no additional team setup screen; a lead bot can set up the team itself (see Team management).

## Alcance do controle de tentativas

O produtor da cadeia está ligado ao percurso interno de Rotinas e, para
`ask_bot` e `message_bot` chamados com a execução de Sessões
(`ExecutionRef::Acp`), que é o caso de toda chamada MCP real:

- O turno que chama abre uma raiz no nível 1, uma por execução: pedidos irmãos
  do mesmo turno dividem a mesma raiz e o mesmo orçamento. Turno de Rotina não
  abre raiz nova: continua a cadeia do recibo real da Run.
- Cada repasse soma um nível e uma execução. O recibo viaja com o pedido até o
  turno do destinatário; os repasses dele continuam a mesma cadeia. Outro bot,
  nova chamada ou a fila não zeram o contador.
- Limites: 5 níveis e 50 execuções por cadeia, persistidos no Hub e conservados
  após reinício. A conferência acontece na mesma transação que grava os dois
  avisos e reserva o turno do destinatário; no limite, a ferramenta recusa com
  motivo claro e nada é publicado, enfileirado ou contado.
- Abaixo do limite, o comportamento das ferramentas não muda. O controle legado
  de oito saltos em memória continua valendo em conjunto.

Chamada interna sem execução e sem cadeia herdada continua só com os oito
saltos. A continuação genérica após configurar conexão/login ainda pode perder
a origem da rotina. Não há trava global de todas as entradas, recuperação
automática, Stop por execução ou Stop por webhook liberado por esta etapa.

## Registro durável de pedidos e conclusões

Cada `ask_bot`/`message_bot` com `ExecutionRef::Acp`
grava um pedido em `donna_session_requests` e, quando o turno do destinatário
termina, uma conclusão em `donna_session_results` (serviço `fork/retornos`,
contrato Retornos v1). O registro guarda só
metadados: modo (`wait`/`async`), solicitante (bot, chat e `SessionRef` resolvidos
por Sessões), proveniência da execução (binding e digest do token, sem o token),
alvo, recibo da cadeia, IDs dos dois avisos, tentativa, cerca, estado, desfecho e
motivo tipados e a referência da entrada `agent` final quando ela existe na sessão
do destinatário. Nenhum texto de pedido, resposta, título ou erro é copiado.

- O pedido é gravado na mesma transação da admissão, dos dois avisos e da
  reserva de turno; falha desfaz tudo e não consome execução extra.
- O alvo é a sessão que o percurso atual usa: a principal do destinatário. Não há
  sessão raiz nova nem `parent_session` entre bots.
- `running` é gravado antes da permissão e do `session/prompt`. A conclusão vem da
  resposta final do `session/prompt` e é gravada antes do retorno ao `ask_bot` e dos
  avisos finais. Desfechos: `completed`, `failed`, `stopped`, `incomplete`; sem
  proveniência do produtor, o conteúdo fica `unverified`, nunca sucesso.
- Cancelamento do solicitante, prazo, parada antes de executar e falha de início
  encerram só pedidos abertos; conclusão tardia de tentativa vencida não altera o
  pedido. Encerrar a execução solicitante não cancela `message_bot`.
- Se a conclusão não puder ser gravada, a ferramenta/aviso mostra falha em vez do
  resultado; o pedido fica aberto e o próximo reinício o marca como incerto.
- No reinício, pedido `accepted` vira `interrupted/restart_before_start` e
  `running` vira `interrupted/restart_uncertain`. Nada é reenviado, nenhum turno
  novo é aberto e a reserva não é devolvida.

Registro durável não é entrega nem consumo: não há publicação `coord-result-*`,
leitura pelo modelo, continuação com IA, Pontas soltas, audiência nova nem retorno
do Berga como recepção. Chamada interna sem `ExecutionRef` continua só no canal
vivo, sem registro.

## Checkpoint de permissão delegada

Quando um turno delegado registrado pede permissão, o Hub grava o cartão existente e
um vínculo com pedido, tentativa, cerca, execução, Space e revisão do cartão na mesma
transação. Uma cerca vencida ou gravação malsucedida não publica o cartão. Pedidos
simultâneos de permissão da mesma execução geram cartões e vínculos distintos. Pedidos
internos sem registro durável ou cerca são recusados com `delegated_permission_unrecorded`;
nenhum vínculo é inventado.

O vínculo é apenas um registro interno: não altera o estado dos pedidos registrados, não cria
uma fila nova, não retoma a execução após reinício e não muda o protocolo dos apps.

## Consulta do estado do próprio pedido

O
MCP `team` oferece `bot_request_status(requestId)`, só leitura (`readOnlyHint`). O bot
que fez um `ask_bot`/`message_bot` consulta o estado registrado com o
`requestId` devolvido no aceite, na mesma conversa em que pediu.

- Alcance: Pessoal, conversa própria do bot (principal ou adicional), durante a
  execução ACP atual. A identidade vem de Sessões, conferida na mesma transação da
  leitura e revalidada (vínculo, runtime e token do turno) antes da resposta. Um turno
  novo da mesma sessão lê pedidos de turnos anteriores; a execução antiga não precisa
  estar viva. Conversa de Space, canal, bot fora do Pessoal ou chamada sem execução
  são recusados, sem fallback à principal.
- Bot, chat e `SessionRef` solicitantes precisam coincidir com o chamador, no filtro
  SQL antes de qualquer projeção. A principal não lê pedidos da adicional, nem o
  inverso, nem uma adicional de outra; outro bot também não. Pedido alheio e ID
  inexistente recebem a mesma recusa.
- Resposta fechada: `requestId`, `state`, `reason` (motivo de `cancelled`/
  `interrupted`), `outcome` (`result` e `reason` de `finished`) e uma nota fixa do Hub.
  Nunca texto, título, prompt, erro, artefato, IDs de avisos ou da entrada final,
  classe de conteúdo, digest, binding, token, produtor, alvo ou transcrito.
- Significado: `accepted` prova só que o envio não começou; `running` diz que o envio
  é possível sem conclusão registrada, sem provar bot ocupado ou efeito; `completed`
  é o fim do turno do destinatário, não entrega, consumo ou cumprimento do combinado;
  `failed`, `stopped` e `incomplete` nunca viram sucesso; `restart_uncertain` continua
  incerto. Versão, estado ou combinação desconhecidos falham fechados.
- Sem efeito: não grava, não enfileira, não acorda o destinatário, não abre turno ou
  sessão, não reenvia e não altera pedidos, resultados, contadores ou reservas.
  Sem polling automático.

A consulta não devolve a resposta nem a publica em outra audiência. Publicação,
consumo, recuperação do corpo após reinício e repetição pela mesma intenção
continuam planejados no contrato Retornos v1.
Quando `ask_bot` falha depois da admissão (prazo, cancelamento, parada ou falha do
destinatário), o erro devolvido ao modelo termina com `(requestId: <id>)`, o mesmo ID
do registro, para a consulta acima. Só o texto do erro muda: o registro, os avisos,
a ordem de decisão e conclusão e a resposta de sucesso ficam iguais. Recusa
antes da admissão (limite da cadeia, alvo inválido, mensagem vazia) não grava pedido e não traz ID.

## Cadeia própria nos Times

O MCP `team` oferece `my_team_chain`, sem argumentos (argumento extra é recusado),
`readOnlyHint` e `idempotentHint`. O bot vê só um nível acima (`reportsTo`: dono como
"Você (o dono)", bot visível com id e nome, ou `hidden`) e os bots visíveis que
reportam direto a ele (`directReports`; ocultos só em `hiddenDirectReports`), mais
`role` e o nome da própria área (`team`). Mesmo Space do `list_bots`, mesma leitura
do quadro Times, Tx descartada e revalidação da execução antes de responder. Reporta
a não concede poder: mandar mensagem continua por `ask_bot`/`message_bot` e o limite
da cadeia. Detalhes em [Times](times.md).

## Notice data

New bot-to-bot notices retain all fields written by the team tools in the base:
`text`, `heading`, `style`, `status`, `delegationId`, `sourceBotId` and `targetBotId`.
They add one optional structured field:

```json
{
  "botMessage": {
    "sourceBotId": "egan",
    "targetBotId": "owen",
    "text": "Review this change",
    "reply": "Looks good",
    "detail": "Failure or cancellation detail, when applicable",
    "contract": {"result": "Três opções", "acceptance": "Fontes citadas", "scope": "…", "limits": "…", "rule": "…"}
  }
}
```

`contract` is present only when the requesting bot proposed one; only
`result` is required. Both notices carry the same object, and the recipient's prompt
receives it escaped in a `<contrato_da_tarefa>` block after the request
(`docs/features/delegacao.md`). The durable record does not change.

`text` is the original request. Completed asks add `reply` (at most 4,000 bytes);
failures and cancellations add `detail`. The existing notice `status` determines
whether the exchange is pending, completed, failed or cancelled. The base notice
heading and rendered text remain unchanged for clients that display plain notices.
Clients read the structured data directly; they do not parse display strings.

Existing notices remain ordinary notices. There is no startup rewrite, data
migration or transcript-cache reset. The existing restart cleanup still marks
unfinished requests failed and adds the failure detail to their structured data,
when present. It never replays work.

`botConversation {botId, peerId}` returns `{entries}`: the newest 200 main-chat
notices containing structured messages with that peer, oldest first. It requires
the Control scope. Clients merge these with the live chat (the higher `rev` wins)
and keep the result in the sheet; it is never added to the chat mirror, so main
history paging is unaffected.

## Ask execution

- Each bot still owns an ACP process (its main session, plus one per routine run) and runs one turn at a time. A busy recipient queues the request. Different bots can work concurrently.
- Requests have their own reply channel and never merge with user messages or another request. Contiguous user messages keep the existing batching behavior.
- An ask's final reply is addressed to the requesting bot. It appears in the request notice
  and the recipient's trace, without a second bubble in the recipient's main chat. The recipient
  returns the answer directly rather than sending another `message_bot` request back.
- The recipient uses its own folder, tools, memory and permission policy. Permission cards appear in its chat. The requesting bot's entire transcript is not copied; only the request is passed. Requests are not treated as user facts by the memory keeper.
- The host rejects self-delegation, duplicate outstanding requests to the same recipient, and direct or indirect wait cycles, including queued requests. Up to 64 requests can be outstanding host-wide.
- Native Claude Code/Codex subagents remain managed by the harness. Codync does not turn them into permanent bots or override their delegation settings.

## Ask stops, failures and restart

- The wait limit is ten minutes, including queue and approval time. Failure is a tool error, not a fabricated successful reply. `ask_bot` is not automatically retried.
- Stopping the requesting bot cancels its outgoing requests. A queued request is removed; a running delegated turn's ACP process is stopped. Unrelated queued messages on the recipient remain. Stopping the recipient also releases its requesters once its turn stops.
- Deleting either bot cancels affected requests. Recipient startup failures, crashes, refusals and empty replies surface to the requester.
- Pending request notices are persisted with IDs and statuses. After host restart they become interrupted errors; delegated turns are not automatically replayed. Check partial work before retrying. A recuperação humana segue o produtor de admissão: permissão nova incerta não dispara replay; só o inflight legado mantém a recuperação histórica aplicável.
- Completion push notifications come from the requesting bot; delegated replies do not send a second “done” push. Recipient permission requests still use the usual “needs you” notifications.

`ask_bot` uses synchronous request/reply over MCP; neither tool adds a task scheduler or isolated worktrees. Give bots explicit file ownership when they share a working directory. Rebuild and restart the host to load the new built-in MCP server.

## Independent messages

`message_bot(botId, message)` queues a bot request and returns immediately, without waiting for a reply. The recipient reports to you in its own chat. Messages use the same queue as asks, run one turn at a time, and never merge with user messages or other requests. Both chats show bot-attributed notices rather than user messages. Up to 64 messages can be outstanding host-wide, with at most 16 from any one sender, separately from asks. Queued and running messages both count. A slot is freed when the recipient finishes, fails or cancels the request, or admission fails. Stopping, restarting or deleting the sender does not free slots for its already accepted messages.

The recipient can use `send_message` for user-facing updates and its report. If it sends none,
its final text becomes the main-chat report. The request notice shows completion
status; it does not copy this user-facing report into a bot reply.

Each independent request includes reporting guidance, even when it reuses a
session that previously handled an ask. The ask's reply restrictions apply
only to that ask; later user turns and independent requests can report in
the recipient's chat normally.

Accepted messages survive sender completion, Stop, disconnection and deletion. Recipient Stop or deletion cancels queued messages and stops running work; started work may have left partial changes. Outcomes update both notices; failures mark them as failed. The recipient uses its normal completion notifications. Messages have no ask timeout or automatic retry and are not replayed after host restart.

Bot request chains can make at most eight handoffs. Both `message_bot` and
`ask_bot` carry the count into the recipient's turn, including queued work;
finishing an earlier request does not reset it. This bounds repeated A → B → A
messages even when few requests are outstanding. At the limit, the tool reports
an error without queuing more work. A new user turn starts a fresh allowance.
Calls made with the session's execution are also bound by the persisted
chain limit (five levels, 50 executions), which usually stops first; see the
section above.

## Gerenciamento do time

Só o bot principal (Berga), identificado por `primaryBotId` na configuração local,
pode ter **Manage the team** ligado e receber o MCP `manage`. O usuário escolhe a
principal e a autonomia geral pela API local autenticada; sem principal, ninguém
gerencia. Não se infere autoridade pelo nome.

- `list_team`: todos os bots agente, inclusive a própria Berga (`you: true`) e os ocultos, com perfil e rotinas.
- `create_bot {name, instructions?, folder?, routines?}`: prefere Codex disponível no Hub (fallback só se indisponível ou com logout confirmado), sem modelo escolhido e com `manageTeam=false`. Herda só skills do Berga, sem conectores, contas de ferramenta, logins ou computador, e a autonomia mais restritiva entre criadora e geral. O limite continua em 20 bots criados por bot, excluindo apagados.
- `update_bot {botId, name?, instructions?, hidden?, routines?}`: altera outros bots; a principal é recusada, inclusive por ela mesma. Uma rotina com `id` substitui uma rotina do alvo. Modelo, opções, harness, avatar, autonomia, poder de gestão e exclusão ficam exclusivamente com o usuário. Nenhum bot altera a si mesmo: `set_name`/`set_instructions` são recusadas e não aparecem no MCP. A principal não recebe nomeação automática tardia. A trava de alvo igual à origem também vale para futuros gestores autorizados.

Criar e alterar exige turno iniciado pelo usuário. Rotinas, webhooks, pedidos de
outro bot, mensagens automáticas do host e retomadas automáticas não autorizam
essas mudanças. A mesma origem é exigida para o Berga alterar suas próprias
rotinas pelo MCP `routines`; ela pode criá-las a pedido do usuário no turno dele. Reduzir a autonomia
geral para **Pedir** restringe também os bots criados por bots sem escolha explícita do usuário; voltar para
**Liberar** recalcula o modo pela autonomia atual da criadora. O controle
**Autonomia geral**, em Ajustes → General no desktop, salva **Pedir / Liberar**
por `setTeamSettings`, inclusive nos hosts por SSH. Abrir o store preserva o modo salvo.

Em **Pedir**, o card síncrono mostra os campos, rotinas e instruções antes da
mudança, com Allow/Deny, limite de 2.000 unidades UTF-16 e espera de 270 segundos.
Recusa, cancelamento ou timeout antes da aprovação não muda nada. Em **Liberar**,
não há card: cada mudança gera aviso no chat principal, com **Desfazer** no desktop.
O snapshot privado persiste no SQLite e o registro anterior à mutação recupera
avisos interrompidos. A recuperação registra erros e coloca registros inválidos
em quarentena, sem impedir o início ou ações do usuário. Falhas transitórias
mantêm a pendência e seu snapshot para nova tentativa, sem absorver edições
posteriores. O snapshot é confirmado junto do perfil e das rotinas. Desfazer
reserva o estado sem turno até confirmar a transação; o ator relê e adota o
perfil restaurado ao reservar Working, antes de compor o prompt. Uma criação cujo aviso
falhou é desfeita na mesma chamada, com cancelamento das suas rotinas pendentes.
Se só a publicação do aviso de uma atualização falhar, a ferramenta retorna
sucesso e informa que o aviso será publicado na recuperação. O perfil salvo permanece.
Depois de iniciada a gravação, cancelar a chamada não libera a exclusão nem
interrompe a finalização do worker.

Desfazer restaura o perfil e as rotinas anteriores em uma transação, ou apaga um
bot criado ainda sem sessão/conversa. O alvo deve estar sem turno em andamento
(ocioso ou em erro); conflitos preservam edições posteriores. Campos de execução das rotinas (`nextRuns`, `lastError`,
`updatedAt`) não são conflitos de edição, nem depois do Allow. Conectores, skills
e ordenação alterados por outras partes
do host não bloqueiam Desfazer nem são revertidos. Atualizações salvam perfil e lote
de rotinas na mesma transação antes de avisar o ator; ator parado lê a configuração
salva ao iniciar. Falha de gravação não produz aviso de sucesso. A exclusão normal
permanece uma ação do usuário.

A API do usuário configura `workspaceRoot`, com padrão `~/Donnabot/workspaces`.
A ferramenta `create_bot` só aceita subpastas dessa raiz, com links resolvidos,
sem reutilizar a pasta exata da principal nem qualquer subpasta dos dados da
Mexerica, mesmo com `workspaceRoot` ancestral. Subpastas
da raiz são permitidas mesmo quando ela fica dentro da pasta da principal.
Sem `folder`, cria uma subpasta própria nessa raiz. Flags legadas de `manageTeam`
nos demais bots são desligadas no início, de forma idempotente. A troca da
principal atualiza as ferramentas da antiga e da nova mesmo sem mudança no perfil.

Hub e bots rodam no mesmo usuário.
As regras acima valem para as ferramentas. Um bot com shell pode ler o token
local em `~/.donnabot/token` e chamar a API diretamente; o gerenciamento do time não isola esse acesso.

`createdBy` continua sendo um registro do host, exibido no perfil. Clientes não
podem reescrevê-lo. Configuração e detalhes operacionais estão no
[manual do harness](manual-do-harness.md).

## Code and verification

- `host/src/chat/team.rs`: discovery, request lifetime and persisted notices and structured conversation data.
- `host/src/store/entries.rs` (`bot_conversation`, `expire_pending`) and `host/src/api/bots.rs` (`botConversation`): the pair history and restart cleanup.
- `host/src/tui/app/exchange.rs`, `manage/bot_chat.rs`, `view/bot_chat.rs`: the terminal client's row logic, keys and conversation sheet.
- `host/src/chat/team/requests.rs`: admission, wait graph, capacity, chain hop limit and the turn's chain receipt.
- `host/src/fork/cadeias/equipe.rs` (called from `host/src/store/team_admission.rs`): chain root per execution and handoff admission in the notices transaction.
- `host/src/fork/retornos/`: durable request and completion records (metadata only), written by the admission, the recipient actor and the startup reconciliation; `consulta.rs` is the read-only `bot_request_status` for the requester's own conversation; `pendentes.rs` is the canonical read-only listing of durable requests, used by the owner's Mission Board section “Pedidos entre bots”.
- `host/src/agent/bot/` (`queue.rs`, `turn.rs`): queue boundaries, recipient execution, completion and targeted cancellation.
- `host/src/mcp.rs`: the authenticated local MCP → HTTP bridge.
- `host/tests/team_e2e.rs`: a real host and MCP subprocess, with scripted ACP agents, covering discovery → delegation → recipient approval → reply → client sync. No paid provider calls.
- `host/src/chat/manage.rs` (+ `Cmd::Approve` in `agent/bot/updates.rs`): team management; `host/tests/manage_e2e.rs` walks an allowed `create_bot`, a denied `update_bot`, and cards that end when the turn stops or the agent cancels the call, through the real `manage` MCP process.

Run `cargo test` and `cargo clippy --all-targets -- -D warnings` in `host/`.

## Fronteira interna de despacho

As descrições de `ask_bot` e `message_bot` distinguem aceite/reserva de envio ao
motor. Ambos passam pelo produtor interno de permissão durável do ator; isso
não torna seus canais duráveis. O registro durável acima guarda pedido e término, não
o canal. Prazos e cancelamentos originais continuam. O
controle público Pausar próximos permanece indisponível; a recusa antes do
aceite de novos pedidos com bot pausado e o consumo pelo app/Voz ficam para
uma etapa futura. Não há RPC de pausa para bots.


## Privacidade do status entre Spaces

`list_bots` conserva a seleção de bots visíveis no mesmo Space e passa a usar o
status da projeção comum BotJSON. `unavailable` significa que o runtime não tem
audiência comprovada nesse contexto; não afirma livre, espera, falha, conclusão
nem quantidade de trabalhos. Não revela conversa, Space de destino ou horários
ocultos. Não amplia colaboração, acesso, memória ou poder de gestão; não use o
shell ou outro contexto para recuperar o dado indisponível.

## Resumo do dia pela cadeia

O resumo do dia sobe pela cadeia de Times
**sem** `ask_bot`/`message_bot`. O Hub guarda o resumo de cada bot (tabela
`donna_daily_summaries`, por bot e dia) e o entrega ao de cima na vez dele, que não é
acordado fora da hora. Cada execução é uma cadeia própria (nível 1, ordinal 1),
isolada, sem ferramentas e sem memória, na assinatura Claude do próprio bot. Não sobe
aprovações nem distribui trabalho e não envia mensagem a conversa alguma.

### Equipes dos bots visíveis

`list_bots` mostra a equipe de cada bot visível e quem é o chefe; use para responder
sobre as equipes. `team` é o caminho da equipe no Quadro deste Space, por exemplo
`Youtube › Roteiros`, ou `null` sem equipe. `teamChief` é `true` apenas quando
`managerBotId` dessa equipe aponta para esse bot; função de gerente não basta.
A hierarquia de reportes de cada bot continua só pela própria cadeia (`my_team_chain`).
A lista conserva os outros bots visíveis do mesmo Space: não inclui o próprio bot,
bots ocultos, bots de outro Space nem equipes sem bot visível na lista. A leitura
usa a projeção do Quadro em transação descartada, sem gravação ou mudança de revisão.
Não acrescenta rotas, reportes alheios, configuração, conta, memória, pasta ou sessão
à resposta existente e não concede poder novo.
