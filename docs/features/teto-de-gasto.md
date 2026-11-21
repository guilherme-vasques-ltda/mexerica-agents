# Teto de gasto

Teto por **período** que **trava o envio do prompt** quando atingido ou quando não dá para conferir. O período pode ser o mês civil UTC (padrão, o mesmo de [Custo estimado](custo-estimado.md)) ou o dia civil no fuso do dono. O trabalho para; o dono decide se continua. Não é cobrança nem bloqueio da assinatura: é uma cerca do Hub sobre o ledger `donna_turn_usage`.

**Cobertura:** o teto cobre o ledger ACP (turnos dos bots) e o trabalho de fundo (títulos, consolidação de memória, resumo do dia). Áudio/voz direta (chamada de voz em tempo real) **não** passa pela cerca nem grava no ledger — o teto não é "todo gasto".

## Configuração

`donnaCeilingSave` (só o dono) grava em `donna.spend-ceiling.v1`:

- `enabled` (padrão `false`).
- `money`: lista `{currency, limit}` — uma linha por moeda ISO de 3 letras, sem conversão cambial.
- `tokens`: inteiro opcional — soma de entrada+saída, sem cache.
- `warnPercent`: `70`, `80` ou `90` (padrão `80` — o mesmo que o primeiro `donnaCeiling` devolve, aceito pelo próprio `donnaCeilingSave`).
- `applyToConversations` (padrão `false`): quando ligada, a conversa do dono também passa pelo teto — **só antes do envio do turno, nunca no meio de uma resposta**.
- `period`: `month` (padrão; mês civil UTC, rótulo `YYYY-MM`) ou `day` (dia civil no fuso do dono, rótulo `YYYY-MM-DD@<fuso>`). Config antiga sem o campo vale `month`.
- `timeZone`: fuso IANA usado pelo período `day`. Ausente, o `donnaCeilingSave` resolve e grava o **mesmo padrão do resumo diário**: o `timeZone` registrado lá, senão o fuso local do Hub; IANA desconhecido é recusado (`invalid_ceiling_timezone`). Registro do resumo presente mas ilegível também falha — não há fallback silencioso. A janela do dia vai da troca real da data local à próxima (00:00–24:00 local): dias de 23 ou 25 h, meia-noite inexistente e até dia civil pulado no fuso caem no instante exato da troca.
- O limite, as métricas, o aviso (`donna.ceiling.warned.<período>`) e as liberações valem para a **janela efetiva** do período `[início, fim)` — com `day` a janela inclui o fuso no rótulo: trocar o `timeZone` muda a janela (liberação, aviso e geração de outra janela não se aplicam; voltar ao fuso na mesma data reencontra a mesma janela). Com `day`, a liberação do dia não vale no dia seguinte (o limite volta ao configurado) e o aviso sai uma vez por dia.

A detecção de datas locais descontínuas é amostrada: verifica a janela em passos de 10 minutos e os 6 dias anteriores ao início em passos de 30 minutos (288 pontos). Uma anomalia mais curta que o respectivo passo pode passar entre amostras; diminuir o passo não elimina essa limitação. Quando detectada, a janela falha fechada.

Configuração inválida (moeda fora do formato, limite zero/negativo/NaN, tokens zero, percentual fora da lista, moeda repetida) é recusada inteira.

Gravar uma configuração diferente é transição de estado: a geração do estado observado anda e nenhuma `checkFailure` anterior volta a valer (desligar e religar exige decisão nova). Os atores com espera são acordados para reaplicar a política na hora.

## Estado e regras fail-closed

Cada teto ativo é avaliado na janela vigente do período (mês civil UTC ou dia local) sobre o ledger:

- **Dinheiro** conta o informado do motor **e** o estimado por preço de API (quem usa assinatura também entra na conta). Só é `Verified` se *nenhum* turno da janela estiver sem preço. Um turno sem preço não diz a que moeda pertence, então ele torna **todas** as métricas de dinheiro `Unverified` (`semPreco`).
- **Tokens** só são `Verified` se *todo* turno tiver `input_tokens` **e** `output_tokens` informados; qualquer `NULL` torna a métrica `Unverified` (`tokensNulos`).
- Falha de leitura do ledger invalida cada métrica ativa.
- Limite efetivo do período = o **maior** entre o teto configurado e os novos limites escolhidos nas liberações de valor da janela (seção Liberações). Baixar o teto configurado não desfaz uma liberação da janela; um teto configurado maior prevalece. Limite efetivo não finito (dinheiro) torna a métrica `Unverified` (`limiteInvalido`) — nunca vira "sem teto".

