# Context and memory

How a bot keeps its instructions, context and long-term memory. Codync owns memory; the ACP harness owns the
conversation and compaction. The design, including what is still to come, is in
[bot memory and compaction](../reference/memoria/bot-memory.md).

## Transcript, context and memory

| | Where | Owner | Lifetime |
|---|---|---|---|
| Transcript | SQLite `entries` | Codync | One persistent chat per bot; replies quote the message they answer |
| Model context | ACP session (`bots.session_id`) | The harness | Until *New session*, a failed resume, or an agent/command/folder change |
| Long-term memory | SQLite `memory_*` tables | Codync: the user, the bots' memory tools, dreaming | Forever, across sessions |

Codync never replays the transcript into a session. Each turn sends only the new message(s); the harness keeps
its own context and compacts it itself.

## Instruction snapshot (`host/src/chat/context.rs`)

- The bot's instructions are rendered once, in English and kept general: profile (name, description), how to
  message the user, other conversations, workspace, skills, plus the memory section. How to use each tool
  lives with the tool (MCP server instructions, tool descriptions), not here. They're stored as a snapshot in `kv` under `context.<bot>`, keyed by **session + compaction
  epoch**, and don't change until one of those changes. This keeps the prompt byte-identical so the prompt
  cache stays warm.
- **Mexerica / Claude and Codex:** the stable identity/manual is delivered once through the native channel;
  the snapshot carries only the changing profile and Codync memory, in `<bot-profile>` on new and after
  compaction; loads retain context and announce only actual profile changes. See [manual do harness](manual-do-harness.md) for the exact fields and configuration.
- **Other harnesses** keep ACP `systemPrompt` append when advertised, otherwise the first-message profile.
- **Compaction epoch:** completed `compaction_update` or a sufficient fall in `usage_update.used` re-renders the
  profile. Claude and Codex receive it on the next message without changing their stable manual.
- **Profile edits** (name, description, skills, connectors, computer and team management) reach the next message
  once as `<agent_profile_update>`. Renaming preserves the session; changing the tool set reloads a resumable
  process without erasing main or routine session IDs.

## Memory (`host/src/chat/memory/`, crate `host/crates/memory`)

Na adicional do próprio Space, `memory` usa o namespace original da identidade,
com sessão e transcript próprios. O compartilhamento efetivo é o da principal: a
pasta gerenciada da adicional não o amplia, e revogá-lo invalida a captura anterior. Propostas de fatos e notas conservam as barreiras de
procedência e só se aplicam após sucesso comprovado, conforme a memória automática
da main. Recibos e Desfazer permanecem na conversa emissora, inclusive após
reinício. Não há confirmação antiga; Codex omite e recusa mutantes de memória. Nos outros
motores, a lista de ações de memória (nome, descrição e campos, sem conteúdo) aparece desde o
preparo do turno e é a mesma depois da liberação; usar só funciona depois dela. O Hub revalida
a origem e o binding após esperas e antes de efeitos. A conversa adicional não é
um bot nem uma autorização para ler o histórico privado por `search_history` ou
`open_history`: ambos continuam exclusivos de `main`. Foreign conserva namespace
separado e não recebe MCPs nem a Alma pessoal.

A memória registra fatos extraídos, não a conversa inteira. Um pedido humano
elegível só aprende após sucesso comprovado; conhecimento extenso usa notas.
Conteúdo externo conserva origem marcada e salva sem pergunta. O recibo mostra
natureza, origem, data, versão anterior e Desfazer; memória não concede permissão
e segredo é recusado. Rotina, webhook, canal e execução bot→bot não gravam memória
do dono. O [desenho](../reference/memoria/bot-memory.md) explica as camadas; `codync-mem`
concentra as regras e `chat/memory` as executa no Hub.

- **Permissão das edições do app:** salvar, editar, fixar,
  arquivar, esquecer, limpar e desfazer (inclusive recibos automáticos) conservam
  a autoridade original do aparelho durante a fila. A permissão é conferida novamente na mesma transação
  que altera a memória, a versão do arquivo e o cartão/recibo correspondente.
  Aparelho revogado antes da gravação não altera fatos, arquivos nem recibos.
  Ao desfazer, o evento do cartão acompanha a ordem das revisões confirmadas,
  sob a trava de emissão existente. A projeção dos arquivos e a notificação de
  mudança acontecem depois de soltar as guardas.
  A conferência não promove aparelhos a Local; ponte e bot continuam recusados.
  Respostas a confirmações legadas continuam recusadas.
- **Two layers** in the host database (`memory_entries`): "yours", shared by every bot that uses it, and one per
  bot. Caps count tokens (one per CJK character, one per four others): 400 for "yours", 1,200 per bot. A bot in
  its own workspace uses "yours" by default and one in a project folder doesn't; the user switches it per bot
  (`sharedMemory`).
