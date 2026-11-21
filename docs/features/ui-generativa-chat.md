# Mexerica — UI generativa no chat

**Estado atual:** o Hub tem o percurso da pergunta de escolha ligado
numa **conversa adicional** do Pessoal (`spaces::Conversation`, slot `conversation:<id>`):
ferramenta `mostrar_componente` (só `choice` v1), publicação da entrada, resposta por RPC com CAS e
entrega à fila de Sessões. O app **monta** o `ChoiceCard` em qualquer linha da lista de mensagens
que receba uma entrada `genuiChoice` e envia a resposta pelo Hub (seção [No app](#no-app-preparado)).
Nas sessões extras abertas de bot agente do Pessoal (slot `session:<id>`) o Hub também cria o cartão
pelo produtor da parte B (abaixo). O app
não tem tela de conversa adicional do Pessoal. O app já consome o contrato congelado
`sessionChoiceAnswer` v1 para responder numa extra (seção [No app](#no-app-preparado)); o Hub
já tem a RPC e o anúncio `choice` (parte A, abaixo), e o app só a usa com esse anúncio. Na conversa
principal do bot não aparece (falta vínculo de sessão da principal). Gráficos continuam só
contrato, em entrega própria.

**Sessões, parte A (Hub):** a RPC `sessionChoiceAnswer` (contrato v1 congelado) responde perguntas de uma **extra** aberta
de bot agente do Pessoal, anunciada por `choice` em `sessions.sessionActions` (ver
[Sessões por bot](context-and-memory.md)). O storage ganhou um ramo estrito de sessão
(`storage::Domain::PersonalSession`: extra aberta, não principal, bot agente vivo do Pessoal, origem e
projeção na mesma sessão e com o `spaceContext` canônico da principal do mesmo bot no Pessoal),
separado do ramo de conversa de Space, que não muda. O domínio é escolhido pelo chamador, nunca pela presença
do carimbo.

**Sessões, parte B (produtor, Hub):** `host/src/fork/sessoes/contexto_ferramenta.rs`.

1. **Selo.** Ao montar as definições MCP de uma extra aberta de bot agente do Pessoal (inclusive o Berga), o
   Hub emite um selo opaco só para o builtin `chat` (`DONNABOT_SESSION_TOOL_SEAL`), guardado no KV por bot e
   extra (SHA256, processo, SessionRef, vínculo original, geração do perfil e, depois de session/new ou load,
   a ACP exata). Não é Authority nem grant; o prefixo `session:`, o env ou o `spaceContext` não provam nada.
2. **Anúncio.** A ponte pergunta ao Hub a cada `initialize` e `tools/list` pela ação interna
   `chatCall`/`__tools` (exceção fechada, só Ponte autenticada). Antes da ACP, vale a Preparation original
   do turno; depois, a execução ativa original; a mesma fase capturada é rechecada antes de responder. Só
   devolve `{"available":true}`: nunca executa, abre turno ou cria pergunta. Recusa, selo falso, Hub fora:
   sem descritor e sem a orientação de escolha. A conferência final usa a MESMA credencial da Ponte.
3. **Chamada.** `mostrar_componente` numa extra usa a `ExecutionRef` já capturada e uma variante própria do
   `ToolContext`/`ToolFence` (lane `bot` + extra, ACP como sessão), comparando explicitamente vínculo, token
   do turno, mensagem humana da extra, ACP, lane, geração e selo; segue outbox → `ContextTool` → `ShowChoice`
   → ator. Na Tx, `check_execution_on` e a cerca pessoal, `storage::create` com `Domain::PersonalSession` e
   `snapshot` (origem e projeção E1, inclusive replay), e só então commit e publicação. A credencial ORIGINAL da
   Ponte que fez a chamada acompanha o contexto até essa Tx: revogada no caminho (fechamento, troca de processo,
   desligamento do Hub), a escrita recusa sem pergunta nem evento, mesmo com vínculo, selo e turno vivos. A resposta
   (`choice_response`) não é fala humana: não abre outra escolha.
4. **Cancelamento.** Fechar a extra cancela na mesma Tx (como antes). A varredura do início cancela também
   perguntas de extra fechada, ausente ou de bot excluído, pela relação `donna_bot_sessions`/`bots`, sem o
   corte da principal e sem recriar sessão.

## Ligado no Hub

Ordem do caminho, toda no Hub:

1. **Envelope.** A ponte MCP do servidor `chat` (`host/src/mcp.rs`) só anuncia a
   ferramenta numa conversa capturada (slot `conversation:` e selo de ferramenta) e,
   em `tools/call`, põe `invocation` = nonce aleatório do processo da ponte + id
   JSON-RPC do motor no **topo** do corpo, fora de `arguments`. O modelo não controla
   nonce nem id. Sem `invocation` válida o Hub recusa (`genui_invocation_required`);
   não há fallback para hash do payload. Um único reenvio, com a mesma invocação, só
   em falha de transporte. Chave durável: SHA-256 de versão, binding da execução,
   token do turno, bot e invocação; outra execução nunca reaproveita o recibo.
2. **Guarda.** `chatCall` → `sessoes::bindings::capture` → `ToolContext` (selo,
   geração do perfil, turno humano) → `tool_servers::decide` precisa oferecer `chat`
   (só Pessoal; `foreign` nada) → ator (`Cmd::ContextTool`/`ShowChoice`). O ator recusa
   pedido de outro bot, Times, canal e rotina e, sob `memory_boundary` (a barreira de
   updateBot/exclusão/troca de turno), refaz `check_actor`.
3. **Transação.** `emit_lock` → IMMEDIATE → `check_execution_on` (inclui ToolFence) →
   `storage::create` (G1a: Lane, sessão aberta, Space, marcador antes da entrada) →
   commit → publicação marcador → entrada por `rev`. Replay devolve o mesmo recibo e
   republica as mesmas entradas, sem reinserir.
4. **Fio.** A entrada `notice` leva `data.genuiChoice`, server-owned (o `update_entry`
   genérico não cria, troca nem remove), atualizada na mesma transação de cada estado:

   ```json
   {"version":1,"questionId":"…","revision":1,"state":"pending","snapshot":"<JSON choice v1 canônico>"}
   ```

   `answered` acrescenta `answer` (`Answer`) e `answeredAt`; `cancelled`/`expired`
   acrescentam `at`. `snapshot` é o mesmo texto de `choiceSnapshot` do app (ordem fixa de
   chaves). A audiência é a da entrada (filtro de Space existente). `MIRROR_SCHEMA` 18.
5. **Resposta.** `donnaConversationChoiceAnswer` (ver [Spaces](../reference/spaces.md)):
   mesma audiência de `donnaConversationSend`; CAS `pending`+`revision`; a resposta
   vira entrada `user` com `choice_response`, `status: queued`, selo da conversa e turno
   novo alocado na mesma transação. Replay idêntico devolve a mesma entrada.
6. **Retomada.** Depois do commit, `Cmd::Send` normal: fila do ator e admissão de
   Sessões. Após reinício, a entrada `queued` é retomada pelo caminho existente.
   Sem fila, timer, ledger ou checkpoint próprios.
7. **Encerramento.** Fechar ou limpar a conversa cancela as pendentes **na mesma
   transação** do fechamento/corte de contexto. Defesa em profundidade: resposta a pergunta
   anterior a `context_start_seq` é recusada (`genui_context_reset`), e a varredura do
   início cancela as pendentes de conversa fechada, de antes do corte ou sem entrada.
   Sessão encerrada já recusa no storage. Exclusão do bot apaga os registros. Sem prazo.

Limitações do v1:

- Só conversa adicional (Spaces/Pessoal); a principal do bot espera o vínculo de sessão.
- No turno iniciado pela resposta, as ferramentas contextuais do Hub ficam recusadas
  (`human_turn_required`), porque `choice_response` não é fala autoral. O bot responde
  pela mensagem final. A regra de memória não muda (origem `other`).
- Sem notificação push nem status "Precisa de você" para o cartão.
- Voz: nenhuma ferramenta nova no modelo de voz.

## No app (preparado)

O que o app faz nesta etapa, sem navegação nova e sem tocar no compositor:

- **Montagem.** `ChatRows` (herdado) tem um gancho curto que chama `genuiChoiceRow`
  (`views/fork/genui/GenuiChoiceRow.tsx`) pela projeção `data.genuiChoice` válida, **sem exigir
  `data.card`**: o Hub grava só `{text, genuiChoice}` (`host/src/fork/genui/storage.rs`), então
  exigir o `card` deixava toda pergunta real como texto literal (ponto D-1(b) do contrato
  `sessionChoiceAnswer` v1). Regras de segurança do app: só entrada **`notice`** (lista permitida;
  **nunca `user`**, que é o que o dono ou o `send` gravam, nem `agent`/`permission`/outras); `card`
  ausente ou o legado `"genuiChoice"`, nunca outro cartão; `choice_response` sozinho nunca monta
  cartão. Sem bump de `MIRROR_SCHEMA` (nenhum campo novo). O módulo lê `data.genuiChoice` com
  `choice-entry.ts` (objeto fechado: `version` 1, `questionId`,
  `revision` ≥ 1, `state`, `snapshot`, e `answer`/`answeredAt` ou `at` conforme o estado). Forma
  desconhecida, versão nova ou estado fora do contrato devolvem null e a linha mostra o texto
  literal da entrada (`data.text`), o mesmo do app antigo. O `snapshot` também precisa passar no
  `parseChoice` (JSON fechado, `choice` v1) já nessa leitura: JSON inválido, outra versão ou chave
  extra devolvem null e mantêm o texto literal, em vez de um cartão "Pergunta indisponível".
- **Estados.** `pending`, `answered`, `cancelled` e `expired` vêm só de `genuiChoice`. Enviar marca
  "Enviando…"; o cartão **não** decide Respondida: mesmo com a RPC aceita, continua enviando até a
  entrada atualizada chegar no fio. Seleção não é aprovação: nenhum texto ou ícone de aprovado.
- **Envio numa extra (sessão UI01).** Quando a entrada traz `sessionId` diferente do `botId` (os
  dois **brutos** da entrada, sem bot lógico nem conversa derivada), a resposta vai pela
  `sessionChoiceAnswer` (contrato v1 congelado). O store captura a intenção `choice` do catálogo de sessões
  (`store/fork-sessions.ts`, link e época atuais) **antes do primeiro await**; sem ela, nada sai
  (`unsupported`, ou `closed` se a extra já está encerrada). Relê `sessions {botId}` no mesmo link,
  exige `choice` no anúncio relido, a extra aberta e legível e usa a `revision` dela como
  `expectedRevision`; confere link/época em volta da releitura, antes da RPC e antes de aceitar o
  resultado (troca = `link`, sem adoção). Envia só `{botId, sessionId, expectedRevision, questionId,
  revision, answer}`, sem `hubId`/`spaceId`/`conversationId`. O resultado
  `{session, question, entry, replayed}` só é aceito se `entry` e `session` são desta extra e deste
  bot e `question` é a mesma pergunta; senão `scope`. `choice` não depende de `send`. Sem cache,
  geração ou fila paralelos; `replayed: true` é o mesmo recibo.
- **Envio na conversa (Space).** `store/fork-genui-choice.ts` (um modelo por store do computador) lê
  `donnaConversations {hubId, spaceId}` para achar a `SpaceConversation` da entrada e usa a
  `revision` dela como `expectedRevision`; então chama `ConversationClient.answerChoice`
  (`donnaConversationChoiceAnswer {hubId, spaceId, conversationId, expectedRevision, questionId,
  revision, answer}`), com `revision` = `genuiChoice.revision`. O destino é o da própria entrada:
  `spaceId` (ou o Pessoal, no histórico legado sem o campo) e `conversationId` (ou `botId`, que no
  fio é o id da conversa). Conversa principal (`id === botId`), ausente ou fechada **não recebe
  RPC**. Um envio por pergunta por vez; nada é durável nem reenviado sozinho.
- **Erros.** O store guarda só um código próprio (`revision`, `final`, `closed`, `invalid`,
  `scope`, `unsupported`, `forbidden`, `link`, `generic`) derivado do código estável do Hub ou do
  status do transporte; a view mostra a frase própria. Nada da mensagem do Hub ou da resposta é
  ecoado. A escolha volta ao cartão para conferir e enviar de novo; repetir a mesma resposta não a
  registra duas vezes (replay do Hub). Códigos da `sessionChoiceAnswer` (seção 3 do contrato congelado):
  `invalid_field`/`genui_question_required`/`genui_invalid_text` → `invalid`;
  `session_revision_conflict`/`genui_cas_conflict` → `revision`; `genui_deadline_elapsed` → `final`;
  `session_closed` → `closed`; `unknown_session`, `unknown bot`, `genui_*_mismatch` → `scope`;
  `session_chat_unsupported` → `unsupported`; `not_allowed` → `forbidden`;
  `engine_unavailable`/`session_selection_unavailable` → `generic` (sem texto novo). Só o prefixo
  estável do código é lido.
- **Acessibilidade.** O do `ChoiceCard`: `radiogroup` com setas, foco visível, rótulos e região
  educada que anuncia só a mudança de estado.
- **Textos novos** (pt-BR, chaves reservadas `genui.choice.error.*`, à espera da base de Idiomas);
  não há catálogo temporário.

Prova visual com dados fictícios no preview do componente (`apps/desktop/tools/genui-check`,
cenas `conversa`, `conversa-minima` e `sessao` — extra com entrada sem `card`, pendente, respondida e
erro —, com a linha real (`ChatRow` → gancho → store → link falso). Na demo do app (`?escolha`), a extra "Invalid-date guard"
do Byte traz uma pergunta no formato do Hub e o `fake-host` simula a `sessionChoiceAnswer` (seis campos, CAS da
sessão e da pergunta, replay) com o anúncio `choice`, pelo modelo real de sessões.

### Decisão: o cartão nasce nas Sessões

O dono verá só **Sessões**, com o cartão no próprio chat e o mesmo
compositor; não haverá navegação "Conversas à parte". Divisão:

- **Hub (frente Sessões):** aceitar o cartão em slot `session:<id>` (ponte MCP
  e selo por sessão), `ToolContext` de sessão com o vínculo correspondente, RPC de resposta por sessão,
  cancelamento na mesma transação de arquivar/limpar a sessão e os testes de fronteira. A forma
  `sessionChoiceAnswer {botId, sessionId, expectedRevision (da SessionRow), questionId, revision,
  answer}` foi congelada como v1 e está implementada no Hub
  (parte A, acima). O armazenamento ganhou a variante de sessão igualmente estrita (`Domain::PersonalSession`),
  sem mudar o JOIN de `storage::context` da conversa de Space.
- **App:** consome o contrato congelado (seção [No app](#no-app-preparado), "Envio numa
  extra"). O gancho no `ChatRow` já serve à lista das sessões. O produtor de sessão do Hub é a parte B
  (acima).

`conversationId` nunca é `sessionId`: nenhuma identidade é inferida uma da outra.

## Base do contrato e storage

### Contrato

O descritor de escolha está em `host/src/fork/genui/contract.rs::descriptor`; é
anunciado pela ponte do servidor `chat` só na conversa capturada. Catálogo atual: **somente `choice`**, versão inteira 1.
Objeto fechado em todos os níveis:

```json
{
  "version": 1,
  "component": "choice",
  "title": "Escolha",
  "summary": "Dados informados pelo bot",
  "data": {
    "question": "Qual opção?",
    "options": [{"id": "a", "label": "Opção A"}]
  }
}
```

Até 64 KiB UTF-8 por JSON; de uma a oito opções; título/rótulo até 160 caracteres;
pergunta/resumo até 2.000; resposta livre até 4.000. IDs únicos, opacos, de um a
160 bytes ASCII, com letras, números, `_`, `-` e `.`. Texto vazio ou controle fora
de nova linha/tab é recusado, sem truncar. A resposta tipada tem `kind` igual a
`option` (campo `id`), `none` (sem outros campos) ou `text` (campo `text`).
O JSON do pedido e todos os objetos da resposta recusam campos extras.

Não há configuração HTML/JS/CSS/SVG, callbacks, comando, IPC, recurso externo,
URL de componente, consulta ou navegação. Strings podem citar markup e ações;
continuam dados. Todo conteúdo não confiável, inclusive resposta, fica num bloco
literal Markdown com delimitador maior que qualquer sequência de crases presente.
Isso impede que uma imagem/link ou fechamento de bloco citado vire markup ativo
no fallback. Parse inválido devolve código estável, sem ecoar o corpo no erro.

### Persistência e limites de autoridade

`fork::migrate` cria idempotentemente `donna_genui_questions`. O snapshot JSON é
único e imutável; a entrada canônica `notice` guarda apenas texto derivado. O
snapshot textual original fica preservado em `snapshot_text`; consulta interna
retorna o texto atual da entrada, atualizado junto do estado. Payload/opções não
são reescritos após resposta. A projeção `genuiChoice` acima vai no fio.

As APIs recebem a transação do chamador e usam somente sua conexão, sem lock do
Store/Cloud nem transação aninhada. **Todo erro exige rollback da transação pelo
chamador.** Origem, Lane, sessão aberta e Space são conferidos contra entradas,
catálogo de sessões e conversa/bot vivos. A principal mantém `entries.session`
nula. O contexto interno não é desserializado do modelo e não prova autoridade.
A guarda da origem/geração de execução é a do fluxo (`flow`), antes destas APIs.

Identidade interna de invocação tem unicidade durável, separada do payload.
Replay com mesmo vínculo, JSON tipado e prazo devolve o recibo da mesma pergunta
com seu estado atual; conteúdo/prazo diferente conflita. Uma invocação nova com
JSON igual cria pergunta distinta. Ordem de chaves/espaços não alteram o hash
SHA-256 do snapshot tipado. A ponte produz a identidade (envelope acima).

Perguntas têm estados `pending`, `answered`, `cancelled`, `expired` e revisão.
CAS confere revisão e estado pendente; resposta duplicada/antiga ou após estado
final falha sem efeito quando a transação é descartada. Não existe operação de
reabertura ou revisão. Prazo é opcional, interno do Hub, sem default; o relógio
do Hub decide validade. Sem produtor de expiração ativo, apenas passar o prazo
não grava estado novo; resposta tardia já é recusada. Sessão fechada recusa as
operações; o encerramento cancela as pendentes no fechamento da conversa/sessão.

A resposta entra como `user`, sem falsificar `from`/`author`, com objeto
server-owned `choice_response`. `update_entry` não pode criar, substituir ou
remover essa marca. `Entry.is_user_authored` a recusa antes do legado; gestão,
origem do turno de memória, write_gate, dream e evidência do histórico não a
promovem a autorização/preferência/sucesso. O histórico preserva texto e papel
conversacional, sem evidência autoral positiva. Uma mensagem autoral independente
posterior continua sujeita aos seus próprios gates.

Inserção usa o helper canônico compartilhado, com carimbo de Space e marcador
antes da entrada na mesma transação. Recibo conserva IDs duráveis de ambos os
marcadores (pergunta e resposta). São publicados em ordem após o commit, sem
reinserir. Na resposta, o Hub aloca o turno novo na mesma transação e entrega a
entrada à fila canônica de Sessões, sem protocolo de execução próprio.

A ordem de escrita funciona com FKs ligadas: resposta/marcador são inseridos antes
do CAS que referencia a resposta. A produção não depende de cascatas SQLite;
exclusão de bot chama gancho que limpa somente os nossos registros. Entradas
órfãs ou vínculo/Space alterado falham na leitura, sem fallback para principal.

### Validação

Testes Rust escritos cobrem catálogo/JSON fechado, limites/duplicados, erro sem
eco de sentinela, fallback com imagem/link/script e crases maliciosas, replay e
conflito, invocação nova igual, vínculos cruzados, marcador recuperável, CAS,
estados finais, prazo do Hub, rollback, banco reaberto e procedência preservada.
Há regressões nos consumidores existentes de gestão, write_gate, dream e histórico.
São testes de armazenamento/procedência, sem afirmar autoridade ponta a ponta.

Isso não ativa o percurso público. Cada etapa posterior tem suas próprias conferências antes de declarar a
entrega; os testes escritos desta seção não substituem esses resultados. Montagem do cartão
no app e gráficos seguem em entregas posteriores.

## Gráficos: contrato preparatório

Esta etapa acrescenta somente `host/src/fork/genui/charts.rs`: catálogo interno,
validador puro e snapshot em memória com fallback. Não ativa gráficos automáticos,
ferramenta, `tools/list`, RPC, storage, migração, fila, audiência ou campo no fio.
Escolha e seu descritor permanecem iguais. O descritor gráfico é separado e interno;
não há consumidor produtivo. A integração pública exige entrega própria, produtores
de Bot por dentro/Sessões e validação de autoridade/audiência no Hub.

Contrato concreto v1, fechado em todos os níveis, até 64 KiB UTF-8 de entrada:

```json
{
  "version": 1,
  "component": "line_chart",
  "title": "Quantidade por dia",
  "summary": "Exemplo de dados informados, com uma lacuna.",
  "data": {
    "xKind": "date",
    "labels": ["2026-11-01", "2026-11-02", "2026-11-03"],
    "series": [{"id": "volume", "label": "Quantidade", "values": [2, null, 0]}],
    "unit": "itens",
    "period": "1 a 3 de novembro de 2026",
    "sourceLabel": "Contagem informada na conversa"
  }
}
```

- `component`: somente `line_chart`, `bar_chart` ou `pie_chart`; versão inteira 1.
  `xKind` é obrigatório somente em linha, com `category` ou `date`; nas demais
  peças é recusado, inclusive `null`. Não há configuração livre de gráfico.
- `title`, cada label, nome de série, `unit`, `period` e `sourceLabel`: texto
  obrigatório até 160 caracteres; `summary`: texto obrigatório até 2.000.
  Regras do contrato: sem vazio ou controle, exceto nova linha/tab; sem truncamento.
  Markup, URL e instruções citadas em texto são inertes, nunca recursos carregados.
- `labels`: de 1 a 500; `series`: de 1 a 3, com exatamente um valor por label.
  IDs únicos de série seguem o contrato: 1 a 160 bytes ASCII alfanuméricos, `_`, `-`, `.`.
  Há uma única `unit` comum a todas as séries; unidade por série/campo extra é
  recusada. Não se convertem nem inferem unidades, período, fuso ou consulta.
- Valores são números JSON finitos ou `null`. Escala de engenharia: magnitude
  de cada valor não pode superar `9.007.199.254.740.991` (2^53 − 1).
  Linha/barra não têm limite de soma global; total só é validado para pizza.
  Isso não prova exatidão decimal nem verdade dos números. Não há
  arredondamento, preenchimento ou alteração do valor aceito. `null` permanece
  **Indisponível**, separado de zero; futura linha não liga trechos pela lacuna.
- Linha com `xKind: date`: cada label é data ISO `YYYY-MM-DD` real, ou timestamp
  RFC3339 com `T` e fuso explícito `Z` ou `±HH:MM`. Offset `-00:00` (fuso
  desconhecido) é recusado. Não se deduz fuso nem se reescrevem os labels.
- Pizza: exatamente uma série, sem `null`, valores não negativos e total positivo
  finito até 2^53 − 1. Lacuna recusa o pedido, sem proporções falsas.
  Até cinco labels conserva `pie_chart`; acima de cinco, até 500, torna-se
  `bar_chart` com **todos** os mesmos labels/valores, título, resumo, unidade,
  período e fonte, sem “Outros” automático. Mesmo nesse caso,
  valem as regras de completude e não negatividade do pedido de pizza.

O objeto validado tem campos privados, sem mutação nem desserialização pública.
`snapshot()` serializa deterministicamente `{requested, effective, payload}`;
`payload` preserva a peça solicitada e os dados tipados aceitos, e `effective`
registra só a normalização decidida. É uma fotografia em memória, sem nova tabela
ou alegação de persistência durável. Ordem das chaves e espaços de entrada não
mudam o snapshot; números seguem a representação numérica tipada de JSON/f64.

`fallback()` inclui título, resumo, peça solicitada/efetiva, unidade, período,
`sourceLabel`, IDs/nomes de séries e **cada** label e valor aceito, incluindo zero
real e “Indisponível”. Tudo fica no bloco literal do contrato com delimitador calculado,
sem ativar Markdown. A marca fixa é **Informado pelo bot**: o validador não prova
fonte, precisão, cobertura, audiência ou autorização. `sourceRefs`, `verified`,
HTML/JS/CSS/SVG/configuração, URL de componente, comando, IPC e callback não são
campos aceitos; não há resolvedor paralelo, busca de fonte/URL ou carga de imagem.
Erros têm apenas códigos estáveis `genui_chart_*`, sem corpo, label ou valor.
JSON inválido não produz snapshot nem fallback do payload recusado.

Gráficos continuam sendo apresentação sobre dados já disponíveis: resumo e **Ver dados**
na integração futura, com a mesma tabela completa, sem consulta nova, exportação
ou outro efeito. Esta base não entrega renderer nem botão. Referência de estado e
gates: próxima etapa.


### Precisão numérica e alcance compartilhado

O parse usa `serde_json` com `float_roundtrip`, feature da dependência existente,
para preservar f64 → JSON → f64, inclusive no limite 2^53 − 1. Inteiros continuam
inteiros; frações seguem f64, sem promessa de precisão decimal arbitrária. Esta
feature afeta **parse de floats no Hub inteiro**, não só gráficos; consumidores
compartilhados de memória, rotas e cache precisam ser conferidos na revisão.
Não há crate nova, mudança de versão ou de lock por essa ativação.

Para pizza, o total é uma expansão exata dos valores binários aceitos. TwoSum
conserva cada resíduo em vez de arredondar a soma final: depois de subtrair o
limite na própria expansão, o sinal do termo não zero de maior magnitude decide.
MAX + 0,1 é recusado em ambas as ordens; resíduos positivos não desaparecem e
resíduos negativos legítimos não causam recusa de totais válidos. Só a validação
usa essa expansão; snapshot, fallback e normalização preservam os dados originais.

## Cartão de pergunta no desktop (preparatório)

O desktop tem o componente real `ChoiceCard` em
`apps/desktop/src/renderer/views/fork/genui/`, com modelo puro, testes de fronteira e preview.
Ele é montado na lista de mensagens a partir de `entry.data.genuiChoice` (seção
[No app](#no-app-preparado)); ainda não aparece para o dono pelos motivos de lá.

O cartão lê o `choice` v1 deste contrato e o valida de novo, defensivamente, com os mesmos
limites e o objeto fechado. Esse parser não substitui o validador do Hub nem dá confiança ao
payload. No JSON bruto, recusa também chaves duplicadas em qualquer nível, inclusive nomes
equivalentes escritos com escapes Unicode. O helper puro `json-fechado.ts` usa JSON.parse para
sintaxe e uma varredura iterativa para duplicatas. A opção explícita
`unsignedIntegerFields: ['version']` exige no bruto um literal inteiro sem sinal: `1` é aceito;
`1.0`, `1e0` e `-0` são recusados, também com chave escapada. O helper não restringe outros
números nem decide faixa/versão/schema, que continuam no consumidor. O limite bruto de 64 KiB
continua anterior ao parse. Valores já decodificados continuam copiados sem executar getters
ou `toJSON`, mas só permitem checagem semântica: após JSON.parse, `1.0` e `1e0` já são o mesmo
Number 1 e sua forma lexical não pode ser recuperada.
Também lê o estado registrado pelo Hub (`pending`, `answered`, `cancelled`, `expired`,
revisão e horários) e, por props, o envio transitório do consumidor (`sending`, `unconfirmed`,
`error`). Esses campos de visualização são privados do componente, não um fio aprovado. A
seleção local fica separada do envio: só `answered` com resposta válida do Hub marca Respondida.
O envio transitório deve conservar a identidade completa da intenção: `questionId`, `revision`
e `snapshot` canônico do payload validado. Envio de outro dono ou sem qualquer campo desse vínculo
é ignorado antes de projetar estado ou restaurar rascunho: não bloqueia nem preenche a pergunta
atual. Mesmo com identidade completa, a projeção aceita em execução somente `sending`,
`unconfirmed` e `error`; status ausente, não textual ou qualquer outro valor (inclusive
`answered`, `cancelled` e `expired`) fica pendente, sem resposta mostrada/restaurada. O enum
transitório nunca encerra a pergunta; os finais continuam vindo apenas de `hub.state`.
A identidade é a mesma do rascunho local, sem parser/protocolo paralelo ou prova de Hub.
Ao tocar em Enviar resposta, o cartão entrega a quem o usa `{ questionId, revision, snapshot, answer }`,
com `answer` igual a `Answer` (`option`, `none` ou `text`). Não espera, não reenvia e não guarda
nada durável. Rascunho local ausente e rascunho deliberadamente vazio são distintos: após erro,
a resposta pode voltar uma vez quando não há rascunho; apagar tudo mantém o campo vazio e
Enviar inativo, permitindo escrever de novo. Trocar pergunta, revisão ou snapshot descarta
efetivamente o rascunho antes de qualquer intenção; voltar ao proprietário anterior não o
recupera. A futura integração passa pelo CAS deste Hub e por `choice_response`. Escolher continua
sendo resposta de conversa, nunca aprovação. Desenho, estados, teclado, acessibilidade e PNGs
do preview estão registrados junto do componente.