Decisão:

- Valor **conhecido** ≥ limite efetivo de qualquer teto → `Blocked` `valueReached`, mesmo com dado parcial (o conhecido é piso, nunca teto).
- Qualquer teto ativo `Unverified` → `Blocked` `unverifiable`. Um teto conferido **nunca** substitui outro: dinheiro verde não abre tokens nem o contrário.
- Aviso `warn` quando o conhecido ≥ `warnPercent` do limite efetivo, uma vez por limite/período (marcador em `donna.ceiling.warned.<período>`), marcado também quando o envio passa; o evento sai só com marcador novo.
- Janela nova zera o estado: linhas fora da janela não contam e liberações são por janela efetiva.

### Uma conferência só

Todo caminho — permit, fronteira de fundo, reavaliação do ator, painel (`donnaCeiling`) e liberação — passa pela mesma `evaluate_on`: ela grava a geração do estado observado (inclusive saudável, falha de leitura e desligado `{período}|off`), marca avisos novos e apaga esperas de fundo quando o teto não trava mais. Por isso o painel mostra exatamente o que o gate faz (uma falha de leitura liberada aparece liberada) e uma `checkFailure` só vale enquanto nenhuma outra leitura viu o estado mudar.

## Onde trava

A decisão roda **dentro da transação do permit** (`ClaimHooks::gate` → `decide_on`): a mesma leitura SQLite que autoriza o despacho confere o teto — nenhum gasto ou mudança de configuração entra entre a conferência e a autorização. `send_admitted` é o único caminho de envio (turno novo e retry). Bloqueado:

- o prompt **não** é enviado e nenhum custo é gravado;
- na mesma transação, a reserva de admissão é apagada e a espera durável é gravada em `donna_ceiling_holds` — **uma linha por trabalho** (chave `(bot, trabalho)`: `routine:<run>`, `request:<pedido>`, `group:<chat>:<época>`, `human:<entradas>`, `recovery:<lane>`), com tentativa, origem e detalhe, lane, insumos do preparo, motivo e assinatura (`fingerprint#geração`); o mesmo trabalho travando de novo reaproveita a linha (e a ordem);
- o trabalho estaciona **fora** de `pending_admission` e da fila: origem isenta continua atendida — `Compaction` nunca trava; `Human` sem `applyToConversations` passa; autônomo (`Routine`, `Ask`, `Message`, `Group`, `Recovery`) trava sempre;
- o preparo humano é confirmado (`mark_human_setup_ready`) **antes** da conferência: parar pelo teto não consome o orçamento de preparo;
- indecisão do gate (leitura, configuração ou ledger indisponível) trava a origem coberta — falha de leitura recebe a identidade `readFailure:{erro}` e também é liberável;
- **o turno já em execução não é cortado**: quem passou no permit continua até o fim; a trava vale para o próximo envio;
- pedido entre bots (`ask`/`message`) só vira `running` (cerca+1) **dentro da mesma Tx do permit, depois que o teto passou**. Parado pelo teto ele continua `accepted`: o reinício o fecha como `restart_before_start`, nunca como possivelmente enviado.

## Espera durável e retomada

Cada linha de `donna_ceiling_holds` sobrevive a restart; no boot o ator reconstrói a lista na ordem das linhas (se a leitura falhar, o ator fica **fechado** — nada é admitido, nem o re-drive do agendador ou a recuperação de lane — e relê a cada ~1 s; valendo, o que entrou na fila nesse meio-tempo e já está parado sai da fila, a espera conduz), e `donnaCeiling` lista todas (`stopped`), com ou sem ator. A lista em memória é só o espelho das linhas (com o que não serializa: canal do pedido, turno de grupo). Trabalho segurado **nunca** é tratado como possivelmente enviado — a retomada reconstitui pela origem:

