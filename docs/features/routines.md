# Routines

## External event consent

The local transport may explicitly record or revoke a contract against the reviewed
card revision. Local transport is not proof of a human gesture. The card remains
pending; each change advances its revision and requires a new compare-and-swap.
The private record binds Space, bot, routine, reviewed card revision, an authority
projection and an exact GitHub repository, event and typed action. That projection
excludes activation, scheduling state and errors; edits to authority fields remove
the old record transactionally. A revoked or invalidated record can be replaced by an explicit new grant with a
new ID. Deleting the routine, bot or Space removes the record transactionally.
The projection excludes the routine name and webhook key; any future admission
must verify its event credential independently, including after key rotation.
The reviewed entry revision is the revision *before* the grant, not the current one.
There is no app interface or consumer. Future admission must independently verify
the original grant, current authority, human proof, revocation and chain controls;
regranting never authorizes an old request. The webhook hook still blocks event and webhook runs.

Routines belong to an agent bot and are usually created by talking to it ("I want
a routine that…"). Every agent receives the built-in `routines` MCP server. The desktop
conversation details panel and the iPhone's **More → Routines** show a compact list
(name, schedule, on/off switch). The header's chat button puts "I want a routine
that " in the composer; **+** opens the form for doing it by hand. A row opens
the same form filled in, with delete and test run beside Save; the row's switch
pauses and resumes. Run results appear in the chat. (No desktop, **Rodar agora**
executa a rotina de verdade, com seus efeitos; não é simulação.) The form is name, instruction, a **When to run** type (Schedule or Webhook, the two
Claude Code routines also offer besides GitHub events) and a run timeout. A schedule
picks how often it repeats (Hourly, Daily, Weekdays, Weekly, or Custom for a cron
written as five fields), the time and day, and an IANA time zone chosen from a list
filtered as you type (the device's own zone by default); the host compiles the
cron, checks it and shows the summary and next run. One-off times, intervals, events and multiple
triggers are left to the bot: editing such a routine shows its current triggers as
the selected type and keeps them until Schedule or Webhook replaces them. The TUI (`n` form with a typed When: `daily 09:00`, `weekdays 09:00`,
`weekly mon 09:00`, `hourly :30`, `every 2h`, a cron or `webhook`, optionally ending
in a time zone that Tab completes; `c` asks the bot) match.
A saved webhook routine's form shows its public URL and key (copy, reveal, replace
with confirmation; TUI `w` copies a curl command and `W` replaces the
key). Creation/update notices open the routine.

## Agendamento pelas Rotinas da Mexerica

O bot usa somente o MCP `routines` para suas tarefas e recordações. Pode criar
uma tarefa própria sem pedido humano. Não escolhe outro bot nem outro
Space pelos argumentos. Editar, pausar, apagar, disparar manualmente e girar chave
continuam exigindo turno humano e vínculo vivo da sessão original. Agendamento
nativo, cron do sistema ou processo em segundo plano não são alternativas.

Toda criação MCP, inclusive a pedido do dono e pelo Berga, conta no padrão de
**10 criações por bot em 24 horas móveis**. Criação direta do dono no app não conta.
Cancelar ou apagar não devolve crédito. Tentativas repetidas pelo transporte não
duplicam tarefa, débito nem aviso. A cota persiste ao reiniciar; uma recusa não
cria tarefa e aparece no Mission Board com motivo e orientação.

Toda criação pelo bot avisa o dono na conversa principal e cria um cartão
**Manter / Cancelar** no Mission Board, com bot, rotina e programação. Silêncio
mantém a rotina ativa. Manter encerra a revisão sem executar ou reativar; Cancelar
retira a rotina e suas pendências canceláveis, preservando trabalho já admitido.
A pausa global e os demais controles de execução continuam valendo.

Exceção única: criação a partir de **conteúdo de fora**
nasce **pausada**. Na mesma transação da gravação, o Hub confere a marca
persistente da sessão visível e a da ACP exata — entrada colada, anexo, resposta,
resultado de conector ou de ferramenta não verificada, skill ou histórico externo —
e nunca texto do modelo. O MCP `routines` do próprio Hub tem identidade verificada
na definição exata; apenas `save_routine` de criação não marca a sessão. Leituras e
operações que podem devolver texto armazenado marcam exposição. A rotina é gravada pausada, o aviso aponta o Mission Board e **Rotinas**, e o
cartão pendente do Mission Board mostra a rotina pausada. Nada executa antes de o
dono liberar: agenda, webhook e eventos respeitam `Paused`; caminhos do modelo
recusam com `routine_external_release`. Ligar no cartão ou no app (`setRoutineEnabled` ou o formulário,
origem humana) libera e reagenda. Depois disso o bot segue podendo pausar e editar,
mas religar continua sendo gesto do dono. **Manter** no cartão continua sendo só
ciência e não liga a rotina. A marca é persistente e não é limpa por conteúdo
posterior; outro bot ou uma sessão nova começa sem ela. Sem a marca, vale a política
acima: nasce ativa e o silêncio a mantém. Motor sem canal de identidade (adaptador
`custom`) é tratado como ferramenta externa por construção (fail-closed): nesse
caso, as criações do bot nascem pausadas até o dono liberar.

Na gestão legada de equipes, criar ou atualizar um bot pode salvar rotinas, inclusive
habilitar uma rotina existente. Essa operação exige um turno humano válido e passa
pela aprovação de gestão; com autonomia automática e pedido claramente digitado,
a política existente pode dispensar o cartão. Ela não autoriza uma ação isolada do
modelo sem o pedido humano. Regras `mcp__routines__*` continuam usando a identidade
de servidor comprovada e o método apresentado ao porteiro; a classificação
por operação não concede nem altera permissões.

O limite por bot pertence ao cartão **Trabalho que ninguém pediu**. Como essa
tela ainda não existe nesta base, o Host conserva o campo `routineCreationLimit24h`
(padrão 10, faixa 0–1000) e RPCs de leitura/gravação exclusivos do dono,
`donnaRoutineCreationPolicy` e `donnaRoutineCreationPolicySave`, contextualizados
por Hub, Space e bot. O Board explica a indisponibilidade sem link para tela inexistente.

Filhas criadas por rotina ou pedido entre bots herdam a cadeia de execução: o orçamento
original de 50 execuções e até cinco níveis nunca reinicia. A procedência vem de
execução, binding, inflight e Run persistidos, nunca dos argumentos do modelo.
Sem prova, a criação é recusada. Reserva, avanço da agenda e Run descendente são
atômicos. Recusa de cadeia gera estado terminal visível, sem chamar IA.

Rotina, cota, aviso e cartão são confirmados na mesma transação. Revogação ou troca
de sessão antes da gravação recusa a chamada. Revisões do cartão exigem autoridade
do dono e o Space original; clientes offline ou com fotografia antiga não acionam
os botões. Dados legados são preservados e não recebem descendência inventada.

### Bloqueado tecnicamente

- **Claude Code:** lançamento e retomada negam `CronCreate`, `CronDelete`,
  `CronList`, `ScheduleWakeup`, `RemoteTrigger`, `Skill(schedule)`,
  `Skill(routines)` (alias de `schedule`) e `Skill(loop)`. As demais skills e
  as ferramentas do MCP `routines` continuam disponíveis conforme suas
  permissões. O lançamento fixa `CLAUDE_CODE_DISABLE_CRON=1`, desligando o
  scheduler local inclusive para crons duráveis antigos. Essas negações
  também valem em Liberar. O repasse de `disallowedTools` foi conferido por
  **leitura de código** no `claude-agent-acp` **0.94.0**, com SDK **0.3.303**
  (Claude Code 2.1.303); os nomes também foram conferidos no CLI local 2.1.306.
  Na mesma lista, fora do agendamento, `Workflow`, `ListAgents` e
  `ReportFindings` não são oferecidas aos bots (nenhum bot as usa; nomes
  conferidos no SDK 0.3.303). `Agent` e as skills continuam.
- **Grok Build, rota Shell:** `_meta.agentProfile.disallowedTools` remove
  `scheduler_create`, `scheduler_list` e `scheduler_delete` **só da sessão
  principal**. Os subagentes internos `general-purpose` continuam com essas
  ferramentas e compartilham o agendador da sessão pai: é uma lacuna comprovada,
  não uma garantia pendente de teste. A delegação continua disponível.
  O contrato e os IDs foram conferidos até **1.0.67** (também em 1.0.62).
  No daemon 1.0.62, a documentação informa que esse scheduler não existe e o
  perfil objeto pode ser ignorado; não generalizar para outras versões/rotas.
  O gancho exige o motor `grok` selecionado; Grok em `custom` não recebe o perfil.
  Na implantação atual, o Grok não está instalado no Hub.

É preciso **revalidar o contrato e os nomes quando a versão mudar**. O registro
pode atualizar os adaptadores automaticamente. O teste `harness_e2e` comprova o
envio de metadata e ambiente pelo Hub a um adaptador fictício; não comprova a
aplicação das negações pelo adaptador real nem sua herança nos subagentes.

### Proibido por instrução

A regra do manual proíbe alternativas por **cron/crontab, `systemd-run`, `at` e
`launchctl` pelo shell**, processos em segundo plano, agenda na nuvem do provedor
e MCPs externos. Para **Codex, Gemini e OpenCode**, a regra só acrescenta essa
instrução; não implementa uma barreira técnica de agendamento nesses motores.
Isso também vale para as vias do Claude/Grok não cobertas pelas negações acima.

**Em Pedir e Liberar, os pedidos enviados pelo motor chegam em cartão.** Se o motor executa algo sem pedir, o Hub não cria um cartão por conta própria. Cron e timers do systemd do usuário podem manter uma agenda fora
das Rotinas. A barreira técnica no chão ainda precisa ser implementada.

**Limite conhecido do Claude:** `Monitor` e tarefas com `run_in_background` podem
acordar o agente por `task-notification`, inclusive sem turno ativo do Hub.
Podem consumir assinatura sem entregar o lembrete no app. A regra não nega `Monitor`
nem fixa `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS`, pois isso também afeta comandos
longos legítimos. Usá-los como agenda continua proibido por instrução.

Se o MCP `routines` faltar, recusar ou falhar, o bot informa a limitação, não
agenda de outro jeito e não afirma que o lembrete foi criado. A regra não apaga
arquivos antigos de cron local nem cancela ou migra agendas já existentes na
nuvem do provedor.

## Tela central "Rotinas" (Mexerica)

Além da lista de cada bot, o app do Mac tem uma tela que junta as rotinas de todos
os bots, no formato da "Automações" do OpenMausBot. Abre pelo ícone de calendário no
topo da barra lateral (e no trilho compacto), ou pelo mesmo ícone na lista de rotinas
de um bot, que já abre filtrada por ele. A tela ocupa a janela; **Voltar** retorna às
conversas e o foco volta ao ícone de Rotinas. Abrir uma conversa pela ilha ou por uma
notificação fecha a tela.

- **Cabeçalho:** **Buscar rotinas** (nome da rotina, do bot ou do grupo, sem acento;
  Esc limpa; ⌘F leva a ela com a tela aberta), filtros por **grupo** e por **bot**,
  **Lista** ou **Calendário** e **+ Nova**. A busca vale para a Lista e para o
  Calendário; sem resultado, aparece "Nenhuma rotina encontrada" com "Limpar busca".
  O seletor de grupo filtra por **Todos os grupos**, por **fora de grupo** ou por um grupo do Hub.
  Colocar um bot em outro grupo muda também o filtro das rotinas dele.
  Um Hub sem suporte a grupos não mostra esse seletor. Numa janela estreita, os
  controles do cabeçalho quebram em duas linhas.
- **Calendário:** **Dia**, **Semana** (de domingo a sábado) e **Mês**, anterior/próximo
  e **Hoje**, na barra de cima do próprio painel do calendário. A grade fica nesse painel: os 7 dias
  dividem a largura (sem rolagem horizontal) e só a grade de horas rola na vertical.
  Ela mostra o fuso deste computador (ex.: GMT-3), destaca hoje e tem a linha da hora
  atual, que anda sozinha. Cada rotina é um marcador de 30 min na cor do bot;
  execuções passadas trazem ✓ ou o alerta de falha, e as próximas vezes de uma rotina
  pausada aparecem tracejadas. Rotinas no mesmo horário dividem a coluna e mostram só
  o nome. No Mês, cada dia mostra até 4 rotinas e "+N mais", que abre o Dia.
- **Faixa "Sob demanda":** rotinas só por webhook ou evento (sem horário). Na mesma
  faixa, **Frequentes** reúne as que rodam mais de 2 vezes numa mesma hora (na próxima
  semana ou no período visto), que não cabem na grade. Quando um período tem cartões
  demais (teto de 5000 do Hub), as rotinas com mais horários saem da grade só naquele
  período, com um aviso; as execuções delas continuam.
- **Barra lateral** (painel de 232 px à esquerda): minicalendário do mês e **Bots**,
  com contagem, busca por nome ou função e uma dica de arrastar. Clicar num bot filtra
  por ele. Com menos de 920 px de largura, ela recolhe num botão ("Mostrar
  minicalendário e bots") que a abre por cima da grade; Esc fecha.
- **Criar:** **+ Nova** pede o bot e abre o editor de rotina, no idioma do app. Arrastar
  um bot para um horário (ou um dia, no Mês, às 9 h) abre o editor já com aquele bot
  e "toda semana, naquele dia e hora", que o usuário ajusta antes de salvar. Clicar
  num horário vazio faz o mesmo, pedindo o bot. **Agendar**, em cada bot, usa a
  próxima hora cheia quando hoje está no período e, fora dele, as 09:00 do primeiro dia
  do período; o rótulo diz qual.
- **Teclado:** a grade tem um cursor de meia em meia hora (setas; Enter cria ali). O
  minicalendário e o Mês são um Tab cada: as setas andam entre os dias e Enter abre o
  dia (as entradas do Mês ficam fora da ordem do Tab; no Dia, cada rotina tem o seu).
  Ao abrir, o foco vai para Voltar.
- **Lista:** agendadas pela próxima vez, sob demanda e pausadas, com o bot, a agenda em
  português, o estado (próxima execução, em execução, última falhou, agenda precisa de
  atenção) e o botão de ligar/pausar. Clicar abre o editor.
- **Estados:** carregando, erro com "Tentar de novo", Hub antigo (sem o método) e vazio.
  A tela consulta o Hub a cada 5 s enquanto está aberta e só se redesenha quando algo
  muda; uma rotina com agenda inválida não derruba a tela.
- **"Onde roda":** cada rotina vem com `runsOn: null`; o selo "Roda em" já existe
  na Lista e, só com ícone, nos cartões da grade, no Mês e na faixa, desligado até o
  Hub ter computadores com esse suporte.

As descrições em português e as ocorrências vêm do Hub (`host/src/routines/calendar.rs`);
o app só filtra e desenha (`apps/desktop/src/renderer/views/rotinas-central/`). A tela, o
painel do bot e o editor de rotina do Codync seguem o idioma do app (catálogo
`rotinas.*`, datas pelo `Intl`; veja [Idiomas](../guides/idiomas.md)). As descrições da lista e
do calendário continuam em português (texto do Hub); no editor, a agenda por extenso é o
`resumo` pt-BR em pt-BR e o `summary` em inglês nos outros idiomas (`rotinas-central/editor-textos.ts`).
A expressão cron, os fusos e a URL/chave do webhook ficam sempre LTR, isolados do texto.

## Busca na barra lateral (Mexerica)

Buscar bots filtra nomes sem acento, com limpar/Esc e ⌘K/⌘F quando a barra está aberta. O seletor **Todos os times** inclui Sem time e os Times salvos no quadro, identificados por computador/Hub/Space e ID. Busca/filtro abre resultados temporariamente sem gravar recolhimento. ↓/Enter passam para a lista; setas percorrem as linhas desenhadas. Arraste/⌥↑↓ só ordena irmãos visíveis com mesmo pin, preservando Chefe primeiro e sem mudar Time. A seção Canais mantém seu fluxo existente.

A sidebar usa Times e grupos de bots; Rotinas filtra pelos grupos do Space, inclusive bots fora de grupo. Sem snapshot, a lista permanece plana. Veja [Equipes na barra lateral](times.md#equipes-na-barra-lateral) e [Grupos de bots](grupos-de-bots.md).

## Apple setup interface

The editor keeps task fields first, then a When to run type menu and its settings.
Inputs use the shared palette with a visible filled surface. New recurring schedules
default to Daily at 09:00 in the device's time zone; Custom takes a typed cron.
The host describes the schedule and shows the next run. Run timeout is under an animated Run settings
expansion. A fixed footer keeps Create routine / Save changes and inline errors
visible while the form scrolls.
The layout uses the same light/dark tokens, type family and custom controls as the
rest of Codync, with Reduce Motion support.

## Conversational setup and tools

Every agent receives the `routines` MCP server automatically; no marketplace
installation is needed. Its tools are `list_routines`, `save_routine`,
`set_routine_enabled`, `delete_routine`, `run_routine`, `duplicate_routine` and
`routine_webhook`.
The same setup guidance is included in newly rendered agent context and MCP
initialization instructions.

`duplicate_routine` accepts the source routine's `id` and creates a disabled
copy with a fresh webhook secret. A copy of a routine created from external
content needs the owner's release again. Its response and `list_routines`
include `externalReleasePending` so the caller can distinguish that state from
an ordinary pause. Internally, MCP creation passes `duplicateOf` through the
creation queue; it is not part of the advertised `save_routine` schema or a
separate public tool argument. Owner-initiated duplication uses a direct path.

For chat setup, the bot should resolve the task, trigger and timezone from the
conversation, asking only about missing or ambiguous details. It should check the
task's required tools, sign-ins and files, list existing routines to avoid
accidental duplication, then save and report the returned configuration and next
occurrence. A saved webhook whose external sender is not connected must be
reported as awaiting connection. Testing runs the actual instruction, so creating
a schedule alone should not trigger an unsolicited test execution.

A deterministic ACP fixture exercises the actual advertised MCP subprocess:
initialize, discover all six tools, reject an invalid schedule, create, read,
edit, pause/resume, fetch webhook credentials, delete, and execute a test run.
This verifies tool wiring and persisted behavior; it does not guarantee how every
provider model will interpret an ambiguous natural-language request.

## Triggers

A routine has an OR list of triggers:

- `interval`: `seconds` (1 second through 365 days).
- `once`: `at` (Unix milliseconds, in the future when created).
- `cron`: a five-field `expression` and IANA `timeZone`, including DST.
- `webhook`: identifica um gatilho de entrega autenticada; até a autoridade central de cadeia estar integrada, o Hub recusa sua execução.
- `event`: `source`, `event` (`*` matches all), and optional exact-match `filters`.
  Sua execução também fica recusada pelo mesmo gancho até existir a autoridade central de cadeia.
  `messageContains` matches a substring of `text`. Source names are `slack`,
  `github`, `origin`, `microsoftTeams`, `linear`, `sentry`, `pagerduty`, and `email`.

For example, Sunday at 14:20 in Taipei is `20 14 * * 0` with `Asia/Taipei`.
The host computes and displays the next occurrence. Calendar parsing uses
[Croner](https://docs.rs/croner/3.0.1/croner/), with chrono-tz for named zones.

## Execution

The host checks deadlines every second, even with no client connected. Definitions,
next deadlines and run records are saved in SQLite (`routines.v1`). Due triggers
are coalesced into one run per routine; only one routine per bot is dispatched at a
time. Bot actors serialize routine work with ordinary conversation work.

Uma execução fala no chat do bot: o aviso “Routine · <nome>”, o rastro e a
resposta ficam no turno reservado pelo aviso; uma mensagem recebida depois fica
no próximo turno. Ela usa uma sessão própria (`routines::session_slot`), gravada
em `lane.session.<bot>@<bot>/routine-<run>` e esquecida após salvar o desfecho.
Guardar a sessão não autoriza reenviar o prompt após reinício. Ao terminar, o
processo do agente para se não houver outra sessão aberta (exceto no Windows;
veja [processos do agente](../architecture/conversation-runtime.md)); havendo
outras sessões, fecha somente a da rotina quando o agente oferece esse recurso.
Uma agenda frequente não conserva servidores MCP por execução. O aviso e o alerta
de cada execução seguem o desfecho (abaixo). Responder ao resultado vai à sessão principal, com o resultado citado. Uma
execução de teste pode rodar uma rotina desativada. Desativar ou excluir cancela
pendências; trabalho já permitido pode terminar. Excluir um bot cancela suas
pendências. Intervalos perdidos são agrupados, sem reproduzir um acúmulo.

**Limite da assinatura:** se o motor responde que a assinatura atingiu
o limite no meio da execução, a rotina entra em **Aguardando** — o mesmo estado
da conversa direta — em vez de falhar. O aviso “Rotina «nome» aguardando o
limite da assinatura; volto às HH:MM” aparece no chat do bot. A execução
persiste pelo reinício do Hub, retoma quando o limite volta (mesmas tentativas,
intervalo e prazo máximo do estado Aguardando) e conclui uma vez só: o gatilho
não a reenfileira enquanto ela espera, e a retomada prepara uma continuação da
permissão durável original, passando de novo pelo teto de gasto. Desativar,
excluir ou cancelar encerra a espera; uma espera **pausada** (sem retomada
automática possível) também encerra a execução, porque não haveria quem a
destravasse e a vaga do bot ficaria presa. Se o limite aparecer antes da
permissão durável, a execução falha como antes. Detalhes e garantias em
[Assinatura no limite: Aguardando](assinatura-aguardando.md).

### Resultado de cada execução

Cada execução tem dois dados separados: o **ciclo de vida** (`status`, como antes) e o
**desfecho** (`outcome`), gravado pelo Hub na mesma transação que encerra a execução.

- **Sem novidades** (`noNews`): o bot declarou que verificou e não há nada a avisar.
  Nenhum aviso nem alerta.
- **Com resultado** (`reported`): o bot declarou um resultado, com resumo. Se a
  resposta final ficou no chat, só o alerta; senão, um aviso com o resumo.
- **Precisa de você** (`needsOwner`): aviso com o resumo e alerta de atenção.
- **Não conseguiu / Falhou** (`failed`): declarado pelo bot, ou observado pelo Hub
  (tempo esgotado, limite da assinatura, erro do agente, pedido para entrar na conta
  publicado, término diferente de `end_turn`, falha ao começar ou ao enfileirar).
  Aviso e alerta de falha. Falha observada sempre vence a declaração.
- **Sem confirmação** (`unconfirmed`): terminou sem declaração válida, com duas
  declarações diferentes, sem motivo de término, foi parada ou interrompida. Sem
  resposta visível no chat, um aviso curto ("A rotina «nome» terminou sem dizer como
  foi. Confira a conversa.") e **nenhum** alerta no celular; parada não
  avisa e interrupção mantém o aviso e o alerta de antes.

A declaração vem da ferramenta `report_routine_result` (`outcome`: `no_news`,
`reported`, `needs_owner`, `failed`; `summary` de até 300 caracteres, obrigatório
exceto em `no_news`). Ela só aparece no servidor `routines` da sessão da execução
(`routine-<run>`) e só é gravada se o vínculo de sessão original continuar vivo na
própria transação que grava `routines.v1`, a execução estiver em andamento e a sessão
do motor for a do slot dela (sessão repetida em outro slot é recusada como ambígua).
A declaração é por execução, não por tentativa; a mesma declaração repetida é
confirmada, uma diferente é recusada e deixa a execução sem confirmação. Ela não
passa pelo pedido do dono, não altera rotinas nem permissões. "Com resultado" e "Sem
novidades" são a avaliação do bot, não prova de efeito externo. `(pass)`, resposta
vazia ou `end_turn` sozinhos nunca viram "Sem novidades".

`outcome` sobrevive à limpeza da outbox (`result`) e ao reinício; publicar de novo não
duplica (aviso `routine-result-<run>`). `status` legado: `needsOwner` e `failed`
declarados usam `failed`, para o cliente antigo nunca mostrar sucesso; "Sem
confirmação" continua `succeeded` no legado. O app atual mostra o desfecho na lista,
no calendário, no painel do bot e pela API (`routines`, `list_routines`,
`routinesOverview`); execuções antigas aparecem como "Terminou", sem ✓. App antigo e
iPhone leem só `status`: para eles "Sem confirmação" ainda parece sucesso. Dado de
desfecho ilegível não impede o Hub de carregar as rotinas e aparece como sem
confirmação. Motor sem MCP, servidor `routines` indisponível ou bot em Space terminam
sem confirmação, salvo falha observada.

Pendências de Rotinas sobrevivem ao reinício. O ator reserva ao chegar à frente
na fila, com a instrução salva atual; reserva não autoriza envio. No produtor
de admissão por bot, permissão durável sem conclusão torna a execução
interrompida, inclusive se ainda estava `Starting`. Ela não volta a `Pending` nem
gera prompt automático. Reserva sem permissão continua elegível. Execuções
legadas ainda podem ser classificadas como `recovering` pelo armazenamento;
isso não autoriza o Actor a enviar um prompt. O fallback sem checkpoint e
token comprovados encerra localmente com aviso, mesmo havendo sessão ACP gravada.
A retomada comprovada conserva o token de execução e usa uma nova cerca de
tentativa; permissão incerta durante retry fica pausada para conferência explícita.
Resultado durável concluído pode ser publicado sem IA. Nenhum desses caminhos
garante efeitos externos exatamente uma vez: confira o histórico antes de tentar
de novo.

### Tentativas duráveis de Rotinas

O controle de cadeia desta etapa está ligado às ocorrências de relógio e aos testes
manuais, incluindo a retomada interna da mesma execução. O pedido começa na
etapa 1 e na tentativa 1. O produtor admite até cinco níveis e 50 execuções na
mesma cadeia; continuá-la no mesmo nível consome nova execução, delegar aumenta
o nível. `ask_bot`/`message_bot` com a execução de Sessões são
consumidores desse produtor: num turno de Rotina, o repasse continua a cadeia da
Run; os canais entre bots seguem voláteis (ver colaboração entre bots). A continuação
genérica depois de configurar conexão/login ainda aguarda essa integração.
Não há cobertura global 5/50 nem liberação de webhooks locais/públicos.

Internamente, `donna_chain_roots` e `donna_chain_admissions` usam o mesmo Store,
sem fila nova. Origem é enum fechado; intenção é digest SHA-256 canônico, sem
payload/instrução brutos. Replay confere rotina, origem, conversa, bot, tipo e
predecessor. A ocorrência de agenda fixa geração monotônica e o primeiro par
horário devido/índice antes de avançar a agenda. A primeira transação reserva o
orçamento e o Run ID; a segunda grava `routines.v1` e o vínculo juntos. Repetir
após falha entre elas devolve o Run ID original. Uma reserva sem Run não executa
IA, continua gasta e gera um único aviso na conversa; pode ser reconciliada pela
mesma ocorrência. A decisão e a publicação desse aviso conservam a trava do
estado de Rotinas e revalidam `linked=0`; durante uma mudança em andamento, o
publisher adia para o próximo tick, sem classificar a janela normal entre commits
como falha. Estado inconsistente por poison não gera aviso. O estado técnico
`_chain` não aparece nas RPCs de Rotinas.

O turno do aviso permanece o mesmo nas retomadas; ele não identifica tentativa.
A continuação herda o escopo e a rotina persistidos no predecessor, conferidos
contra a raiz pelo SQL da admissão; conserva o mesmo bot, Run e Space. Trocar
ou omitir o Space já fixado é recusado antes de incrementar o contador. A variante de
delegação não autoriza, por si, atravessar bot ou contexto: depende de contrato
próprio do consumidor futuro.
Cada despacho tem recibo persistido, com CAS de reservado para envio possível
antes do `session/prompt`. Retry SQL reutiliza a reserva; uma nova retomada IA
usa outra tentativa na mesma cadeia. Não há atomicidade SQLite→ACP: queda depois
da marca pré-envio conserva a incerteza e o gasto. A recuperação existente só
continua em sessão salva e precisa de nova admissão; sem sessão, interrompe.
Repetir a publicação do resultado salvo não chama IA nem altera o contador.

Após atualização ou reinício, execuções antigas já iniciadas ou em recuperação
sem recibos confiáveis podem permanecer em `Recovering`, exibido como
**Retomando** ou **Retomando após reinício**. Esse estado ainda não confirma um
saldo de tentativas nem permite nova chamada à IA. Ao preparar a chamada para
uma dessas execuções, o Hub confirma **Interrompida** e o motivo no histórico
antes de qualquer envio. Sem sessão salva, encerra sem enviar. Se falhar ao
salvar a preparação, mostra erro e tenta salvar a mesma preparação novamente,
com a execução reservada; isso não repete IA ou efeitos externos. Excluir o bot
com sucesso confirma **Cancelada** antes de parar o bot; uma exclusão que falha
antes de confirmar conserva o estado anterior. Não se inventa contagem passada
nem orçamento novo, e não há retomada automática da execução antiga sem recibo.
Pendentes comprovadamente não iniciadas aderem pela identidade da Run; histórico
encerrado permanece histórico. Reinício, retenção das últimas 1.000 Runs e edição
de rotina não apagam o orçamento de pedidos já admitidos. A pausa mantém o padrão atual:
pendentes aguardam; trabalho já reservado ou em recuperação pode continuar.
Rotinas continuam sem reserva nova de modelo. Não recrie o mesmo pedido para
escapar do limite; confira o histórico preservado.

Os testes focais cobrem as fronteiras 5/6 e 50/51, corrida em 49,
replay divergente, StoreBusy, falha entre commits, órfã visível, migração legada e
nova tentativa com o mesmo turno do aviso; sua presença no código não comprova aprovação.

Each run has a timeout (default one hour, configurable from 1 to 86400 seconds).
A hung agent is terminated so subsequent work can proceed. A run's outcome is
saved before it is reported, and a failure notice has a stable transcript ID, so a
restart never reports it twice. Failed reporting retries without rerunning the agent.
The latest 1000 terminal runs are retained globally, plus active/unpublished runs;
the panel returns the latest 200 runs per bot. Deduplication lasts while the
corresponding run record is retained.

Intervals retain their original cadence; missed deadlines coalesce into one
pending wake, including when a prior run is still active. Enabled time schedules
keep the computer from idle sleep. Explicit sleep, closing the lid and powering
off still suspend local execution. The host uses an exclusive data-directory lock
to prevent two daemons from firing the same schedules. Shutdown stops scheduling
before stopping actors.

## Webhook delivery

Every webhook routine has a key and two addresses (`routineWebhook`): the public
`{cloud}/v1/hooks/{computerId}/{routineId}` through the configured Mexerica cloud (null while the
cloud is off) and the local `http://localhost:<port>/hooks/routines/<id>`. Both run
the same checks (`host/src/routines/hooks.rs`); the protocol is
the numbered sections of the remote relay protocol.

- **Authentication**: `Authorization: Bearer <key>`, or `X-Hub-Signature-256` (an
  HMAC-SHA256 of the body with the key). For GitHub: payload URL = the public URL,
  content type `application/json`, secret = the key. `routineWebhook` with
  `rotate: true` replaces the key; the old one stops working at once, including in
  the cloud.
- **Events**: a JSON object is the event as-is; a GitHub delivery becomes
  `{source:"github", event:"<event>.<action>", repo, sender, text, url, payload}`
  (GitHub `ping` succeeds without running); other bodies become `{text}`. Event
  triggers filter on these fields, for example:

```json
{"source":"github","event":"pull_request.opened","repo":"owner/repo","text":"A PR opened"}
```

- **Entrega pública**: a nuvem guarda a chave da rotina no relay; o Worker usa essa chave para autenticar e vê o corpo pelo tempo necessário para validar e cifrar. Só o Hub tem a chave privada para abrir o envelope e confere novamente a credencial. Corpo de até 64 KiB; cabeçalhos permitidos também têm limite. A nuvem não persiste nem registra o corpo e não mantém fila de eventos.
- **Indisponibilidade**: Hub desligado, desconectado ou antigo sem capacidade cifrada recebe 503 com `Retry-After`. Sem tentativa nova do remetente, o evento se perde. Os limites são 30 tentativas por minuto e 2.000 por dia por usuário, somando seus Hubs, e 10.000 por dia global, com virada diária em UTC. Excesso retorna 429 com `Retry-After`; contadores e metadados mínimos não contêm o evento.
- **Repetição**: use o mesmo `X-Delivery-Id`, `X-GitHub-Delivery` ou `Idempotency-Key` em cada tentativa. O Hub deduplica com os registros de execução ainda retidos. Se o ACK se perde após o Hub aceitar, o resultado é incerto; reenviar sem ID estável pode executar novamente. ACK significa aceitação, não término da rotina.
- **Legado**: a antiga fila é removida logicamente quando cada relay acorda; isso não comprova apagamento físico ou de backups. O endpoint local mantém autenticação própria.

O controle de cadeia estabelece para cadeias um máximo de cinco níveis e 50 execuções, sem repetir um gatilho, com motivo visível quando houver bloqueio. O controle do percurso interno de Rotinas não fornece, sozinho, autoridade de origem e propagação aos webhooks; o limite de entregas e o identificador de entrega não implementam essa proteção. Dados e cabeçalhos externos não concedem autoridade sobre a cadeia. Até a integração da autoridade central, o gancho no Hub bloqueia a execução de webhooks locais e públicos, com motivo no histórico da rotina; o público responde 503 com Retry-After. A integração da política completa no Hub é requisito antes de ativar a entrega.

Codync does not create provider subscriptions: the user (or the bot, with the user)
pastes the URL and key into the sending service. Do not describe an event routine as
connected before a real delivery has arrived.

## Tarefas próprias de todos os bots

A agenda interna nasce disponível sem plugin, interruptor ou concessão por bot.
Cada bot, inclusive o Berga, pode criar rotinas e recordações (`once`) para si pelo MCP `routines` quando o
usuário inicia o turno. Turnos de rotina, webhook, outro bot ou mensagens geradas
pelo host não autorizam mudanças nas rotinas da principal. Pela gestão do time,
a mesma restrição se aplica às rotinas dos outros bots. As ações do usuário na
API continuam disponíveis mesmo quando uma recuperação pendente falha.

## API

All normal methods use the existing authenticated host API / authorized E2E channel:

| Method | Body |
|---|---|
| `routines` | `botId` |
| `routineSchedule` | `triggers` + preferred `timeZone` to load, or `draft` to preview; returns hydrated form, validated triggers, summary, warning, next run, and (Mexerica) `resumo`, the schedule in pt-BR |
| `saveRoutine` | `botId`, optional `id`, `name`, `instruction`, `schedule` form or `triggers`, optional `enabled`, `timeoutSeconds` |
| `duplicateRoutine` | `botId`, `id`. Creates a disabled copy (`"<name> (cópia)"`) with a fresh webhook secret and no inherited runs or errors. Returns `{routine}` with `externalReleasePending`. Rejects deleted routines or routines belonging to another bot (`routine not found`), and enforces the 1,000-routine limit. Like `saveRoutine`, it requires owner authorization (`Dono`); devices without it, the bridge, and executor channels are denied. |
| `setRoutineEnabled` | `botId`, `id`, `enabled` |
| `deleteRoutine` | `botId`, `id` |
| `runRoutine` | `botId`, `id`. Se o banco estiver ocupado por outra tarefa do Hub naquele instante (`StoreBusy`), a chamada espera até ~2 s e repete a reserva manual, que é idempotente; depois disso, devolve o mesmo `core_StoreBusy`. |
| `routineWebhook` | `botId`, `id`, optional `rotate`; returns `url` (public or null), `localUrl`, `key`, `connected` |
| `routinesOverview` | `from`, `to` (ms, até 50 dias), `timeZone` (IANA de quem vê); devolve `now`, `teams` (`Pessoal` com todos os bots agentes), `routines` (o formato de `routines` mais `summary` em pt-BR, `teamId` e `runsOn: null`), `occurrences` (`routineId`, `botId`, `at`, `trigger`, `enabled`) em `[max(from, agora), to)`, `dense` (mais de 2 vezes numa mesma hora), `truncated` (fora da grade neste período pelo teto de 5000 cartões, as de mais horários primeiro), `invalid` (agenda que não pôde ser projetada) e `runs` criadas no intervalo mais as ativas, sem `event` nem `result`. Só leitura; Mexerica. |
| `routineTeams` | Sem argumentos; devolve `teams` (`id`, `name`, `botIds`), os mesmos de `routinesOverview`. Só leitura; compatibilidade com apps anteriores ao suporte a grupos. O app atual usa `botGroups` para os filtros de grupo. |

The local-only `routineCall` backs the MCP tools. Each MCP instance is scoped to
its bot. Phone and desktop panels refresh while visible; definitions do not currently
participate in offline client caches. Run transcript entries use normal rev/SSE.

## Reference and parity

Observed on the installed Grok Bot on 2026-09-26: a routine list in conversation
details, instruction and trigger details, pause/resume, delete, and chat-based
editing. Creation produces a linked transcript card. The shipped renderer also
contains test-run and webhook credential controls, interval/one-shot/cron schedule
formatting and multiple provider trigger types.

The historical reconstruction examined during the upstream research contains a
partial automation fire consumer with event matching, durable wake delivery and
completion reporting. Most automation modules it imports are absent. Its renderer
and private cloud are not source-complete references.

Remaining differences from the reference: provider subscription provisioning,
cloud execution while the host is offline and inactivity auto-pause. A entrega não fica em fila: o remetente recebe 503 com `Retry-After` e o evento se perde se ele não tentar novamente com ID estável. O gancho de segurança também recusa execuções de webhook sem criar `Run` até o controle central de cadeia estar disponível. The Apple UI
uses Codync's shared native controls. This is not a verified complete one-to-one
reconstruction of every Grok Bot routine behavior.

## Validation

Os testes executados incluem `host/tests/routines_e2e.rs` (agendamento, credenciais, autenticação, pausa global, recusa do gancho sem `Run` e execução manual), `host/tests/memory_e2e.rs` (recusa e isolamento de memória em rotina manual) e os testes do receptor do relay no Host (envelope cifrado, autenticação repetida, recusa e aviso). Vetores cruzados TS–Rust cobrem cifra, destino e adulteração. O produtor de cadeia cobre o percurso interno de Rotinas descrito acima; a cobertura dos demais ingressos depende dos respectivos consumidores, sem promessa de proteção global.

## Public webhook ingress without a fixed IP

Research checked on 2026-09-26: [OpenClaw](https://docs.openclaw.ai/cli/webhooks)
uses Tailscale Funnel and the [Hermes webhook adapter](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/messaging/webhooks.md)
needs a reachable server URL. Codync instead reuses the host's outbound relay socket
(above): no fixed IP, open port or per-user tunnel, mas o evento se perde se o Hub estiver desligado e o remetente não tentar novamente. A user's own Cloudflare Tunnel to the local endpoint also
works — it delivers the request, but local execution stays blocked until the
chain-authority integration above — and needs their own account, domain and
`cloudflared`, and drops deliveries while the computer is off.

## Usability comparison (2026-09-27)

The installed Grok Bot was inspected through Computer Use. Its sidebar emphasizes
the routine name and a human-readable schedule (for example, Every Sunday at
2:20 PM). Details expose Instruction, When to run, Pause, Edit and Delete. The
conversation contains a linked creation notice; an existing edit draft refers to
the routine by name. No reference routine was run, paused or deleted during this
comparison.

Codync retains that compact sidebar and the chat-based setup path, while providing
a direct editor: a light Repeat choice with the time and day for the common
schedules, and Custom for a cron typed as written. Existing duration intervals keep their cadence until
explicitly replaced. Editing hydrates the original expression, timezone and interval
exactly; compound and provider-event triggers retain a lossless Keep existing
triggers fallback. Expired one-shot timestamps are preserved when editing other fields.

Saved definitions appear immediately. Paused schedules omit next-run deadlines.
List rows and details expose queued/running/recovery/failure states.
Edit with bot includes the routine ID in a draft without sending it.

## Cron scheduling contract

All newly configured recurring schedules in the editors use `cron` and run through
the host's existing Croner scheduler. The editors send the Repeat choice as
`calendarStyle` (`hourly`, `daily`, `weekdays`, `weekly`) with the hour, minute and
weekday, or `custom` with the typed expression, and the host compiles it. A loaded
schedule in a style the editors don't offer (every few minutes, several days,
monthly) opens as Custom with its cron unchanged; switching to Custom starts from
the cron the host compiled (empty while the host is still checking the last change).
Cron generation, parsing, validation, timezone checks, descriptions and next-run
calculation all live in `host/src/routines/editor.rs` and `schedule.rs`. The Swift
draft is only a Codable form DTO. The editor debounces preview requests, discards
stale responses, shows host errors and disables Save until the current draft is
validated. Save sends the form to the host, which independently compiles and
validates it again. Both direct trigger API calls and Bot MCP calls use the same host
scheduler. Run timeout text is also parsed and validated by the host.
The built-in agent instructions also select cron for recurring clock schedules.

Existing elapsed-duration intervals remain readable/editable without silently
changing their execution times. An interval anchored to creation time cannot be
converted to a wall-clock cron without changing its cadence. One-time schedules
retain their absolute timestamp: five-field cron has no year or run-count guard.
Webhook/event triggers remain event-driven. These paths are intentionally not
misrepresented as cron. The host must still be running and awake.

Rust regression tests cover default cron creation, preset round trips, arbitrary
minute selection, timezone validation, custom expression preservation, existing
interval and expired one-shot preservation, and next occurrences across month
boundaries. An isolated HTTP integration test previews, saves and reloads the same
form and verifies direct saves reject invalid drafts. Swift tests cover wire DTO
decoding and millisecond date binding; there is no Swift cron implementation.

### Pausa global das rotinas

Na tela Rotinas, a pausa vale para todo o Hub, inclusive outros espaços e filtros. Ela impede novos disparos agendados, manuais e por webhook ou evento. Pedidos externos recusados não ficam guardados. Execuções pendentes já aceitas aguardam; uma execução já reservada ou em recuperação pode continuar, assim como a publicação de resultados. Conversas e outros efeitos não são interrompidos. O estado sobrevive ao reinício e não altera o interruptor individual de cada rotina. Na lista e no calendário, previsões ficam identificadas como suspensas enquanto a pausa global está confirmada; na troca de Hub ou quando o estado estiver indisponível, elas não são anunciadas como próximas execuções ativas.

Ao retomar, agendas recorrentes calculam o próximo horário a partir de então.
Uma execução única vencida roda uma vez; uma já consumida não é rearmada e uma
pendência existente não é duplicada. A pausa global não concede permissão de
recuperação: no produtor de admissão por bot, despacho permitido sem conclusão fica interrompido e não
repete prompt. `Recovering` legado também não dispensa checkpoint e token
comprovados no Actor. Publicar resultado durável continua possível sem IA;
retomar a agenda não resolve a incerteza de efeitos nem autoriza replay.

No disparo manual pelo MCP, o Hub conserva a execução e a mensagem humana
originais também ao admitir a cadeia. Raiz, recibo, rotina e fila são confirmados
na mesma transação, somente depois da guarda da origem. Troca de sessão,
revogação ou falha de gravação não deixa reserva órfã, tentativa consumida, fila
ou aviso parcial. Repetir uma entrega conserva seu recibo/idempotência e exige
a mesma autoridade ainda válida; retry não concede autorização nem cria outro
pedido para contornar o limite de 50. App e agendador mantêm seus produtores e
suas próprias regras de admissão/recuperação.