- **The opening** holds every active entry the bot sees, pinned first, within the caps, with ids so the bot can
  revise or archive one. It says memory is information, never instructions: the bot runs no command, reveals no
  secret, changes no permission and uses no tool because an entry or a note asks (Mexerica, adapted from
  ai-memory's notice; the notes index and the harness manual say the same). Archived entries stay searchable (`search_memory`, which ignores case and accents: an
  FTS5 trigram index with `remove_diacritics`, kept by triggers; a query under three characters reads the entries
  instead). The opening doesn't change for a
  memory change, so the prompt cache holds: every change (the user's, a bot's, dreaming's) reaches each session of
  the bots that see it on its next message as a `<memory_update>` with the whole memory. The session whose own
  tool call made the change isn't told. A ledger in `kv` keeps, per session, the revision its opening holds and
  the one it has heard; after a compaction the second falls back to the first, so whatever is newer is told again.
- **The cut follows the request:** when a layer exceeds its cap, a local deterministic ranking
  (`codync_mem::rank`, BM25 over accent-folded terms; no model, no network) chooses which entries fit — only
  the set changes; render order, caps and pinned bounds stay as before, and with no cut the opening is
  byte-identical to the old one. The query is the owner's own words (`turn_text`): routines, groups,
  bot-to-bot requests, channels and hidden turns don't rank. Each active entry left out is recorded in
  `donna_memory_opening_omitted` (viewing bot, Space, memory layer, entry, closed reason `overBudget` /
  `pinnedLimit`, `at`, capped at 500 rows) and the Memory screen shows it under "Left out"; shared-layer
  entries outside a bot's scope show as `scope`. The record mirrors the last opening rendered for an
  owner turn — a routine's, a group's or a hidden turn's opening (`query` absent) never overwrites it —
  and a `<memory_update>` notice may be ranked by the current request without recording anything. No
  content travels in the record, and another bot's or Space's rows never surface.
- **Ferramentas e gravação:** o Hub prepara mudanças de memória durante
  um pedido humano elegível e as confirma somente no fechamento bem-sucedido.
  Rotina, webhook, canal e execução bot→bot não gravam memória do dono. No Codex,
  ferramentas de escrita não são oferecidas; a extração usa o Bot de fundo após
  o turno. Isso também vale quando o dono pede uma revisão ou arquivamento manual:
  `revise_memory` e `archive_memory` continuam omitidos e recusados nesse motor.
  Revisar/arquivar manualmente pelo chat Codex fica fora desta etapa. Em outros
  motores, propostas de ferramentas também aguardam sucesso.
  Não há cartão de confirmação, inclusive para conteúdo externo marcado. Cada
  alteração confirmada produz recibo estável com origem, natureza, versão anterior
  e Desfazer. O recibo é ligado aos IDs da resposta daquele fechamento, mesmo se
  chegar depois de outro turno. O undo de nota confere versão e revisão do recibo; limpar e recriar a nota não
  permite que um recibo antigo altere a nova. Uma repetição de undo é idempotente; timeout não
  confirma sucesso nem falha. Edições posteriores são preservadas. O recibo da
  versão desfeita é imutável; a lista recarrega o estado atual. Evidências e
  tombstones impedem reaprender o mesmo fato da mesma mensagem, sem alegar
  deduplicação semântica perfeita. Em fatos e notas, repetir o conteúdo ativo ou uma
  alternativa já registrada não abre versão ou aviso por uma indicação do modelo.
  Uma fala literal nova do dono mantém seu tratamento próprio; revisão ou hash
  obsoleto continuam recusados antes de reconhecer equivalência. Material desfeito da mesma evidência não é reaprendido nem
  tratado como falha técnica a repetir. IDs de fatos não são reutilizados; notas conferem também a revisão.
- **Checks** nas gravações automáticas (`codync_mem::check`): no hidden characters, prompt-carrying
  links, planted instructions, passwords, keys, ID or card numbers, nothing the user forgot, no duplicates, and
  the cap. Entries the user wrote can't be changed by a bot or dreaming.
  A edição humana mantém seus limites próprios, mas também recusa segredos nas
  novas escritas e restaurações; não é uma exceção à regra de não guardar segredo.
  - **Mexerica:** the checks also read Brazilian Portuguese, without accents and as whole words: planted
    phrases ("ignore as instruções anteriores", "prompt do sistema", "você agora é", "fonte confiável"). A secret
    label counts only with a value that looks like a secret (8+ characters with a digit, or a 4-to-8-digit code):
    "senha", "token" or "código de verificação" followed by `:`/`=` or "é" and such a value ("preciso trocar a senha
    e o e-mail do banco" and "senha do Netflix: a mesma do e-mail" pass); "chave pix" followed by
    an e-mail, phone, CPF/CNPJ or random key; "conta" followed by an account number. A phone, CEP or 0800 number
    alone isn't a secret; a card number is caught by its Luhn digit. A CPF or CNPJ (the letters-and-digits CNPJ
    too) whose check digits add up is refused. Dreaming's evidence knows Portuguese bare acknowledgements
    ("beleza", "isso", "pode", "fechado").
  - **Credentials redacted** (`codync_mem::redact`, patterns adapted from ai-memory, see `THIRD_PARTY.md`): API
    keys of known vendors, bearer tokens, JWTs, private keys, the password in a URL, environment variables in
    capitals (`*_KEY=`, `*_TOKEN=`) and header fields whose value looks like a secret (8+ characters with a digit),
    and the Portuguese labels above. A gravação de um fato ou de uma nota com segredo
    é recusada antes do efeito; não se substitui o valor para salvar a proposta.
    A redação protege o material enviado ao extrator: ele lê somente as referências
    do fechamento bem-sucedido, com segredos redigidos. Edições humanas conservam seu texto quando elegíveis, mas novas
    escritas e restaurações com segredo também são recusadas antes do efeito.
- **Scanner nos portões de escrita (memória pessoal):** `check::content` confere o conteúdo INTEGRAL
  que seria gravado — a nota inteira, não só as linhas novas — na mesma transação da escrita, junto
  com a revisão exata do alvo. Vale nos dois portões: a promoção automática (`memory_auto::apply`) e
  as propostas das ferramentas do bot (`apply::stage`), antes de estacionar qualquer candidato. Veredito
  suspeito é recusa fail-closed: nada é gravado nem preparado. Cada recusa grava um recibo
  (`donna_memory_injecao_recusas`, padrão de `memory_auto/audit.rs`) com id do candidato, revisão, ids
  das regras (`Refusal::id`), versão do scanner, `space_id`, `bot_id` e hora — nunca o texto nem hash
  do texto —, emite o evento interno `memoria.promocao.recusada` com os mesmos campos (tipo definido
  em `memory_auto/recibo_injecao.rs`) e deixa na conversa do bot o aviso "nota não salva: parece
  instrução vinda de conteúdo externo". A memória de equipe fica fora; `spaceId` nulo é `invalid_space`
  e Space ausente é o Pessoal, pela guarda de `spaces/scope.rs`.
  - **Liberação manual:** só o dono (`releaseMemoryRefusal`), em dois passos: sem `confirm` o método
    devolve o texto integral pendente para o dono ler; com `confirm: true` aplica. A origem é
    comprovada pelo mesmo critério de `write_gate::external_origin` — mensagem digitada ou de voz,
    `appReported`, sem anexo nem resposta — e a mensagem tem de ser posterior à recusa, a reação ao
    aviso. `spaceId`/`botId` informados no pedido têm de bater com a recusa. A liberação fica presa à
    revisão exata recusada: se o alvo mudou, falha (`memory_revision_changed`) e o dono revisa de
    novo. Rejoga a operação como `Writer::Override` — escrita do bot liberada pelo dono — dentro da
    mesma transação que confere origem, revisão e camada do alvo (Revise de outra camada falha), com
    origem `External` e natureza inferida: o texto não lava a proveniência nem vira "Você disse".
    Recibo separado em `donna_memory_injecao_liberacoes` e evento `memoria.promocao.liberada`. Não
    contorna as outras guardas: segredo continua recusado sempre e a recusa por `secret` não é
    liberável; descrição, tamanho, esquecido, duplicado e teto seguem valendo. O candidato pendente
    (`donna_memory_injecao_pendentes`) e os recibos de recusa e de liberação seguem a retenção da
    auditoria `non_saved`: 30 dias de vida, teto de 100 por bot + Space e de 1.000 por Hub, saindo
    os mais antigos primeiro — a recusa que sai leva pendência e liberações juntas, e a recusa
    vencida responde `memory_refusal_expired`. Tudo sai antes disso na exclusão do bot e no
    `clearMemory`.
- **Long-form files** (`chat/memory/files.rs`, rules in `codync_mem::files`): Markdown notes, one topic each, in
  `<data>/shared-memory/` and `<data>/bots/<id>/bot-memory/`. Names are lowercase slugs (`people.md`); each starts
  with frontmatter whose `description:` becomes its line in the opening's index (path, description, tokens). Caps:
  2,000 tokens per file, 20 files per folder, marked in the index when passed, never cut. Links, hidden files and
  folders are ignored.
  - O bot lê as notas pelas ferramentas de leitura ou `read_memory`, e grava só por
    `edit_memory`. Pastas de memória não entram em `additionalDirectories` graváveis.
    O Claude recebe regras de leitura e negação de edição. O vigia restaura mudanças
    diretas à versão registrada, sem atribuí-las ao computador; o dono edita pela tela
    do Hub. Propostas de alteração só se aplicam após sucesso comprovado, com
    origem e natureza registradas. Conteúdo externo não exige confirmação nem
    concede autoridade; o Hub continua recusando segredo e instrução plantada.
  - A varredura no começo/fim do turno e na tela de memória restaura alterações fora
    do Hub a partir do histórico. Notas novas sem versão são removidas. Um arquivo
    novo com menos de 2 s espera estabilizar; o fim do turno agenda nova conferência.
    Notas conhecidas adulteradas são restauradas sem depender da data do arquivo.
    A integridade é conferida pelo hash do conteúdo mesmo se tamanho e data forem
    preservados. Se a pasta sumir ou for renomeada, o Hub a recria e restaura as notas
    ativas sem criar versões. Links na cadeia da pasta são recusados; no Unix, leitura,
    restauração e escrita ficam ancoradas em descritores, sem seguir links para outro bot.
    Não se registram conteúdo nem nome da nota recusada no aviso de segurança.
  - O dreaming usa a versão registrada como base, confere e restaura adulterações antes
    de acrescentar a nota legítima, sob a mesma trava da gravação. Nenhum escritor aceita
    o hash de uma alteração externa como nova base confiável.
  - Alterações validam a versão anterior: fatos usam revisão e notas usam o hash
    registrado. A memória automática e as edições da tela confirmam primeiro a
    versão no banco; depois de soltar as guardas, o Hub projeta essa versão no disco
    por arquivo temporário e troca atômica. Isso vale também para limpar e desfazer.
    Se a projeção falhar, o diário confirmado permanece válido, o Hub registra um
    aviso e a varredura/reinicialização recompõe o arquivo, sem criar outra versão.
    A projeção não usa alteração externa como base autorizada. Edições da tela
    conservam a conferência de versão própria. `search_memory` também
    encontra trechos das notas.
  - Deleting a bot deletes its folder and versions; "Forget everything" deletes a layer's files and versions; a
    file deleted one at a time stays restorable from its versions.
- **Extração e consolidação:** o checkpoint de sucesso admite a fila
  durável na mesma transação e emite um despertar imediato, inclusive em conversa
  contextual. Inicialização, Flush e recuperação recontam essa fila; não varrem
  `conversation_since` nem aprendem de turno falho ou cancelado. Cada trabalho
  referencia mensagens humanas revisionadas e as respostas do mesmo fechamento,
  além do bot, conversa e Space reais. O executor recebe somente o material
  redigido e validado; entrada total limitada a 128.000 bytes, até 32 operações.
  Reservas abandonadas são recuperadas. Capacidade ocupada adia a tarefa, sem
  consumir evidência como sucesso. Resultado vazio válido pode concluir sem
  alterações; falha não equivale a “nada mudou”. A aplicação revalida o escopo e
  as revisões e grava memória, recibo e estado na mesma transação. Consolidação
  externa usa a mesma origem marcada e a mesma prova de sucesso. Nenhum lote é
  descartado apenas por ter sido exposto. O executor isolado continua limitado
  às contas e audiências comprovadas, como detalhado abaixo.
- **The harness's own memory** is off for every bot and for background jobs (`agent/own_memory.rs`): Claude Code
  (`autoMemoryEnabled` and `autoDreamEnabled` false, `CLAUDE_CODE_DISABLE_AUTO_MEMORY`), Copilot (its memory tools
  excluded), CodeBuddy, and Qwen Code (workspace settings, so only in a bot's own workspace: a project folder's
  files are the user's). Gemini CLI, Kilo Code and Kiro have no switch Codync can use. Project knowledge goes in
  the bot's own notes, and into the project's `CLAUDE.md` or `AGENTS.md` only when the user asks.
- **Mexerica:** Claude and Codex native memory always stays off, even with a persisted `harnessMemory: true`.
  The host reports `harnessMemory: null`, so clients hide that switch. Codync owns the bot's memory. Other
  harnesses retain the upstream switch where supported; they are outside this validation.
- **The apps** (iPhone, desktop, terminal) show "About you", "This bot", the dream log and Claude Code's notes,
  with add, edit, pin, archive, forget and forget-everything. Under each layer's entries (the terminal: a Notes
  section) they list its notes with description, size and date, marked past a cap; a note opens to read, edit,
  delete or bring back any version, and on the desktop, when the host is this computer, in the user's editor. They
  fetch with `memory` and fetch again when the bot's `memoryRev` moves. Methods: `memory`, `saveMemory`,
  `pinMemory`, `archiveMemory`, `forgetMemory`, `clearMemory`, `undoDream`, `memoryFile`, `saveMemoryFile`,
  `deleteMemoryFile`, `restoreMemoryFile`.
- **Moving the old memory**: on the first start of this version, each bot's Engram database (or the Markdown
  memory before it) moves in once. What the old opening showed comes in active as far as the cap allows (the
  profile and pinned facts first, then the newest), the rest archived; then the old files (deleted bots' too) and
  the Engram download go. Dreaming starts from that point in the chat.

## Compaction and rollover (`host/src/agent/bot/compaction.rs`, rules in `codync_mem::compaction`)

- **What the host records**, per session in `kv` (`compaction.<bot>.<session>`): context use and window size
  from `usage_update`, compactions, and when turns end. How long the provider keeps prompts cached is learned
  from turns that came after a pause (`cache.life.<bot>`); until then it is an hour for Claude on a subscription
  and five minutes otherwise. Each turn's cached read and write tokens are logged (`turn usage`).
- **Compacting ahead of time**, in the bot's own chat only (never rooms, routines or bot requests):
  - at the end of a turn past three quarters of the window or 200k tokens, whichever is less;
  - while idle, with a one-hour cache and over 100k in use, five minutes before the cache expires.

  It runs as a hidden turn: dreaming first, then the opening rendered again (a Claude session reloads only when
  it changed), then `/compact <keep-list>` for Claude, or `/compact` for Codex, whose keep-list is its
  `CODEX_CONFIG` `compact_prompt`. Other harnesses compact on their own. The keep-list is also in the opening.
- **Loop guard:** after three host compactions in a day, or one that left 60% of the trigger in use, or one
  fewer than five turns ago, the host rolls over instead of compacting again.
- **Rollover:** also due when a conversation over 100k went cold (its cache expired) or after eight hours away
  with three compactions behind it. A handoff card appears in the chat (`chat/handoff.rs`): the last summary,
  the user's last four messages and the last reply (each cut at 2,000 characters, so a pasted document doesn't fill
  the new conversation), the open plan, and a bookmark for `search_history`. The user can
  edit it (`editHandoff`) or keep the conversation (`keepConversation`); otherwise their next message there starts
  a new session that begins with it. The summary belongs to that conversation: if the new session fails to open, it
  waits for the next message there, and a group turn, a request from another bot or a routine run that comes first
  never carries it. The card is built from the **main session only**: every path (rollover Full/Cold/Away,
  a fresh session after a restart without `session/load`, the waiting reservation) reads `entries.session IS NULL`
  in SQL, before the read limit, so a line from an extra session never reaches it (see [Sessões por bot](#sessões-por-bot)).
- **Engine switch.** One hook, `engine_switched` (`agent/bot/fork_troca_motor.rs`), runs
  where the actor learns the main conversation's engine changed. The Personal bot's main
  conversation has its own engine selection, so the switch point is the start of the next new human turn that
  promotes the owner's new selection (`turn.rs`, `turn_config` → `engine_switched_on_turn`), before the old
  connection closes. Editing the bot's default no longer switches a conversation that has its own selection: no
  line and no handoff (`Cmd::Reconfigure` still switches one without a readable selection). Only the main
  conversation, and only when a session existed; extra sessions get no engine-switch handoff yet.
  - **Same provider, another model** (Opus → Haiku): the session resumes whole, as before; no handoff.
  - **Another provider** (Claude → Codex): the old engine's session can't serve the new one, so the host drafts
    the rollover handoff with reason `engineSwitch` before forgetting the session. The card says which engine it
    moves from and to, and the user can edit it but not keep the old conversation (`keepConversation` is
    refused, and the app hides the button). The next main-chat message carries it (`take_handoff`), as in a
    rollover; when the switch happens at turn start, that message is the one opening the turn, so the card is
    sent with it right away. A draft already pending is not duplicated: it carries the passage itself, and its
    reason becomes `engineSwitch` even if the queue already took it for this message (by the exact card ID that
    `take_handoff` kept, never by searching the history). Closing the old connection keeps that passage only for
    a switch promoted in this conversation; any other configuration change at turn start still drops it.
  - **Two cautions that no longer apply to the main conversation.** (1) *Switch during a reply*: the
    owner's change only takes effect on the next new turn, so the reply already ended and is in the passage;
    the deferral to the end of the turn (`engine_switch_after_turn`) stays only for `Cmd::Reconfigure` of a
    conversation without its own selection. (2) *Usage deferred by a pending restart*: at turn start the new
    connection opens right away, so no restart is pending; the `restart_after_turn` guard on `resentTokens`
    stays for the same legacy path. Neither guard was removed.
  - **A request waiting for the limit**: its retry already carries the handoff (`switch_waiting_model`), so
    only the line is added.
  - **The line in the chat**: a notice reading "Modelo trocado de X para Y. A conversa foi passada para o novo
    modelo." (no tokens, no cache), with `modelSwitch {from, to, kind: model|provider, carried: session|handoff,
    resentTokens}`. `from`/`to` are `{backend, model, label}`. `resentTokens` stays `null` until the new session's
    first `usage_update`, then holds what it read. The desktop shows the technical detail ("84 mil tokens
    reenviados…", whole session or summary) in a tooltip on hover or focus (`views/fork/troca-modelo`).
- **After every compaction** the host records a notice ("Earlier context was summarized to make room.", its
  `compaction` field carrying Claude's summary, which the next handoff starts from). The apps don't show it: the
  same conversation goes on.
- **In the apps** the chat explains only a fresh conversation, in one line, left-aligned like the bot's messages.
  The iPhone and desktop show the draft card with *Keep this conversation* and a pencil that opens the editor
  sheet; once sent it folds to "Started a fresh conversation with this summary.", which unfolds to what it
  carried, and once kept it leaves no line. In the terminal, pick the line (`v`, `j`/`k`), then `o` unfolds it,
  `e` edits and `K` keeps the chat. The user starts a new session from the bot's menu (terminal: `S`), not with a
  `/` command.

## Turns

- Messages sent while the agent works are folded into one next turn, joined by blank lines.
- Bot-to-bot requests occupy separate turns in the same queue; user messages are only folded together up to the next request. Requests never feed dreaming. See [bot collaboration](bot-collaboration.md).
- Turnos delegados também registram reserva/permissão em inflight, com origem transitória; seu canal não sobrevive ao reinício. Avisos pendentes e o registro durável do pedido ficam interrompidos, sem replay automático.
- Stop sends `session/cancel`; an agent still running 15 seconds later is stopped (it resumes its session on
  the next turn). `session/new`, `session/load` and `session/fork` get three minutes before the agent is
  stopped. Connector or computer changes restart an agent that can resume sessions before its next turn; one
  that can't keeps its session, and its next new session gets them.
- `turn.inflight.<bot>` agora registra a reserva e a permissão durável do produtor
  de admissão por bot, com o token de execução que o Hub já gera e uma
  cerca por tentativa. Permissão incerta não dispara continuação automática no
  reinício. Marcadores legados sem checkpoint comprovado também não
  autorizam prompt oculto: o Hub avisa para conferir o histórico e conserva a fila
  dos pedidos novos. A retomada restaura o token original do checkpoint existente;
  ausência de identidade/política comprovadas pausa a retomada sem fabricar outra.
  Resultados prontos continuam sendo publicados sem novo prompt.

## Sessões por bot

Este trecho descreve o que já está no Hub e o que o app faz com isso.

- **No app:** no chat de um bot do Pessoal, o **+** das abas, **Nova sessão** no painel e no menu do bot
  criam uma sessão vazia (`sessionCreate`) e abrem a aba devolvida, sem mandar nada, com o cursor já na caixa
  de mensagem dela. O **+** fica logo depois da aba do bot, com as sessões mostradas ou recolhidas. Na aba da extra, a caixa envia
  com `send.sessionId` da aba escolhida no instante do envio; rascunho e anexos são da aba. A principal envia como
  antes. Canais e conversas de Space continuam só com a principal; o × fecha a aba (a sessão segue no painel);
  arquivar, encerrar e continuar ainda não aparecem. O antigo "Nova sessão" do menu, que recomeça a principal pelo
  `newSession`, agora se chama **Recomeçar a principal…**.
- **Anúncio (`sessions.sessionActions`):** a resposta de `sessions` traz `["create","send","close","continue"]` (mais
  `engine` onde a conversa tem seleção própria e `choice` no chat de bot agente vivo do Pessoal), só de leitura; `close`/`continue` só num Hub que as suporte. O app só
  mostra criar e só envia numa extra com as duas ações anunciadas **neste** link e nesta época: o `sessionCreate`
  já existia antes de o `send` respeitar `sessionId`, e um Hub assim gravaria a mensagem da extra na principal.
  Ausente, ilegível ou com nomes desconhecidos = nenhuma ação (o Hub antigo fica só com a principal, como antes);
  reconectar ou trocar de Hub zera o anúncio até o catálogo responder de novo, e antes de cada envio (depois dos
  anexos) o app relê o catálogo no mesmo link. Um Hub trocado por um antigo no mesmo link entre essa releitura e a
  chegada do envio grava na principal; o app só pode avisar depois ("A mensagem foi para a conversa principal"). O anúncio não autoriza nada: cada RPC continua validando bot, sessão, estado e conta.
- **`choice` (cartão de escolha, contrato `sessionChoiceAnswer` v1; RPC no Hub, ver "Responder a uma escolha numa extra"):**
  o app só responde um cartão de escolha numa extra com `choice` anunciado neste link/época e relido antes do envio, sem
  depender de `send`; sem ele, nada sai e nunca cai na principal ([UI generativa no chat](ui-generativa-chat.md)).

- **Catálogo** (`host/src/fork/sessoes/`, tabela `donna_bot_sessions`): cada conversa tem uma **principal**, com o id
  da própria conversa: o chat de cada bot e de cada canal, e cada conversa de Space. A migração cria as que faltam,
  sem reescrever nenhuma entrada, revisão, sessão do motor, compactação ou passagem; salvar um bot ou criar uma
  conversa de Space cria a dela. Sessões extras só existem no chat próprio de um bot do Pessoal e só nascem por
  `fork::sessoes::store::create`, o produtor único.
- **Campos:** `origin` (quem pediu: `owner`, `continuation`, `bot {botId, fromSession?, fromExecution?}`, `routine`,
  `notice`, `group`, `subsession`, `suggestion`, `routed`), `source_session`, `parent_session`/`depth` (até neta),
  título e origem do título, estado e `runs_on` (onde a sessão roda; vazio = local). `origin` e `runs_on` são
  validados na gravação **e** na leitura: JSON ilegível, tipo desconhecido, chave extra ou campo obrigatório
  ausente falham, nunca viram `owner` nem local. `runs_on` não vai ao fio.
- **Entradas:** `entries.session` (vazio = principal). No fio, toda entrada leva `sessionId`; a principal tem o id
  da conversa. `history {botId, sessionId}` devolve só uma sessão daquela conversa.
- **RPCs:** `sessions {botId}` lista a principal primeiro e as outras pela última vez usada;
  `sessionRename {botId, sessionId, title, expectedRevision}` renomeia com conferência de revisão (até 60
  caracteres) e publica o evento `session`. A principal não muda de nome. `sessionCreate {botId, title?}` cria uma
  sessão extra (`origin: owner`) no chat de um bot do Pessoal e publica o evento `session`; criar não executa nada.
  Com título, ele é do dono; sem título, a extra recebe o da 1ª mensagem (1ª linha, sem Markdown, até 60
  caracteres). Valem para o Local e para aparelhos com `control`, como `send`.
- **Envio a uma sessão** (`send.sessionId`): ausente, `null` ou o id da conversa = principal, como antes. Sessão de
  outra conversa → `unknown_session`; encerrada → `session_closed`; escolha ilegível falha antes de gravar a entrada.
  A extra roda em série com as outras sessões do bot, no próprio slot do motor (`session:<id>`), com carimbo de conta
  próprio (`account.stamp.<bot>/session:<id>`). Antes de gravar, o envio confere com a classificação canônica do
  Roteamento (só leitura, escolha fixada ou Fixo do bot) se a extra seria admitida; se não for (sem conta gerenciada,
  conta quebrada ou opções sem modelo explícito), recusa com `session_selection_unavailable`, sem entrada e sem travar a fila
  do bot. Se uma extra assim chegar ao ator por outro caminho (fila restaurada no boot, conta trocada antes do
  despacho), ela fica retida na fila ("Aguardando execução nesta sessão."), sem reserva e sem virar prompt da
  principal, como a fila já fazia. A admissão do permit continua obrigatória: se a conta mudar ou for revogada entre
  essa conferência e o permit, o permit falha fechado e o pedido fica pendente, segurando a fila do bot até o dono
  usar Parar (retry existente), sem envio.
- **Escolha de motor da sessão** (`donna_bot_sessions.engine_choice`): a principal do Pessoal e cada extra
  guardam na criação o retrato do padrão atual do bot (`schemaVersion: 2`, `source: botDefault`, conta, motor,
  modelo, opções e velocidade), sem copiar a principal nem acompanhar edições posteriores do bot. Só
  `fork::sessoes::engine_choice` lê e escreve a seleção; leitura ilegível falha fechado. A revisão da conta fica
  fora deste JSON, e a coluna não concede autoridade. A principal de Space segue a escolha da conversa de Space.
  A troca explícita por conversa grava a próxima seleção, promovida só no início de um turno novo.
  - **Extra:** no `queued` → `sent`, dentro da transação IMMEDIATE do permit, o Hub grava o recibo com a
    pré-imagem e o carimbo da própria sessão (`SessionAccountStampV1`); a execução deve coincidir com o retrato
    efetivo. A admissão não reescreve a seleção nem a revisão da sessão. Uma extra é sempre roteada: exige conta
    gerenciada válida e modelo explícito validado no catálogo ou padrão implícito sem opções. O padrão é
    registrado como `model: null` no recibo, sem inventar modelo; vale também para extras já criadas com essa seleção,
    sem migração. Seleção ausente ou ilegível não cai no padrão atual do bot.
- **Fila por sessão**: cada mensagem `queued` fica na fila da sessão onde foi escrita. Remover,
  editar, mover (`queuedMove {botId, entryId, position, expectedRevision}`, posição dentro da fila dessa sessão; a
  ordem nova fica em `data.queueOrder` e volta igual depois de reiniciar) e mandar agora mexem só nela; mandar agora
  interrompe só a resposta dessa sessão (ou uma compactação) e põe a mensagem na frente do ator.
- **Encerrar** (`sessionClose {botId, sessionId, expectedRevision}`): só extras. Na mesma transação, com a
  autoridade original do pedido reconferida (Local ou aparelho com `control`), a sessão vira `closed`
  (`closedReason: owner`, revisão nova) e as mensagens dela na fila viram `cancelled`, junto dos cartões de
  permissão, perguntas e escolhas pendentes dela, o checkpoint de espera dela (vira encerrado, como o tombstone da
  espera; uma conclusão durável, resposta já produzida e ainda não publicada, fica e é publicada sem prompt novo) e
  a chave do cartão de continuação. Depois do commit o ator para só o turno ou a espera dessa sessão,
  revoga só as pontes do slot dela, cancela só os pedidos `ask_bot` feitos por ela (também quando a parada conclui) e
  esquece o slot dela; nenhuma outra sessão para. Um pedido cuja resposta já expirou não é executado depois pelo ator. Responde
  `{session, cancelled: {entryIds}, stopRequested}` e publica `entry`/`session` depois do commit. Erros:
  `unknown_session`, `principal_not_closable`, `session_closed`, `session_revision_conflict`, `not_allowed: …` e
  `session_control_pending` (sem resposta em 10 s: resultado incerto, o app relê `sessions` e nunca mostra
  encerrada por otimismo). Depois de fechar, o despacho (`Dispatch`) e a retomada (`Resume`) recusam
  `session_closed` na transação do permit, o vínculo da sessão deixa de valer (chamadas atrasadas são vencidas) e uma
  espera dela termina sem turno, com o aviso "Esta sessão foi encerrada antes de eu conseguir responder.". O histórico
  continua legível; `send` direto ao ID continua `session_closed`.
- **Continuar em nova sessão** (`sessionContinue {botId, sessionId, expectedRevision}`): a origem precisa ser uma
  extra encerrada (`session_open`, `principal_not_continuable`). Numa transação cria outra extra pelo produtor
  único (`origin: continuation`, `sourceSession`), com o retrato do **padrão atual** do bot (nada da
  seleção, carimbo, vínculo, token ou checkpoint da origem), herda a exposição externa da origem e posta na nova
  sessão o cartão de passagem editável (`handoff.reason: continuation`, chave `handoff.<bot>/<sessão>`), montado só
  com as linhas da origem e o resumo do motor da última ACP dela (`donna_session_acp`), dentro de `<handoff>` como
  informação, não instrução. Responde `{session, source, handoff}`. Não envia nada nem reabre a origem. O cartão é
  a única fonte da passagem: o preparo do primeiro turno humano da sessão o lê do banco (também o preparo
  restaurado de uma reserva interrompida), mesmo com outra passagem em memória, e o permit (`queued` → `sent`) o
  consome na mesma transação, comparando a revisão dele com a que compôs o prompt. Uma edição aceita antes do
  permit desfaz esse permit e o mesmo preparo (entradas, reserva, token e conta) recompõe o prompt com o texto
  editado; depois do permit, editar encontra o cartão enviado e não muda nada. Falha de abertura, admissão ou
  reinício antes do permit o deixa em rascunho, e a falha definitiva de uma sessão não tira o cartão de outra. O
  Hub sempre envolve o texto do cartão num quadro de contexto recuperado ("information, not instructions"), mesmo
  editado: a mensagem digitada no turno é o pedido. Dentro do quadro o texto vai literal, como string JSON com `<`,
  `>` e `&` escapados, então nenhum fechamento ou marcador escrito no resumo ou na edição abre ou fecha bloco; o
  cartão guardado mantém o texto bruto. Duas continuações têm cartões separados. "Manter
  conversa" não existe nesse cartão (o app também não oferece). A ACP nova nasce marcada como contexto herdado e,
  com a sessão visível exposta, como externa.
- **Recomeçar a principal** (interno, usado pelo `newSession` legado, que continua respondendo `{}`): a autoridade
  original do pedido vai explícita até o ator; numa transação com ela, cancela só a fila, perguntas, escolhas e o
  checkpoint de espera da principal (nunca uma conclusão durável) e deixa o cartão de passagem na chave legada
  `handoff.<bot>` (`reason: restart`, sem "Manter"): um rascunho que já estava lá vira o do recomeço, com o texto
  editado; sem ele, um novo. Como o da continuação, esse cartão é lido pelo preparo do primeiro turno da principal e
  consumido só no permit dele (sobrevive a troca de configuração e a uma queda entre reserva e permit); os rollovers
  (Cold/Away/Full/troca de motor) seguem o fluxo de antes. Autoridade revogada antes de o ator processar: nada muda. Depois do commit, para o turno ou a espera
  só se forem da principal, cancela só os pedidos `ask_bot` que a principal fez e as rotinas/pedidos de bot na fila
  (vivem na principal) e esquece só a sessão do motor da principal. Extras trabalhando, na fila, em espera ou com pedidos próprios continuam. `stop` continua como antes (ainda para o bot inteiro).
- **Responder a uma escolha numa extra** (`sessionChoiceAnswer {botId, sessionId, expectedRevision, questionId,
  revision, answer}`, contrato v1; anúncio `choice`): só extras abertas de bots agentes do Pessoal,
  inclusive o Berga; principal (`genui_conversation_required`), Space, grupo, canal, Times e rotina ficam fora. Classe
  `Comum`: o computador do Hub ou o app com Control e a autoridade ORIGINAL, capturada antes de qualquer espera e
  reconferida na transação da escrita (`with_db`, sem `engine.accounts`). Corpo fechado depois do envelope comum
  (`invalid_field: pedido de resposta inválido.`). Na transação, a sessão primeiro (`unknown_session`,
  `session_chat_unsupported`, `session_closed`, `session_revision_conflict`), depois a pergunta: inexistente, de
  outra sessão ou de outro bot recebe o mesmo `genui_question_scope_mismatch`, sem eco. Replay idêntico devolve o
  mesmo recibo (`replayed: true`) sem gravar, publicar ou reenviar ao ator, mesmo com a conta da extra indisponível.
  Resposta nova exige a extra admissível agora (`engine_unavailable`/`session_selection_unavailable`, pré-checagem só
  de leitura, nunca prova) e entra uma vez na fila canônica como `user` `queued` com `choice_response`, sem autoria.
  Responde `{session, question, entry, replayed}`; a `SessionRow` não muda. Fechar a extra cancela a pergunta pendente
  e a resposta ainda na fila. O produtor é o `mostrar_componente` do próprio bot na extra, com selo de
  ferramenta emitido pelo Hub para a extra exata ([UI generativa](ui-generativa-chat.md)); não muda esta RPC, a
  seleção de conta nem a fila. A varredura do início cancela a pergunta de extra fechada ou ausente.
- **Editar e manter o cartão** (`editHandoff`, `keepConversation`, classe `Comum`): exigem a origem original do
  pedido (sem origem, recusa; nunca vira Local) e decidem sobre o cartão ATUAL numa só transação com ela (fronteira
  de memória → emissão → guarda original → Cloud → DB): ainda em rascunho, apontado pela chave da conversa e com a
  sessão extra aberta. Manter confere o motivo atual (recusa recomeço, continuação e troca de motor); editar troca só
  o texto. Um recomeço que ganha a corrida vale; a edição dele preserva o motivo. Aparelho revogado: nada muda.
- **Arquivamento automático** (no Hub): uma varredura por hora encerra com `closedReason: idle` as extras do
  Pessoal sem atividade há N dias. Configuração do sistema `sessionAutoCloseConfig {}` (leitura) e
  `sessionAutoCloseSave {days, expectedRevision}` (dono; `days` 1–90, ou `null` = Nunca; padrão 3; evento
  `sessionAutoClose`). Atividade é a criação da sessão e as entradas dela (`created_at`/`updated_at`); ler,
  renomear, trocar o modelo e mudar a configuração não contam. Na transação do fechamento o Hub relê a configuração e
  o limite e confere de novo: bot agente vivo do Pessoal, extra aberta e inativa, sem mensagem na fila, cartão de
  permissão, pergunta ou escolha pendentes, espera de retomada dela, admissão em andamento ou cartão de continuação
  em rascunho; registro durável ilegível conta como pendente (adia). O ator também pula a sessão com turno, espera,
  conclusão, fila ou `ask_bot` em memória. A varredura nunca cancela nada e nunca toca a principal. Não há preferência nem temporizador
  no app; "arquivada" é só apresentação de `closed`.
- **No app** (`views/fork/conversa/arquivar/`): cada ação só aparece com `close`/`continue` anunciados
  em `sessionActions` neste link; Hub antigo sem eles não oferece nada, sem substituto por `newSession` nem pela
  principal. **Encerrar conversa** fica no menu da aba de uma extra aberta (botão direito), com confirmação curta;
  nunca na principal. A conversa encerrada (por você ou por inatividade) fica só para leitura: no lugar do campo de
  mensagem, o motivo ("Encerrada por falta de atividade" / "Encerrada por você") e, junto, **Continuar em nova
  conversa**, desabilitado até a resposta (cada chamada cria outra sessão), que abre a sessão nova com o cartão de
  continuação (sem "Manter"). O catálogo só muda com a resposta confirmada; erro ou `session_control_pending` relê
  `sessions`. **Ajustes → Conversa → Arquivar sozinho**: 1, 3 (padrão), 7, 14, 30 ou 90 dias, ou Nunca, gravado no
  Hub com `expectedRevision` (conflito relê; só o dono grava); texto fixo "Conversas com trabalho pendente nunca são
  encerradas sozinhas."; Hub sem o RPC: a seção pede para atualizar. O bot não vê nada novo.
- **Apagar o bot** não muda o estado das sessões dele nesta versão: os vínculos de sessão são revogados pelo gatilho
  da exclusão e o chat deixa de existir para o app. `closedReason: bot_deleted` é um motivo previsto no catálogo, ainda
  sem produtor; nada grava esse motivo hoje.
- **Admissão** (`fork::sessoes::admissao`, encaixe `ClaimHooks`): no primeiro despacho de um turno humano,
  classe canônica do Roteamento (`admission_class`), recibo (`record_dispatch`) com o token e o vínculo originais e as
  revisões lidas na mesma transação (`revisions`), CAS da escolha da extra, claim, permissão e commit numa só
  transação; falha em qualquer passo desfaz tudo, sem ativação, evento ou envio. A retomada usa a regra canônica
  (`classify_resume`) com a classe da espera: `routed` exige o checkpoint original, sem recibo novo, nova escolha ou
  outro token; `notRouted` sem checkpoint segue o caminho atual; checkpoint ausente quando roteado, corrompido ou
  contraditório falha fechado, também na principal. Fatos sem produtor uniforme ficam como fato requerido não
  atestado.
- **Quando a principal fica sem roteamento:** bot fora do Pessoal, sem conta gerenciada (login legado/API/local) ou
  com conta gerenciada válida **sem modelo explícito** (compatibilidade legada da principal). O turno roda
  como antes, sem recibo, marcado `notRouted{unprovenSelection}`. Com conta gerenciada, a validação canônica roda antes
  na mesma transação, com a revisão capturada no setup: conta retirada, revisão divergente, desconectada ou
  desabilitada, motor incompatível, comando próprio ou catálogo inválido falham fechado
  (`routing_selection_unavailable`), sem envio. Nenhum modelo padrão é inventado. Sessões extras nunca ficam sem
  roteamento: com conta válida e padrão implícito sem opções, a extra grava recibo com `model: null`, continua
  `routed` e retoma pelo checkpoint original. O retrato do padrão do bot continua valendo na criação, sem copiar
  a principal nem mudar ao editar o bot; não há migração ou resolução automática de modelo.
- **Classificação de roteamento:** o permit grava no registro de admissão (`turn.inflight.<bot>`), no mesmo
  escritor e na mesma transação, `routing`: `routed` (recibo gravado) ou `notRouted` com o motivo (`preRoteamento`,
  `unprovenSelection`, `notHumanTurn`). A espera de assinatura copia a classificação como copia o token. Na
  retomada, a classe vai à regra canônica (`classify_resume`); sem classificação, a espera pausa com aviso, sem
  reclassificar. Um turno de extra nunca é `notRouted`.
- **Compatibilidade (migração de boot):** o primeiro binário com o gravador ligado marca, antes de qualquer
  ator e numa transação só, todo `turn.inflight.*`/`turn.waiting.*` sem classificação como
  `notRouted{preRoteamento}` e grava `donna.routing.legacy_marked`. Antes dele nenhum despacho gravava recibo, então
  esses turnos retomam como antes. O inflight numérico histórico (`started` em ms) vira o objeto legado equivalente
  `{started, conversationId, routing}`, com o mesmo início e o mesmo chat. A migração é idempotente: depois da flag,
  nada é remarcado. Valor vazio ("sem turno") ou corrompido não é tocado e não vira legado; falha de banco desfaz tudo,
  sem flag. A classificação vem da origem capturada na reserva: turno humano exige o consumidor de Sessões (sem ele,
  falha fechado); rotina, canal, pedido de bot, compactação e recuperação ficam `notHumanTurn`.
  - **Instante do carimbo:** o setup (`ensure_session`) grava o carimbo da execução aberta antes do permit; o encaixe
    lê o valor bruto na Tx do permit, antes de qualquer escrita dela. Editar o bot depois do setup não muda o turno
    admitido (o snapshot do setup é o que executa e o que se grava); conta retirada do cadastro falha fechado.
- **Recuperação:** a espera de assinatura guarda a sessão visível e retoma nela; a reserva retoma o lote na
  sessão das entradas, com o mesmo token; o aviso de reinício após permissão vai para a sessão do turno. O resumo
  pendente fica preso à conversa **e** à sessão para as quais foi tirado.
- **Carimbo de conta por sessão:** uma extra guarda o próprio (`account.stamp.<bot>/session:<id>`). Trocar a conta do
  bot invalida a principal, canais e rotinas, não as extras.
- **Vínculo da extra fixada:** trocar a conta ou o motor do bot revoga os vínculos do bot, menos os das extras com
  escolha fixada (gatilho `donna_bindings_account`, recriado pela migração). Ao abrir ou recarregar a sessão do motor,
  a ativação confere a escolha da própria extra e valida a conta dela no cadastro, na mesma transação; principal e
  extra sem escolha seguem a guarda antiga (Fixo atual do bot). Conta da extra retirada ou escolha ilegível recusam.
- **Retomada com vínculo vivo:** a retomada de assinatura exige o vínculo do preparo desta tentativa, revalidado na
  transação do permit e no destino original da espera; se ele for revogado durante o preparo (por exemplo, troca de
  conta do bot), a retomada recusa antes de liberar o turno, sem envio, recibo novo ou nova escolha.
- **Troca da sessão do motor numa extra:** se o motor não retoma (`session/load` ausente ou falho), a sessão nova
  começa com o resumo só daquela sessão visível; a sessão visível continua a mesma.
- **Marcador para apps antigos:** quando uma entrada é de outra sessão que a anterior do chat, o Hub grava antes
  dela, na mesma transação, um aviso "Sessão “X”" (`data.sessionMarker`), e publica os dois nessa ordem. Vale para
  toda inserção, inclusive o trecho que nasce `final: false`, rotinas e o aviso de mudança no time. O app novo
  esconde o marcador.
- **Exposição por sessão visível:** cada sessão do motor fica ligada à sessão visível dona dela
  (`donna_session_acp`). Toda marca de conteúdo externo da sessão do motor (entrada externa, gateway, ferramenta,
  histórico, contexto de canal) também fica na sessão visível (`donna_session_exposure`), na mesma instrução;
  falhar ali faz a marca falhar, e o Hub trata como exposta. Assim a marca sobrevive à troca da sessão do motor e
  passa à continuação.

## Limits

- An agent without ACP `systemPrompt` gets the instructions as a user message, which models follow less firmly
  than system text.
- The harness writes every summary. Codync chooses when the bot's own chat compacts and what it asks the summary
  to keep, and re-applies the instructions afterwards; it can't make a harness keep anything.
- A session a harness has deleted (for example, one cleaned up after a month) can't be resumed. The bot
  starts fresh, but its memory remains.
- With a managed account, the account the sessions were opened with is kept per bot as a stamp: account id, its
  revision and the backend (`account.stamp.<bot>`). Restarting the Hub with the same stamp resumes the sessions
  (the main one, each group's, routine runs'); a different account, revision or backend starts them over, once, even
  when the selection was saved and the Hub restarted before the bot heard of it. Before, every restart started them
  over.
- Memory search is a substring scan, not semantic vectors. Live dictation in the owner's app carries voice origin;
  attached audio and file transcripts remain external. Group turns do not feed dreaming and mark only the member's
  session for that group (each group has its own).
- `search_history` and `open_history` cover the bot's own chat, not group chats it took part in, and answer **only in
  a turn the owner started in the bot's own chat**. In a group, a request from another bot, a routine or
  with no turn running they return `denied`, so private messages never reach another context. In that turn,
  `search_history` finds messages by words, or reads back from a message id (`from`, then each page's `next`);
  `search_memory` gives each entry's source messages, so a bot can check where a memory came from.
- Turns are numbered per chat. Every new turn takes its number from one atomic counter per chat (`alloc_turn`:
  a message sent, a room turn, a routine run, a request from another bot), so two turns never share a number,
  and memory keeps a turn's session and origin per `(bot, chat, turn)`. Rows from before this change keep their
  meaning; an old group row that had overwritten a turn of the bot's own chat stays out of dreaming, as before.

A reply stays in its chat's session, and its quote stays out of what memory takes as the user's words; see [groups and replies](groups-and-replies.md). Group and delegated requests don't feed dreaming.


## Histórico com fontes exatas

`search_history` busca apenas mensagens `user` e respostas finais `agent`
do bot chamador vivo. A identidade vem do bridge, sem `botId` nos argumentos
do modelo. O mesmo filtro vale para `from`, cursor, abertura, vizinhas e
revalidação da entrega. Trace, raciocínio, cards, saída bruta de ferramenta
e notices de resumo não entram na projeção.

O contrato legado permanece: consulta opcional de até oito palavras, 10
resultados por padrão, máximo 30 e snippet de até 1.500 caracteres.
A correspondência usa somente o texto da projeção já redigida (`original_redacted`),
inclusive seus marcadores; valores removidos não selecionam mensagens nem
alteram a continuação. Cada chamada examina até 256 mensagens visíveis, mais
uma linha de fronteira. Ao atingir esse orçamento, pode devolver uma página
vazia com `next`/`nextCursor`; continue até não haver continuação. A fronteira
é inclusiva e ainda não foi examinada, podendo não corresponder à consulta.
`next` identifica o primeiro item ainda não entregue ou examinado e deve voltar como `from`
inclusivo. O teto de resposta pode reduzir o número de resultados; seguir
`next` continua sem perda. O novo `nextCursor`, passado como `cursor`, fixa
consulta normalizada, fronteira e `snapshotMaxSeq`. Com ele, omita `query`
ou repita a mesma; `from` e `cursor` juntos são recusados. Novas mensagens
não deslocam essa paginação; edições detectadas na referência/fronteira
retornam `stale_reference`. Não se promete fotografia imutável de textos.

Só um turno humano ativo na principal lógica do próprio bot recebe
`sourceRef`/`nextCursor` e pode chamar `open_history`. O Hub captura mensagem,
revisão, sessão e lane, e confere novamente antes da entrega.
A geração persistida do perfil também é revalidada: mudanças de configuração
ou permissão, inclusive A → B → A, invalidam a captura. Atividade do motor e
marcas de leitura não alteram essa geração nem invalidam a consulta. Fora desse
contexto (canal, pedido de outro bot, rotina, webhook ou sem turno), as duas
ferramentas devolvem `denied` e nenhum resultado. O Hub também põe em
cada servidor interno o slot da sessão do motor que o recebeu
(`DONNABOT_SESSION_SLOT`, `main` na conversa principal); uma chamada atrasada da
sessão de um canal ou de uma rotina, que chegue durante um turno do dono, é
recusada pela origem. A busca legada sem
referências, que antes respondia ali, levava mensagens privadas para o canal.
Não há leitura entre bots, Times, canais ou conversas adicionais.

`open_history {sourceRef, before?, after?, offset?, limitChars?}` abre a
âncora e até duas vizinhas visíveis por lado, ordenadas por `seq`. Os padrões
são duas por lado, offset zero e 8.000 caracteres por mensagem; o JSON inteiro
fica limitado a 32.000 caracteres. `anchor` identifica a âncora; `messages`
contém as projeções, cada uma com sua referência e cobertura. `nextOffset`
na raiz continua a âncora, e o de cada mensagem continua aquela mensagem.
Offsets são caracteres Unicode da projeção redigida. Redação ocorre antes
do snippet, do hash e de qualquer faixa. `previous`/`next` são referências
visíveis fora da janela. Nenhuma abertura reexecuta ferramenta ou carrega
arquivo, anexo ou conector para completar uma fonte.

Cada mensagem conserva `id/date/from/text` e acrescenta `createdAt` em
Unix-ms, `entryRevision`, `authorKind`, `originKind`, `botId`,
`conversationKey: main:<bot>`, `representation: original`,
`projection: original_redacted`, `redacted`, `truncated` e `coverage`.
`from` mantém o formato legado (`user`/`you`); `authorKind` distingue host,
humano, agente e desconhecido. Original é apenas o texto armazenado, com
redações declaradas. `coverage` informa faixa `[start,end)`, `totalChars`,
unidade `unicode_scalar`, completude da faixa e eventual `storageTruncated`.
O alcance é sempre `stored_text_only`, sem reconstrução de texto perdido.

Referências são envelopes opacos versionados, com namespace do banco,
bot/principal lógica, ID, revisão e hash da projeção redigida. Reutilizam o
namespace persistente do Guard; não há segredo, índice, retenção ou cache
de payload novo. Referência não é grant, assinatura ou prova de autoria:
decode limitado e fechado, escopo e banco são conferidos em cada chamada.
Um clone/restauração que preserve a identidade do banco conserva o namespace;
revisão e projeção ainda precisam corresponder. A política completa de
restauração/revogação de derivados fica para outra etapa.

Respostas têm `status`: `ok` (inclusive `results: []`), `partial`
(corte ou próxima página), `unavailable` (armazenamento indisponível),
`denied` (contexto sem alcance), `invalid_arguments`, `invalid_cursor`,
`stale_reference` ou `source_unavailable`. Fonte inexistente, alheia ou
oculta recebe a mesma resposta sem conteúdo ou contagem. Falha SQL não vira
vazio nem ausência de ID. Alterar a mensagem, o bot ou o contexto durante a
consulta invalida a entrega.

Origem externa, gerada ou desconhecida registra exposição pela porta interna
antes da entrega; falha nessa gravação bloqueia o retorno inteiro. Metadados
legados insuficientes são desconhecidos. Conteúdo histórico, inclusive humano,
jamais vira novo pedido ou evidência humana para `remember`. O bot pode
apresentar uma sugestão para o dono salvar na tela de memória. O porteiro de
efeitos e o isolamento forte continuam etapas separadas. A interface de
fonte no app e as conversas adicionais ficam para etapas futuras.

## Memória automática, procedência e natureza

Nunca se pergunta “Salvar na memória?”. O chat mostra **Memória salva · Desfazer**
com origem por item: dito pelo dono, extraído da conversa, externo identificado ou
origem não registrada no legado. A marca de exposição acompanha a linhagem da
sessão, resumos e compactação; uma conversa nova sem resumo começa limpa. A
origem desconhecida não vira conteúdo confiável. Segredos e instruções plantadas
continuam recusados antes da gravação. Memória nunca concede permissão.

No cartão **Precisa de você**, o turno da pergunta espera o dono e não vira
memória; respondido ou retirado o cartão, esse trabalho é descartado. A resposta
humana é a fonte da memória no seu próprio turno, pelo fluxo normal.

Separadamente, a natureza é `said`, `inferred`, `conflict`, `confirmed` ou `unknown`. Só um
literal humano verificado ou uma edição humana explícita recebe “Você disse”. `confirmed` (“Confirmada pela sua
mensagem”) é a lembrança que o dono aprovou no app depois que o Hub a achou sustentada por uma mensagem
dele: não é fala literal, e o topo da memória e as instruções do bot dizem “confirmada pela mensagem do dono”,
nunca “o dono disse”. Literal quer dizer **frase
inteira** digitada ou ditada (`write_gate::said`: separada por `.` `!` `?` `;` ou quebra de linha, nunca por
vírgula; do fato só se ignora a pontuação final), na extração por turno, nas propostas do MCP, nas sugestões e
no atalho do sono. Um trecho dentro de uma negação ("Não moro em Curitiba; moro em Recife." não dá “Você disse”
a "moro em Curitiba") ou de uma condição fica inferência.
Uma sugestão do extrator só pode indicar inferência ou conflito; não pode promover
um fato a certeza. Uma revisão inferida que muda o texto conserva o texto ativo e
a alternativa marcada como conflito. Versões guardam data, revisão, origem e
natureza anteriores. Desfazer restaura esse conteúdo/estado/origem/natureza se o
item não mudou depois; a restauração ganha nova revisão e data, sem apagar a
versão anterior.
As instruções e o contexto entregues ao motor preservam esses rótulos.

No Codex, o fluxo automático não usa o MCP Memory dentro da sessão: ele acontece
no Bot de fundo depois do sucesso do turno. Não há fallback para chamada MCP ou
arquivo direto. A futura revisão/arquivamento manual pelo chat exige recorte e
especificação próprios de autoridade, antes de código. O aceite de confirmações
nativas é condicional às operações realmente disponíveis; `send_message` pertence
ao Chat, permanece separado e mantém sua política nativa. Isso não promete zero
confirmações nativas em todas as operações do motor.

A proposta preparada guarda o vínculo da execução que a criou; vínculo revogado ou sem prova = preparado não salvo.

O Hub conserva destino e escopo; desabilitar a memória compartilhada ou mudar a
conversa não redireciona o fato silenciosamente. O dream só consome a prova durável
de sucesso da mesma fila; Flush ou compactação não tornam um turno falho elegível.

A fila guarda referências, não uma cópia do histórico: até 100 por bot/Space e
1.000 por Hub, com expiração em 30 dias desde a admissão. Cartões antigos recebem
disposição durável sob os mesmos limites. Falta de prova de sucesso não é
reclassificada como sucesso. O histórico permanece. A interface distingue espera
por capacidade, configuração compatível ausente e fila cheia/expirada; só a
primeira promete tentar de novo quando houver capacidade. Uma nota antiga sem
prova exibe apenas diagnóstico genérico, sem expor o corpo nem recriar aceites.

O vínculo Space→seleção referencia a escolha de função e a conta existentes,
com suas revisões; não duplica catálogo nem permite fallback global. Sua gestão
usa Local ou a capacidade original `engine.accounts` com guarda SAAS-04, nunca
MCP. O executor de fundo oferece execução isolada somente em Pessoal sem contexto e assinatura
Claude elegível. Sua consulta oficial separada antes/depois não comprova a conta
do processo ACP emissor. Por isso, o cadastro de vínculo não libera envio em
outro Space: falta essa prova do próprio emissor, incluindo o conteúdo enviado
na criação da sessão. Hash, TTL e uma conta consultada em outro processo não bastam.
Codex sem seleção compatível e Spaces ainda sem prova permanecem indisponíveis;
a fila não elimina essa limitação nem apaga o histórico.

Configurações → Bot de fundo mostra escolhas e conferências que a API realmente
oferece. Disponível no catálogo não garante cota, identidade ou execução atuais.
O histórico local do executor, quando entregue pelo Hub, limita-se a 50 registros:
status obrigatório, modelo efetivo possivelmente ausente, etapa de falha opcional
e uso opcional em tokens. Não há preço ou causa detalhada inferidos pelo cliente.
Roteamento de pedidos e bloqueio por papel ficam para uma etapa futura.

## O que os bots aprendem

Ajustes → **Memória** junta num cartão o que antes ficava espalhado:

- **Modelo da memória:** motor e modelo da função `memory_consolidation` do Bot de fundo. É a mesma
  seleção da tela Bot de fundo (`functionModels`/`saveFunctionModel`); o cartão não guarda uma cópia.
  O padrão continua Haiku com esforço baixo. A nota mostra a frequência real: a extração roda depois de
  cada turno do dono que deu certo.
- **O que ele aprende sobre você** (`memory.learning`, uma escolha do Hub para todos os bots, inclusive
  o Berga):
  - `auto`, **Grava sozinho, com Desfazer**: o padrão.
  - `suggest`, **Vira sugestão pra você aprovar**: cada mudança que o bot faria (extração do bot de fundo
    ou `remember`, `revise_memory`, `archive_memory` e `edit_memory`) vira um item pendente, com o mesmo
    texto, origem, natureza, evidência e escopo. Antes de guardar, a operação passa pela mesma régua do
    automático (alvo, camada, revisão, compartilhamento, origem externa, fato esquecido), num ensaio que é
    desfeito: um alvo de outro bot ou Space é recusado e nada aparece. O texto mostrado é o que foi
    conferido nessa hora, nunca uma leitura nova pelo ID. Guardar uma sugestão faz a tela recarregar
    (`memoryRev`) sem avisar as sessões do bot de que algo virou memória. O pendente fica numa tabela própria
    (`donna_memory_suggestions`): não entra no contexto de abertura, no `search_memory`, nas notas nem nos
    tetos de tokens. **Aprovar** aplica como o modo automático (recibo **Memória salva · Desfazer**,
    versão), pelo caminho do dono: não vira proposta automática nem depende do vínculo da execução que a sugeriu;
    as propostas das ferramentas já tiveram o vínculo conferido antes de virarem sugestão. Se a lembrança
    ou a nota mudou depois da sugestão, aprovar recusa com aviso e não
    sobrescreve. **Descartar** guarda o item como descartado, e o mesmo fato não volta a ser
    sugerido para a mesma memória. As sugestões aparecem na aba **Sugestões** da Memória do bot.
  - `off`, **Não aprende nada**: nenhum pedido entra na fila e o modelo não é chamado, então não gasta
    cota. Pedidos admitidos antes da troca esperam na fila sem rodar; um trabalho que já esperava a vaga do
    executor confere o modo de novo no preparo e logo antes do envio, e volta a esperar sem ocorrência
    nova em Não salvei. As ferramentas de memória recusam
    com `memory_learning_off`, sem fingir que salvaram.
- A nota fixa “Só conta o que você disse, nunca o que um agente supôs” resume a regra de evidência
  humana e de natureza descrita acima.

A edição manual do dono no app não muda em nenhum modo. O modo (`memoryLearning`, `saveMemoryLearning`) e
as sugestões (`approveMemorySuggestion`, `discardMemorySuggestion`) são do dono: no próprio computador
(Local) ou no app pareado com Control, como o Mac ligado ao Hub do servidor. A autoridade ORIGINAL do pedido
é capturada no dispatch, antes de qualquer espera, e conferida de novo na mesma transação que grava
(`Autoridade::with_db`, ordem MB → dono na Cloud → DB); revogado o aparelho no meio, nada muda (nem
memória, nem recibo, nem o status da sugestão). A ponte MCP e o bot não têm Control e são recusados. O
modelo da memória continua na regra herdada do Bot de fundo, só pelo acesso local.

## Linhagem da memória

Toda mensagem que já sustentou um fato ou uma nota fica registrada em `donna_memory_lineage`
(`kind` = `fact` ou `note`, `item`, `message`, `rev`). Entram a evidência de **qualquer** versão
do fato e as fontes da origem (`sources[].messageId`), que já trazem a procedência herdada e as
duas pontas de um conflito; nas notas, as fontes da origem de cada versão.

**Só identificadores opacos.** O `item` de um fato é o seu id. O de uma nota
é um código aleatório de 128 bits (`memory_file_versions.note_key`), criado quando a nota nasce, sem nada
derivado do nome ou do caminho. O vínculo código ↔ camada e nome fica só nas versões da nota e some
com elas (esquecer, limpar, excluir o bot); a linhagem não guarda nome, caminho nem texto. Uma nota
apagada e recriada com o mesmo nome ganha um código novo, e o antigo nunca é reusado.

- **Quem grava:** gatilhos do SQLite em `memory_entries` e `memory_file_versions`, na mesma
  transação da escrita. Cobrem todas as rotas
  (extração, MCP, edição do dono, fixar, arquivar, ativar, Desfazer, `undoDream`, notas, "Sobre
  você") e as rotas futuras, sem código por rota. O SQL fica em `host/crates/memory/src/lineage.rs`;
  `schema.rs` só chama a migração 4 e recria os gatilhos a cada início, como o índice de busca. A
  migração dá um código a cada nota existente (um por vida da nota) e traz a linhagem do que já
  existia, com a menor revisão em que cada mensagem apareceu (evidência ou origem).
- **Nunca faz a escrita falhar:** evidência ou origem malformadas (JSON inválido, `sources` que não
  é lista, `messageId` que não é texto) só não geram linha.
- **Só acréscimo:** sem chave estrangeira; UPDATE e DELETE na tabela são recusados. A
  linha fica depois do DELETE físico do item (Desfazer de criação, esquecer, retirar, limpar,
  excluir bot). O `rev` é o da primeira versão em que a mensagem apareceu; reescrever em lote
  (`undoDream`, restaurar) tenta um INSERT por fonte de cada linha alterada e não cria linha nova
  se a fonte já estava lá.
- **Quem lê:** o sono (abaixo), pelo `sleep_checks::valid_for_cleanup`.

## Sono de checagem

Uma vez por dia de calendário por bot, com o bot parado (nenhum turno vivo, nenhuma extração do bot na fila e a
última mensagem de qualquer conversa dele com 30 min ou mais), o dreamer agenda uma rodada do sono, sem timer
novo. Com `off` ela não agenda nem roda. A única mudança de memória é o atalho literal (abaixo); a
rodada do modelo (abaixo) só marca lembranças mantidas e grava um recibo, sem mudar nenhuma.

- **Fila própria** (`donna_memory_sleep_jobs`), separada da extração: a reserva, os limites, a expiração e a
  auditoria da extração não mudaram e nunca veem um trabalho de sono, então o sono nunca bloqueia nem passa à
  frente de uma extração. O sono só é reservado sem extração esperando no Hub e com um só sono por bot. Não
  conta a extração já tentada e devolvida por falta de seleção (`attempts>0` e `noSelection`): parada à espera
  da seleção de um Space, ela seguraria o sono de todos os bots.
- **Por mensagem** (`donna_memory_sleep_checks`): uma mensagem humana da conversa, na revisão exata, vira
  `covered` se está no contexto de um trabalho terminado (`applied`, ou `noChange` com a prova
  `learningOn` do commit; escopo permitido) e não
  sustenta nenhuma lembrança duvidosa (conflito, inferência, data ou prazo, fixada, não escrita pelo dono, na
  camada da memória ou em "Sobre você"); vira `excluded` se o dono bloqueou (`memory_blocked`). O resto não ganha
  linha e segura o cursor: trabalho na fila, vencido, fora da fila, turno falho, pergunta ao dono.
- **"Sem mudança" só com prova.** O commit da extração grava `proof='learningOn'` no
  `noChange` quando o aprendizado estava ligado nessa transação e nenhuma operação foi ignorada ou recusada
  (confirmar vindo do extrator, duplicata, fato esquecido) ou ficou sem efeito (arquivar o já arquivado). Nesses casos o job recebe a marca `proof='unproven'`, também quando é `applied` num lote misto: as
  escritas válidas e os recibos ficam, mas o sono não toma esse job como prova de que a mensagem passou pela
  memória. Com `off` lido no commit, as operações são
  descartadas e o pedido fecha como `ineligible` ("Não aprende nada": nada agora nem depois, sem guardar
  propostas), nunca `noChange`; a mensagem não conta como lida e fica retida. Um `noChange` antigo, sem a prova, não conta: a mensagem fica pendente até outra prova.
- **Cursor** (`donna_memory_sleep_cursor`): o maior `seq` até onde toda mensagem humana tem checagem válida; só
  avança, na mesma transação das checagens. Rodada interrompida não grava nada e recomeça do mesmo ponto.
- **Lote:** até 500 mensagens provadas por rodada; as seguradas não gastam essa cota, com teto de 5.000
  lidas, para uma conversa com muitas seguradas antigas não deixar as novas sem checagem.
- **Atalho literal:** até 20 lembranças por rodada que são inferência **sem conflito**, escritas pelo bot,
  sem origem externa. Antes de ler qualquer texto, o Hub exige a **linhagem inteira** nesta conversa: toda
  mensagem que já sustentou o fato existe e é desta conversa (fonte de outra conversa da mesma memória exclui o
  candidato), e cada versão do fato tem recibo de um trabalho do mesmo bot, conversa, Space e memória, com a
  evidência no contexto desse trabalho na revisão atual da mensagem. Versão sem recibo (legado, edição do dono,
  fixar, Desfazer) exclui. Passou: se o texto é **igual a uma frase inteira** (frases separadas por `.` `!`
  `?` `;` ou quebra de linha, nunca por vírgula; do fato só se ignora a pontuação final) de uma mensagem do dono
  **limpa por inteiro** (digitada ou falada, sem colagem, anexo ou resposta citada) e a sessão não está exposta a
  conteúdo externo, nem na marca inicial nem na atual, em `auto` a lembrança
  vira "Você disse" (operação `confirm`, só do Hub: o extrator e o MCP não a usam), com versão nova, cartão
  **Memória salva · Desfazer** e recibo preso ao trabalho de onde veio a evidência; o Desfazer volta à inferência
  e bloqueia o fato daquela mensagem, então o sono não confirma de novo. Em `suggest` vira sugestão
  ("Confirmar: você disse isso") e não toca `memory_entries`. "Não moro em Curitiba; moro em Recife." confirma
  "moro em Recife" e nunca "moro em Curitiba"; uma condição ("Se eu mudar..., moro em Curitiba.") também não.
  Candidata conferida pelo atalho sem passar fica marcada nessa revisão (`donna_memory_sleep_facts`, só ids e
  revisão) e não ocupa de novo as 20 vagas; a marca vale só para a conversa conferida, então
  uma lembrança de "Sobre você" recusada pela linhagem de outro bot continua candidata onde tem a prova. Confirmar em "Sobre você" avisa todos os bots que veem a
  camada. Mensagem mista não vira "Você disse"; conflito
  nunca usa o atalho (um literal do texto antigo não elimina a alternativa). Confirma só por fala humana do
  contexto, nunca pelo cartão, resumo ou revisão do próprio sono. O atalho roda sob a mesma guarda do Space que a
  extração (MB → Owner/Cloud → DB); sem a seleção do Bot de fundo autorizada, a rodada só faz as checagens.
- A rodada guarda um resumo (cobertas, excluídas, seguradas, cursor, quantas a limpeza poderia apagar, confirmadas e
  se sugeriu).
- **Rodada do modelo (modo relatório):** só na conversa
  principal do Pessoal, pelo executor de fundo da memória (função `MemoryConsolidation`, vaga, cota e trava de
  75% de sempre). No preparo da entrada e de novo em cada fronteira de envio, o Hub confere a reserva do sono, o
  Context original da reserva (revisão do perfil do bot, conversa, Space e memória: um perfil salvo durante a
  espera recusa antes de ler ou enviar), a seleção da função, o modo diferente de `off` e nenhuma extração esperando; se chegou extração, a rodada é devolvida sem gravar nada nem contar
  tentativa. Só roda no adaptador Claude, o único com barreira pré-uso no executor (`tools: []`,
  `disallowedTools: ["*"]`); em outro adaptador não envia nada e o recibo diz `adapterNotContained`, sem trocar de
  modelo. Vão até 20 lembranças duvidosas, não escritas pelo dono, da camada do bot ou de "Sobre você" (só com o
  compartilhamento ligado AGORA no perfil do bot, conferido também na gravação), com a linhagem inteira validada
  nesta conversa e ainda não conferidas nesta revisão, mais as mensagens humanas da linhagem (só dado, redigidas, com ordem e
  origem do Hub). A origem é `user` só para mensagem limpa por inteiro numa sessão não exposta; o resto chega
  como `external` e nunca sustenta um veredito. A resposta é validada inteira (schema exato, no máximo 20, uma por
  lembrança, `text` só em `corrects`, evidência só entre as fontes dela): inválida, nada marca. A gravação usa a autoridade ORIGINAL da chamada (o `Prepared`
  do Space e a seleção usada pelo executor, reconferidos; nunca uma seleção resolvida de novo): seleção trocada
  depois do envio = nada do modelo é gravado. Na transação da rodada, sob MB → Owner/Cloud → DB e com as versões
  reconferidas, só um `undecided` válido marca a lembrança como conferida nesta revisão
  (`donna_memory_sleep_kept`, com as mensagens da linhagem e suas revisões), e ela deixa de segurar as mensagens.
  A marca cai se qualquer mensagem da linhagem mudar, sumir ou entrar depois. O histórico da chamada só fecha
  como sucesso depois desse commit. `supports`, `corrects`
  e `changed` válidos (evidência humana não vazia, natureza certa, texto proposto que passa no filtro e não perde
  negação, condição, número ou mês; em `changed`, vencedor pelo maior `seq`, escolhido pelo Hub) ficam **só no
  recibo** (`donna_memory_sleep_jobs.receipt`: ids, revisões e motivos de lista fechada, sem texto nem saída do
  modelo). Um `supports` válido sobre uma inferência do bot, sem conflito, com origem conhecida e não
  externa e evidência no trabalho da linhagem, vira **sugestão** "Confirmada pela sua mensagem" (operação
  `reclassify`, só do Hub; em `suggest` e em `auto`, nunca aplicada sozinha), presa a esse trabalho e com as
  mensagens da linhagem e suas revisões. O recibo leva só o id dela. Aprovar (com a Authority original até a Tx)
  reconfere a lembrança e a linhagem (mudou = desatualizada, nada aplicado) e grava `confirmed`: mesmo texto, versão
  nova, recibo e Desfazer, que volta à inferência. `corrects` e `changed` ainda ficam só no recibo. Resposta com uso de ferramenta marca a tarefa exposta e grava
  só `failed`; o teto de 75% (ou a cota ilegível) recusa antes do envio e grava `limit`, enquanto a recusa
  da assinatura durante a chamada devolve a reserva. Nos dois casos as checagens e o atalho
  do dia seguem, e a rodada do modelo volta no dia seguinte.

**Contrato para a limpeza da conversa:** o DELETE de mensagens da limpeza chama
`sleep_checks::valid_for_cleanup` **na mesma transação** do DELETE e só apaga o que ela aprovar. **Pronto**, com a validação da linhagem inteira. O predicado usa sempre a checagem mais recente da revisão atual de cada mensagem.
Mensagem cuja linhagem inclui uma nota, ou um fato ou nota que já não existe, é retida e vai
para a lista à parte "mensagens que nunca viraram memória"; mudar um fato invalida a checagem de
toda a sua linhagem. Sem essa chamada, a limpeza não apaga.

**Contrato por turno:** as entradas que **não** são do dono (respostas do bot, cartões,
avisos) passam por `sleep_checks::turn_valid_for_cleanup(conn, entry_id)`, também **na mesma transação** do
DELETE, só leitura e só pela Connection recebida. Ela só aprova se:
1. o turno da entrada tem **pelo menos uma** mensagem do dono e **todas** passam em `valid_for_cleanup` (turno de
   rotina, pedido entre bots, mensagem só do bot ou só com o cartão de pergunta → não);
2. a entrada não é um cartão de memória nem um recibo: o cartão "Memória salva · Desfazer" tem o mesmo id do
   recibo em `donna_memory_auto_receipts` (qualquer status) e é dele que o Desfazer depende; os cartões legados
   (`memory` de fato, `memoryFile` de nota) também ficam. E o id dela não aparece na linhagem, na evidência ou na
   origem de nenhuma lembrança, versão ou nota, em nenhum trabalho de memória aberto (`waiting`, `reserved`,
   `running`, `owner`), nos `responseIds` de nenhum cartão de memória, no "não salvei" nem em sugestão. Enquanto
   o cartão existir, a resposta citada nele fica;
3. o turno está encerrado (nenhum trabalho `running`, a sessão do turno assentada), sem pergunta ao dono
   pendente e a entrada não é um cartão de escolha genUI **pendente** nem o marcador ou a origem dele
   (`donna_genui_questions`): é o vínculo canônico da pergunta. Respondida, cancelada ou expirada, vale
   a regra normal. A mesma retenção vale em `valid_for_cleanup` para a **mensagem do dono** que é a origem de uma
   pergunta pendente (o mesmo critério, `sleep_checks::pending_choice`, nos dois predicados).
A mensagem do dono continua decidida só por `valid_for_cleanup`; esta função não decide nada sobre ela. Na
dúvida, não aprova. **Limites conhecidos:** a identidade visível da sessão não é comparada, e um
turno partido entre sessões (ou sem o registro da sessão do turno) não tem caso próprio; sem o registro a função
não aprova. As mensagens "que nunca viraram memória" (`excluded`, retidas por nota ou item apagado) só
são **informadas** (lista à parte da prévia); ninguém as apaga sem o dono. A rodada do sono conta, na
prévia, quantas respostas a limpeza poderia apagar (`cleanableReplies`), sem efeito.

## Histórico de “não salvei”

O aprendizado automático mantém um histórico de “não salvei” com motivos fechados, incluindo
duplicata, bot errado, não selecionado como fato duradouro e falha técnica adiada,
preservando as disposições honestas já existentes. Não cria fila humana. O
registro contém metadados/IDs escopados, nunca o segredo recusado, seu hash ou
cópia de trecho externo bruto. Desfazer conserva os tombstones por fato/evidência.
Consultar esse histórico exige bot/Space/audiência antes da ordenação; não revela
lembrança existente em outro escopo. Adiamento técnico só descreve
falha recuperável, não uma recusa permanente. Ausência de motivo não vira diagnóstico
inventado. Interface de memória apenas apresenta as decisões do Hub.

### Vínculo das ferramentas com a sessão

O catálogo de leitura `__tools` do MCP memory pode ser consultado durante a
preparação canônica da abertura, conservando a mesma captura até a resposta.
A exceção não lê memória/histórico nem prepara mutações, não substitui capability,
generation/backend do transporte e não anuncia ferramentas omitidas pelo motor.
Com ACP presente, o Hub exige vínculo ativo, sem alternativa pela preparação.

O Hub identifica a sessão de origem das chamadas das ferramentas internas, do gateway e dos pedidos de permissão ACP. Uma chamada revogada ou vinda de outra sessão do bot é recusada como **chamada vencida**; ela não publica mensagens, reações ou arquivos na conversa atual, não usa o pedido do dono de outra sessão para gerenciar bots ou computadores e não lê nem grava memória ou histórico em nome dela. O histórico continua limitado ao turno humano no chat próprio do bot; `computers` continua somente na principal.

O vínculo acompanha a chamada durante esperas e é conferido também na transação que grava. Conteúdo externo atrasado marca apenas a sessão de origem; uma abertura pendente não marca a sessão que rodou depois. O cartão de permissão conserva o `sessionId` e só pode autorizar a sessão que mantém a espera.

A sessão de memória de uma conversa (principal ou extra) perde o vínculo quando a seleção efetiva dela passa a outro motor ou outra conta, no início do turno novo que promove a troca; editar o padrão do bot (para a conversa com seleção própria), trocar só modelo ou opções e gravar só a próxima seleção não a afetam, os vínculos de rotina, canal e Space seguem as próprias regras, e o motor seguinte abre com vínculo novo.

Isso fecha o vazamento entre sessões. A correlação de chamadas atrasadas de dois turnos **na mesma sessão** continua sendo uma limitação anterior. Nenhum motor perde ferramentas da principal por falta de token de prompt. O fluxo automático usa um token de transporte só no MCP memory para geração/replay; ele permanece separado do vínculo de sessão e não habilita mutantes omitidos pelo motor.

## Medidor de contexto

A barra fina abaixo do compositor, alinhada à direita, mostra o uso **informado pelo motor** para a conversa
visível, com percentual, tokens usados e tamanho da janela. Acima de 90% fica
vermelho. O balão abre com ponteiro, foco ou clique; teclado e leitor de tela
recebem o mesmo controle. Não consulta tabela de tamanhos de modelos. Motor sem
`size` não mostra medidor, mesmo que tenha informado uma janela em outro turno.

O balão informa o ponto em que o Hub comprime automaticamente, pela própria
`Conversation::trigger` do crate de memória, e o horário da última compressão.
Uma nova compressão pulsa o anel uma vez; movimento reduzido desliga a animação.
Não há cartão nem linha nova no chat: o medidor fica abaixo da caixa; horário só no balão e pulso.
As alternativas de cabeçalho e linha no chat foram descartadas.

`donnaContext` recebe `botId` (destino do chat) e `sessionId` opcional; ausência ou
ID da conversa indica a principal. Adicionais de Space podem usar `conversationId`
com a identidade do bot. O resultado tem `hubId`, `botId`, `sessionId`, `scopeId`, `epoch` (época de compactação) e, quando medido,
`context`: `used`, `size`, `updatedAt`, `trigger`, `lastCompaction {at,byHost}`
(quando há), `canCompact`, `busy` e `contextId` opaco. Uso e tamanho são os valores
brutos do `usage_update`, não custo, tokens processados na assinatura nem estimativa.
A observação é guardada por Hub/bot/ACP e traz a conversa visível, a época de
compactação e a seleção do motor. Só uma medida da época atual é publicada;
após um resumo, sem novo `usage_update`, o campo `context` fica ausente. A época
vem da mesma chave canônica de `context::bump_epoch`, sem contador paralelo.
Troca de sessão, Hub, conta/modelo ou motor não herda números da conversa anterior.
A prop de UI é `sessionId: string | null`: `null` é a principal e uma string é
enviada exatamente como foi selecionada. `conversationId` só vem de um campo
explícito de destino; nunca é inferido de `sessionId`.

`donnaContextCompact` recebe o mesmo destino e o `contextId` da medição. A
permissão é a mesma de mandar mensagem naquela conversa. O ator recusa turno,
espera, fila, admissão bloqueada, sessão de outro destino/motor, adaptador sem
`/compact` e repetição vedada pela regra atual. A captura é conferida outra vez
no despacho e após os awaits do preparo, antes de enviar qualquer comando ao ACP.
Não usa o campo do bot atualmente aberto como fallback de destino.

O app valida o Hub da resposta, o `botId`, a `sessionId` exata, `scopeId` e `epoch`;
respostas de época anterior são descartadas na mesma conexão e no mesmo escopo ACP; medida ausente
esconde a barra, sem reaproveitar a principal para uma extra.

A ação entra no `Queued::Compact` e usa o `start_compaction` existente: Claude
recebe `/compact` com `KEEP`; Codex recebe `/compact` com o KEEP no canal nativo.
Conserva a atualização do contexto e o reenvio de perfil/memória. A conclusão conta
como `byHost` na trava atual contra repetição. Só a compressão manual capturada
pode usar esse caminho oculto numa extra; outros turnos ocultos continuam recusados.
Não altera os limiares automáticos nem cria o ajuste de compressão.

O app lê a RPC só enquanto o componente está montado, online e a janela visível,
a cada 2 segundos. Hub antigo oculta o controle e interrompe a consulta nessa
conexão. Resposta de um Hub/principal/sessão anterior é descartada. É uma leitura
própria: nada foi acrescentado a bot, entrada ou hello, portanto **MIRROR_SCHEMA
não muda**.

Ajustes → Conversa recebe “Mostrar o medidor de contexto”,
ligado por padrão neste aparelho; desligado, o componente não consulta o Hub.
O compositor de destino repassa `sessionId` da aba visível por
um gancho de uma linha: `<BarraContexto botId={botId} sessionId={sessionId} />`.
A principal medida continua medida ao voltar de uma extra sem medição; a extra
fica oculta, sem fallback. Os textos usam `useT` e chaves `contexto.*` nos
catálogos reais, com números e horários no idioma escolhido.
Não há catálogo paralelo nem stub de tradução.

O RPC de contexto publica `scopeId` opaco por Hub/bot/lane/ACP/motor, estável entre
compressões. `epoch` só é comparável dentro desse escopo: Nova sessão ou rollover
troca `scopeId`, aceita época 0 e oculta a medida anterior até chegar uso real da
nova ACP. `contextId` continua prendendo a ação à captura e inclui a época.

### Ajustes e pausa depois de falhas

Os ajustes de compressão pertencem ao Hub inteiro e só o dono (Local ou Device
com Control) pode consultá-los ou alterá-los. `donnaContextSettings` lê e
`donnaContextSettingsSave` grava `triggerTokens` (null = regra automática do
crate; inteiro em milhares, mínimo 50 mil) e `idle` (`beforeExpiry` ou `never`).
O ponto escolhido é aplicado às decisões existentes de fim de turno e à trava
de repetição; `trigger` da medição mostra esse valor efetivo. "Nunca" desliga
somente a compressão preventiva das conversas paradas. KEEP e o reenvio de
instruções e memória continuam no caminho já existente.

Duas falhas consecutivas de compressão automática pausam novas tentativas
(fim de turno e ociosa). A contagem fica no banco por bot e lane visível,
independente da ACP, e sobrevive ao reinício. Outra conversa não é afetada.
A checagem também acontece imediatamente antes da tentativa, para impedir
que uma compactação automática já enfileirada escape da pausa. Nova mensagem
do usuário naquela conversa zera a contagem; um resumo bem-sucedido também.
Falha manual não aumenta a contagem automática. A medição acrescenta
`context.automaticPaused` e `context.automaticFailures`; o balão explica a
pausa e mantém Comprimir agora quando suportado. Não aparece cartão no chat.

## Custo observado

O balão do medidor consulta o custo registrado da mesma conversa e sessão visível. Dinheiro só aparece quando o motor informou uma diferença válida de custo; caso contrário aparecem os tokens disponíveis. A nota explica que é equivalente estimado de API, não cobrança por token da assinatura. Ajustes → Uso apresenta o mês UTC por bot, conversa e sessão ajudante, sem duplicar os filhos no total do Hub. Veja [Custo estimado](custo-estimado.md) para fontes, lacunas, autoridade e limites. Nenhuma ferramenta de custo foi disponibilizada aos bots.

### Perfil consolidado e envelhecimento

O Host monta um perfil curto por bot a partir dos fatos ativos que esse bot pode
ver, em duas partes: **Sobre você** e **Como trabalhar com você**. É uma seleção
extrativa determinística em Rust, sem chamada a modelo e sem inventar sínteses.
O bloco inteiro tem no máximo 2.400 caracteres Unicode, incluindo títulos,
marcas e alternativas de conflito. Cada fato entra inteiro ou fica de fora;
`search_memory` continua disponível para consultar o restante. A classificação
entre as duas partes usa palavras de preferência e comunicação; é uma heurística,
não uma nova conclusão sobre o dono.

O perfil entra em todo turno do bot. O cache é separado por identidade de memória
e uso da camada compartilhada. Só mudanças nos fatos visíveis invalidam esse
cache, na mesma transação da escrita, inclusive edição de natureza/origem,
arquivo, restauração e exclusão. Notas longas continuam no índice existente.
Marcas de dito, inferido, confirmado, desconhecido e conflito são preservadas;
conflitos mantêm os dois lados, sem escolher vencedor. Origem externa ou
desconhecida nunca ganha autoridade. O filtro atual de conteúdo e segredos é
reaplicado ao montar o perfil, inclusive ao legado e à alternativa de conflito.

No aprendizado automático, o agendador existente confere o envelhecimento a cada
hora por bot, quando não há extração ou revisão pendente, inclusive após reinício. Uma inferência escrita por bot ou sono,
com exatamente uma fonte, sem fixação, conflito ou restauração pelo dono, pode
ser arquivada após 90 dias sem alteração/reforço. Datas ausentes, fatos humanos,
legados de natureza desconhecida, fatos ditos e confirmados ficam protegidos.
O modo de sugestões e o aprendizado desligado não executam esse arquivo.
O fato arquivado continua pesquisável e restaurável pela tela de memória; suas
versões e um registro de motivo/data/revisão permanecem no banco.

Uma repetição literal em nova mensagem limpa do dono, comprovada pelo Hub no
fluxo de extração existente como frase inteira, acrescenta evidência e reforça o fato ativo. A mesma
fonte não conta duas vezes. Reforço não muda a natureza nem resolve conflito;
fatos com múltiplas fontes deixam de ser fracos para essa regra. Não há busca
semântica de equivalência. Arquivados não são reativados por repetição.
A migração é aditiva, transacional e versionada/idempotente; não chama modelo,
não lê arquivos privados e não modifica permissões ou aprovações.

O perfil resumido nunca é enviado em turnos de Canal. Continua disponível em
conversa pessoal, grupo do dono, rotina e pedido de outro bot do mesmo dono.
O teto de 2.400 caracteres cobre o bloco inteiro; o separador de dois newlines
entre ele e o restante do prompt fica fora da cota. Sem conteúdo na seção
“Como trabalhar com você”, “Sobre você” pode usar toda a cota disponível.

### Sobre você

O Hub guarda uma nota de cinco seções por pessoa e Space para Ajustes › Memória. A chave é a pessoa mais o Space; hoje o Hub conhece uma pessoa só, o dono, e membros de Equipe e convidados terão a própria nota na mesma chave, sem migração (ainda sem tela). Nota, revisão, exclusão e lembranças soltas (`loose`) são dessa chave; não existe nota global herdada. A marca interna de formato não aparece nas projeções legíveis. A edição pertence ao dono pelo app; ferramentas de bot não editam essa nota.

A nota só entra na conversa da própria pessoa. A pessoa vem apenas da origem que o Hub carimba no envio a partir do chamador autenticado (`contentOrigin.ingress`): o computador do Hub e os aparelhos com controle são do dono. Mensagem pela Ponte, sem origem, de outro bot ou com pessoas diferentes no mesmo turno não identifica ninguém, e aí nenhuma nota entra. Pedido de outro bot, rotina, retomada e resposta a cartão de escolha não são remetentes: seguem a pessoa da sessão em que rodam; uma sessão aberta por eles não tem pessoa. Canal pode ter várias pessoas e não recebe nota. Cada sessão guarda a pessoa com quem falou; uma sessão que ouviu a nota de uma pessoa nunca é reaproveitada por outra nem por remetente desconhecido: a conversa abre sessão nova. Na primeira vez que uma sessão já aberta passa a ter pessoa, a próxima mensagem leva a memória inteira com a nota dela.

Quem pode receber, além disso: no Pessoal, os bots com memória compartilhada ligada (como antes); nos outros Spaces, todo bot do Space, porque lá o interruptor da memória compartilhada só recusa a memória do Pessoal. Cada bot recebe apenas a nota do próprio Space. Grupo, execução com identidade alheia (bot de um Space numa conversa de outro) e chamada interna sem Space não recebem nota nenhuma, nem a do Pessoal. Uma mudança avisa só os bots do Space da nota, no próximo turno. No Pessoal, `read_memory` só entrega a nota com pessoa comprovada; a nota não é projetada na pasta compartilhada. Nos outros Spaces ela não tem arquivo, pasta nem ferramenta.

Na atualização, sessões antigas sem marca de pessoa são abandonadas por segurança. Sua continuidade do motor e o histórico anterior ao corte não são retomados; a pessoa pode precisar fornecer o contexto novamente. As mensagens originais e a nota no diário permanecem. Alternar entre uma pessoa comprovada e pedidos sem pessoa pode recriar sessões e aumentar o custo de preparo do contexto.

O aprendizado automático segue o Space da conversa e a pessoa das mensagens de evidência do job, com a mesma regra de origem: sem pessoa comprovada, o extrator não lê nota nenhuma (nem a do Pessoal pela camada compartilhada); com ela, lê só a dessa pessoa no Space da conversa. Ele não grava a nota (`memory_user_owned`) nem a camada compartilhada fora do Pessoal (`shared_memory_is_personal`).

`memoryProfile {spaceId?}` retorna `spaceId`, `content`, `revision` e `loose` da nota do dono. `saveMemoryProfile {spaceId?, content, base}` exige a revisão lida do mesmo Space; uma revisão antiga ou de outro Space retorna `memory_profile_conflict`, inclusive entre duas gravações simultâneas com a mesma base. Sem `spaceId`, vale o Pessoal; `spaceId: null` retorna `invalid_space` e Space inexistente ou excluído, `space_not_found`. O Space e a revisão são reconferidos na transação da gravação. A gravação segura a mesma trava da exclusão do Space do commit até o último efeito (projeção, avisos aos bots e evento): a exclusão acontece antes, e a gravação recusa sem efeito, ou depois, quando gravação e efeitos já saíram e ficam registrados. O conteúdo nulo apaga a nota. O Hub recusa texto identificado como segredo com `secret_detected`, sem devolver o trecho. Essa proteção também vale para o dono. Fora da nota do dono no Pessoal, `loose` é sempre 0: lembranças soltas existem só na camada compartilhada, que é do Pessoal.

Armazenamento e migração: a nota, o histórico e a lápide existentes continuam onde estavam e passam a ser os do dono no Pessoal, sem cópia; os outros Spaces começam sem nota. As demais notas ficam em camadas internas próprias do diário (`profile:<pessoa>:<spaceId>`), sem pasta no disco e fora das listagens e ferramentas de memória dos bots.

### Disponibilidade por Space

O evento `memoryProfile` (`spaceId`, `revision`, sem conteúdo) chega só ao stream do Space da nota. A capacidade `spaceMemoryProfileContract` nos dois `hello` e a integração do cliente ainda não estão disponíveis. Até lá, o app continua chamando sem `spaceId` e vê só a nota do Pessoal.