- `Routine` volta como item `Queued::Routine` (re-drive pelo caminho normal do agendamento). A execução fica `Starting` enquanto parada (a vaga de rotina do bot continua dela); após restart o agendador a reenvia e o ator ignora o reenvio enquanto a espera existir;
- `Human`/`Recovery` volta como `Pending::Setup` com as mesmas entradas e texto — a captura refaz o preparo com reserva nova, nunca reenvia o prompt composto. A fila restaurada no boot e um `send` repetido pulam as entradas que já estão numa espera (sem duplicar);
- a **retomada de uma espera de assinatura** parada pelo teto continua sendo da mesma espera: a tentativa volta a `waiting` sem contar, o `retry_waiting` fica segurado enquanto a linha existir (a linha leva `waiting: true` e sobrevive a restart) e, ao liberar, a espera fica devida na hora e ela mesma retoma — nunca um segundo consumidor, nunca o reenvio do lote como pedido novo. Espera pausada continua pausada. Linha gravada pela versão anterior (sem `waiting`) é reconhecida pelo checkpoint vivo do bot (`turn.waiting.<bot>` em `waiting`/`retrying`/`paused` com as mesmas entradas e conversa da captura): a migração do boot marca `waiting: true` (`migratedWaiting`) e o restore aplica a mesma regra — volta como espera de assinatura, uma retomada só, sem captura residual;
- `Ask`/`Message`/`Group` cujo canal morreu viram aviso durável no chat do bot (Notice idempotente por trabalho), gravado **antes** de a linha sair; o pedido com registro fecha como não iniciado.

A política por origem é reaplicada a **cada** espera em toda reavaliação: desmarcar `applyToConversations` solta a fala do dono já parada (pelo caminho normal) e mantém o autônomo parado. Abertas saem **uma por ciclo, na ordem em que travaram**, e só com o ator livre (sem admissão, despacho, turno ou item autônomo na fila à frente; a
própria espera de assinatura retoma ao ser liberada, como acima); a próxima sai quando o trabalho anterior terminar — rotina, pedido e fala do dono chegam ao agente na ordem das linhas. Cada linha só é apagada depois do efeito durável dela (aviso de perda, pedido fechado como não iniciado, espera de assinatura regravada); erro conserva a espera.

Cancelamentos do dono valem para o que está parado: **Parar** na principal cancela a rotina parada (execução `cancelled`, vaga devolvida), fecha pedidos parados e descarta capturas das entradas canceladas; Parar numa conversa/mensagem descarta as capturas daquelas entradas; **recomeçar a principal** tira rotinas, pedidos e capturas da principal; cancelar um `ask` ou um turno de grupo tira a espera correspondente. A decisão do dono é durável antes do efeito:

- a **intenção de cancelar** é gravada **antes** do efeito (rotina cancelada, pedido fechado) numa tabela própria, só por INSERT (`donna_ceiling_cancels`, chave `(bot, trabalho)`) — não depende de reescrever a linha da espera, e sai junto com ela (gatilho no DELETE). Efeito falhou: a liberação **nunca** devolve esse trabalho e o efeito é retentado a cada ~1 s — sem o dono repetir Parar, com ou sem o teto aberto, inclusive depois de um restart (o restore lê a intenção). Se **nem** a intenção **nem** o efeito puderam ser gravados, Parar devolve erro explícito (`ceiling_cancel_not_durable`) — nada de cancelamento durável anunciado — e o item fica bloqueado para retomada em memória, com retentativa; um restart nesse estado volta ao que existia antes do Parar (o dono foi avisado do erro);
- na fronteira durável de admissão (`decide_on`), trabalho com intenção de cancelar (ou marca `cancelling`/`settled` da versão anterior) nunca passa: a linha não é apagada (teto aberto) nem sobrescrita (teto fechado) por uma captura nova — o re-drive é segurado e o ciclo conclui o cancelamento;
- efeito confirmado mas a linha não saiu: o item fica liquidado em memória e a intenção gravada continua; a linha sai no ciclo seguinte, mesmo com o teto ainda travado, e depois de restart a intenção refaz o efeito (idempotente) e apaga a linha;
- Parar **uma** mensagem de um lote parado reescreve a captura com as que sobram (texto delas, **mesma linha, mesmo `seq`**) — elas saem uma vez na liberação, na vez delas. Se a reescrita falhar, a linha fica no lugar (a ordem também), a captura não sai com a mensagem parada e a reescrita é retentada até valer; o restore refaz a conta pelas entradas ainda na fila.

A exclusão de Space considera esperas como trabalho vivo.

Parar pelo teto também solta o preparo do ticket descartado (`preparing_turn_token` daquele token; um preparo sucessor fica): Desfazer e a instalação do motor não veem o bot como ocupado.

**Migração:** esperas do formato anterior (`donna.ceiling.hold.<bot>` e `donna.ceiling.hold.bg.<bot>` no kv) viram linhas com `work`, na ordem do instante gravado, com o corpo inteiro como evidência (`migratedFrom`); a chave antiga só sai depois que a linha existe. O que não converte (ilegível, sem origem, marcas antigas `donna.ceiling.stop.*`) fica no kv, intocado.

A reavaliação é espaçada: uma sonda O(1) (rótulo da janela — mês ou dia — + contador de escritas do ledger mantido por gatilho em INSERT/UPDATE/DELETE de `donna_turn_usage` + última liberação + configuração) decide se a conferência completa precisa rodar; sonda idêntica não reagrega. A conferência completa usa o índice por mês (`donna_turn_usage_month`) e ainda roda ao menos a cada ~60 s — estado mudado fora da sonda só pode manter a espera, nunca abri-la. A avaliação não é compartilhada entre atores (cada um faz a sua, barata). Emissão de evento e reescrita da linha só acontecem em mudança efetiva — a mesma parada não gera tráfego.

## Liberações do dono

`donnaCeilingRelease {kind, alvo, newLimit?}` (só o dono; autoridade reconferida na Tx): recusada se o gate não estiver de fato bloqueado naquele alvo — nenhuma liberação é cheque em branco. "Continuar mesmo assim" sempre leva a um **novo limite escolhido pelo dono**; não há opção padrão nem liberação "até o fim do período".

- Trava por valor (`alvo` = `money:XXX` ou `tokens`, a métrica que atingiu) — o dono escolhe uma de três:
  - `plus25` / `plus50`: novo limite = limite vigente × 1,25 / × 1,5 (tokens em inteiro, arredondando para cima);
  - `setLimit` com `newLimit`: o valor livre do dono (dinheiro na moeda da métrica; tokens inteiro exato).
  - Todo novo limite tem que ser **finito**, **maior que o gasto conhecido** e **maior que o limite vigente**. Atalho que não passa do gasto é recusado (`ceiling_release_below_spend` — ex.: teto US$ 100, gasto US$ 130: +25% = US$ 125 recusado, +50% = US$ 150 aceito); abaixo do limite vigente é a segunda guarda (`ceiling_release_below_limit`); não finito com `ceiling_release_overflow`; `newLimit` ausente/inválido em `setLimit` ou presente em outro tipo é `invalid_ceiling_release`/`invalid_ceiling_release_limit`. Tipos antigos (`plus20`, `untilMonthEnd`) não existem mais (`invalid_ceiling_release`).
  - O novo limite vale na janela do período (mês ou dia); o gate volta a travar nele e pergunta de novo. `donnaCeiling` devolve por métrica `shortcuts.plus25/plus50` (`limitAfter`, `allowed`) — o app desliga o atalho que o Hub recusaria, sem refazer a regra.
- `checkFailure` (`alvo` = `blocked.signature` lido em `donnaCeiling` — a assinatura **completa** `fingerprint#geração` que o dono viu): dispensa **só** o estado `Unverifiable` atual (inclusive falha de leitura `readFailure:{erro}`), sem valor, vinculado à **geração** daquela assinatura. O alvo é conferido na mesma Tx IMMEDIATE contra a assinatura atual; divergência (pedido atrasado ou repetido, ex.: F#1 → saudável → F#3, ou só o `fingerprint` sem geração) é recusada com `ceiling_state_changed` — o app relê `donnaCeiling` e pergunta de novo. O estado observado muda em qualquer caminho (inclusive saída saudável vista só pelo fundo, desligar/religar ou nova configuração) → a geração anda → a liberação velha morre — mesmo que a mesma assinatura volte. Liberação de valor não dispensa `Unverified`. Liberar de novo um estado já liberado é recusado (`ceiling_not_blocked`).
- Liberação acorda o ator (`CeilingChanged`): a reavaliação abre o gate e devolve o trabalho à origem, ou mantém a espera.

Cada liberação grava em `donna_ceiling_releases` (`id`, `at`, `actor` — identidade do chamador, sem nome pessoal —, `kind`, `month` — o rótulo da janela (`YYYY-MM` ou `YYYY-MM-DD@<fuso>`), `metric`, `limit_before`, `limit_after`, `overage_known` — quanto o conhecido passou do limite vigente —, `scope`, `state` — a assinatura `fingerprint#geração` liberada pelo `checkFailure` ou, nas liberações de valor, o novo limite exato em texto, que é o que o gate usa). Gatilhos recusam `UPDATE`/`DELETE`: a tabela é append-only.

## Trabalho de fundo

Títulos, consolidação de memória e resumo diário passam pela **mesma** decisão fail-closed (`background_gate`): antes de consumir vaga, conta ou processo, e de novo imediatamente antes do `session/prompt`. Travado, a linha `bg:<job>` do bot (tipo `background`) registra job, motivo e assinatura, e nada é enviado; o evento sai só quando a linha mudou. As linhas de fundo são só evidência (o job tenta de novo no próprio ciclo): saem na primeira conferência em que o teto não trava (liberação, painel, permit ou outro job). Compactação permanece fora do teto.

Todo prompt de fundo **enviado** grava uma linha no ledger (`source = 'fundo'`), uma por envio, **antes** do `session/prompt` (tokens NULL) — cancelar o trabalho depois disso (timeout, heartbeat da memória, Hub caindo) não apaga o envio; a resposta só completa o uso na mesma linha: `bot_id` = o bot do trabalho, `chat_id` = a principal dele, `session_id` = `background:<job>` (sessão sintética, sem pai; no `donnaCostReport` aparece como conversa "Trabalho de fundo (<job>)", mesmo formato de resposta, e aceita consulta individual — `botId` + `sessionId=background:<job>` na principal, validada pela linha do ledger daquele bot), tokens de entrada/saída/cache do retorno do motor (`_meta.quota.token_count`); o motor de fundo não informa preço, então a linha é **estimada** quando o modelo tem linha na tabela e há tokens (origem `estimado`, como nos turnos) — e esse valor entra na conta do teto de dinheiro. Job de fundo sem modelo conhecido ou sem retorno de uso fica sem preço (origem `fundo`, `amount`/`currency` NULL). Consequência honesta: com teto de **dinheiro** configurado, qualquer job de fundo sem preço no período deixa o dinheiro `Unverified` (`semPreco`) até o dono decidir (`checkFailure`); envio sem retorno de uso (timeout, falha) grava tokens NULL e deixa os tokens `Unverified`. O histórico de fundo (`background_history`, últimas 50) continua igual.

## Contabilidade

Tokens são comparados em inteiros (`u64`/`u128` no aviso) — a decisão nunca passa por `f64`, precisa além de 2⁵³. Dinheiro segue `f64` por moeda, sem conversão.

## Eventos

Mudança de estado visível (gravação de config, liberação, espera nova/alterada/removida, aviso novo, espera de fundo apagada) emite `{"type":"ceiling"}` em `hub.events`; o app relê `donnaCeiling`. `donnaCeiling` devolve `config`, `state` (`month` — o rótulo da janela —, `period`, `window {start, end, timeZone}`, métricas, `blocked` — `null` ou `{kind, metrics, fingerprint, signature}`, com `signature` = `fingerprint#geração`, o alvo do `checkFailure` —, `warn`), `stopped` (todas as linhas de espera — `botId`, `work`, `kind` `turn`/`background` e o corpo —, com ou sem ator) e `releases` da janela vigente.

## Risco residual conhecido

Voz em tempo real fica fora da cerca e do ledger.

O ledger só conhece turnos que gravaram linha. Um turno **interrompido** que não deixou linha (kill no meio, crash antes do commit do `finish`) não é detectável: o gasto real pode ser maior que o registrado. O teto minimiza isso travando por valor com o conhecido como piso e tratando qualquer não verificação como bloqueio, mas não finge ver o que não foi gravado.

## Quem lê e grava

- Grava: `donnaCeilingSave` e `donnaCeilingRelease` — `require_owner` + `pedido::gravar`; o gate grava esperas em `donna_ceiling_holds`, geração/avisos no kv; o executor de fundo grava o uso em `donna_turn_usage`.
- Lê: `donnaCeiling` — `require_owner`.
- Não é ferramenta de bot nem da Ponte; `MIRROR_SCHEMA` não muda.
