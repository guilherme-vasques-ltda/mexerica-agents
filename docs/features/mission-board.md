# Mission Board — projeção do Hub e primeira tela

O nome é **Mission Board** (antes “Torre de Controle”). Os nomes internos
(`control_tower`, `donnaControlTower`, `views/fork/torre/`) continuam como estão.

A projeção do Hub disponibiliza a API de leitura da Mexerica e a primeira
tela que a consome (seção [Tela no app](#tela-no-app)). Nenhum dos dois
entrega biblioteca, pausa ou novo Parar. O Hub mantém a única fonte de estado;
a projeção não tem tabela, fila, cache persistente ou ledger próprios. O worker
por função não é um bot do roster e não entra nesta projeção.

## Rotinas criadas pelos bots

A seção aparece mesmo quando o bot está livre. Toda criação pelo MCP gera aviso
na conversa principal e cartão com rotina, bot, data e programação. **Manter**
encerra a revisão; **Cancelar** usa a exclusão existente, sem desfazer efeitos de
trabalho já admitido. Silêncio mantém a rotina ativa. Rotina criada a partir de
conteúdo de fora nasce `paused`: o cartão mostra a rotina pausada, o
silêncio a mantém pausada e o cartão oferece **Ligar**. O Hub valida a decisão,
liga e reagenda na mesma transação que encerra o cartão, sem executar agora.
**Manter** nos outros cartões continua sendo ciência. Fotografia antiga ou sem
conexão conserva conteúdo e desabilita ações. Hub antigo ou projeção inválida
mostra “Não informado”.

A observação aditiva `routineCreations` contém `items` e `blocked`, sempre do
Space autorizado. `donnaRoutineCreationReview` exige autoridade atual do dono e
confere Hub, Space, identidade da entrada e revisão. A decisão e o estado de
Rotinas são persistidos juntos; revisão repetida não repete o efeito.

Cota esgotada mostra motivo e orientação. O padrão é 10 criações por bot em
24 horas móveis, contando toda criação MCP, inclusive turno humano. Criação
direta do dono no app é isenta. Como a tela do limite ainda não existe nesta
base, só há campo/API no Host, sem navegação fictícia no Board.
Ver [Rotinas](routines.md).

## Consulta e autoridade

`donnaControlTower {hubId, spaceId}` retorna o DTO `TowerSnapshot` de
`host/src/fork/control_tower/model.rs`. Os dois seletores são obrigatórios;
campos extras são recusados. O Hub deve ser o da conexão autenticada. Local e
Device com Control atual podem ler; pareamento, Device sem Control, revogado ou
com lease expirado não podem. O retrato original de autoridade do transporte é preservado no stream, sem renovar
a concessão em nome do pedido antigo. Revogar e autorizar novamente a mesma
chave/escopos não recupera consulta pendente nem hint enfileirado. A concessão é revalidada antes da guarda Hub/Space (sem oráculo de existência
para Device revogado), após a espera pela trava administrativa e antes da resposta. Não há ferramenta MCP para o bot nem nova
concessão por selecionar Space.

O SQL filtra o roster antes de construir os cartões: bots agentes vivos do Space,
e a principal somente se já tiver conversa naquele Space, como no produtor atual
de conversas. Canais não viram executores. Nome/avatar reutilizam a apresentação
atual; a função `title` só é enviada no Space próprio do bot. O avatar vem da
coluna canônica `bots.avatar_config`, recomposto com o perfil por
`fork_avatars::restored`, o mesmo decodificador do roster atual, na mesma
Connection/transação autorizada. Nunca enviar Alma, pasta, conta, modelo ou
configuração pessoal por este DTO.

## Contrato do snapshot

- `version: 1`, `hubId`, `spaceId`, `collectedAt` em milissegundos Unix e
  `snapshotId` UUID desta coleta. O UUID identifica a fotografia efêmera, não é
  revisão de execução/CAS. Não expor um contador global de mudanças privadas.
- `bots[]`: `botId`, `name`, `title` opcional e `avatarColor`, `avatarShape`,
  `avatarConfig` existentes; `runtime`, `open`, `pending`, `queue`, `usage`.
- `interBotRequests` (aditivo, `version` continua 1): observação dos pedidos entre
  bots; ver [Pedidos entre bots](#pedidos-entre-bots).
- `delegatedPermissions` (aditivo, `version` continua 1): checkpoints de
  permissão de pedidos entre bots ainda em execução. A fotografia usa apenas os
  cartões pendentes da conversa principal aberta no Space autorizado, com os dois
  bots no roster. Cada item contém somente chave opaca, IDs dos bots e do cartão,
  referência e revisão da conversa, modo e data do pedido; até 20 itens e `omitted`.
  Indisponibilidade da fonte é `unavailable`. A referência permite abrir a conversa
  do destinatário; não promete rolagem ao cartão. Não concede, responde ou retoma.
- `routineCreations` (aditivo): avisos de criação e diagnósticos de cota; ver seção acima.
- `whatHappened` (aditivo): a coluna "O que aconteceu"; ver
  [O que aconteceu](#o-que-aconteceu).
- Uma observação é `{availability:"available", source, coverage, value}` ou
  `{availability:"unavailable"}`. Indisponível não é zero, lista vazia, Livre,
  sucesso anterior ou ausência de trabalho. O consumidor deve mostrar “Não
  informado”/“Fila não disponível”, conforme a medida.
- `runtime` vem de `hub.runtime_audiencia`, cobertura
  `currentAuthorizedConversationOrObservedIdle`: `status` real
  `idle|working|needsInput|waiting|error`; atividade/início apenas enquanto
  `working`, quando medidos; `resumeAt` apenas enquanto `waiting`, quando medido.
  Espera/erro/entrada usam motivos genéricos seguros, sem erro cru do motor.
  `conversation` contém somente `conversationId` real e revisão atual, se a
  conversa capturada ainda estiver aberta e autorizada. Não é referência de
  execução nem SessionRef.
- Runtime inexistente nunca usa o default Idle. Uma lane fora do Space, conversa
  fechada/desconhecida ou erro sem lane têm a mesma resposta indisponível: não
  revelar se há atividade em outro contexto. Idle sem lane só é apresentado
  quando existe um valor observado limpo do runtime e a conversa própria aberta
  foi comprovada pelo produtor canônico. Atividade/relógio sem lane não provam
  repouso. Lane de canal exige membro atual; nome ou Space igual não bastam.
- `queue` está indisponível: não existe medição geral com cobertura comprovada
  nos produtores consumidos. Não somar agendas, asks ou mensagens do renderer.
- `usage.dailyBotTokens`, `usage.subscriptionLimit` e `usage.contextTokens` são
  três observações distintas, todas indisponíveis nesta versão. Não copiar o
  limite compartilhado da assinatura/contexto como uso ou teto diário do bot.
- `pending` tem fonte `entries.permission`, cobertura
  `openCatalogSessionsPermissionCardsInBotConversations`. Só referências únicas
  a cartões `permission/pending` originais das conversas abertas do bot no Space,
  com sessão aberta conferida no catálogo `donna_bot_sessions`: `entryId`,
  `sessionRef:{id,chatId}` do tipo canônico de sessões, `kind`, `status` e
  `conversation` (ID/revisão). Sem corpo, comando,
  argumentos ou nova aprovação. Não cobre todos os tipos de pendência, canais,
  asks ou rotinas. A lista vazia vale apenas para essa cobertura autorizada. Sem conversa aberta
  e sessão aberta capturadas no catálogo, a observação fica indisponível.

A audiência é aplicada também a IDs, existência e contagens. O runtime da
principal atuando em outro Space não envia estado, atividade, início, título da
conversa ou número de pendências privadas ao cartão do Space consultado. A
lista de pendências não depende da lane atual: lê somente os cartões originais
já autorizados naquele Space. Sessões ACP nunca saem no DTO. A `sessionRef` da
pendência vem de `sessoes::store::session_ref_of_entry`, no catálogo original
ligado à entrada, nunca da Lane ou de ACP.

## Perguntas ao dono

O bot pergunta por ação explícita: a ferramenta `ask_owner {question}` (servidor `chat`, até 500
caracteres). Não há inferência pelo texto (“terminou com ?”) nem tabela nova.

**Limite desta etapa:** só no Pessoal. `ask_owner` existe apenas onde
`tool_servers::decide` oferece o servidor `chat` (bots do Pessoal); bot nativo de outro Space e a principal
com identidade estrangeira não recebem a ferramenta nem a orientação, que vai só nas instruções desse
servidor. O servidor `chat` não foi liberado em outros Spaces.

- **Cartão.** Entrada `notice` na conversa/sessão da execução de origem, gravada na mesma transação IMMEDIATE
  que confere a `ExecutionRef` viva: `data.card = "ownerQuestion"`, `data.text` (a pergunta),
  `data.author` e `data.ownerQuestion = {status, chatId, sessionId, askedAt, answeredBy, closedAt}`, com
  `status` `pending|answered|withdrawn`. Recusada sem execução, em pedido entre bots/equipes (`ask_bot`,
  `message_bot`) e em rotina. Código: `host/src/fork/owner_question.rs`.
- **Respondida.** Na MESMA transação em que a próxima mensagem humana daquela conversa/sessão é gravada
  (`send` da conversa ou canal e envio da conversa de Space), com `answeredBy` = id dessa mensagem. Num
  canal, uma mensagem responde todas as perguntas pendentes ali, de todos os bots.
- **Retirada.** Pergunta nova do mesmo bot na mesma conversa/sessão; reiniciar (`newSession` da principal)
  ou limpar a conversa (`reset` do controle de conversa); fechar a conversa de Space; excluir o bot ou o
  canal. Fechamento de sessão adicional ainda não existe no Hub; quando existir, retira também. A retirada
  é uma segunda transação logo após a operação; se o Hub cair entre as duas, a varredura idempotente do
  início (`owner_question::sweep`) retira as pendentes de bot/conversa excluídos, fechados ou limpos.
- **Runtime.** `pendingQuestions: [{entryId, chatId, sessionId?, text, askedAt}]`, no máximo 5, as mais
  novas, recarregado do transcript ao iniciar o ator (reinício do Hub não perde). Com ao menos uma e sem
  turno em andamento, o estado mostrado é `needsInput` (como a permissão); o estado interno segue o do turno,
  então verificações de “turno em andamento” não mudam. A notificação é a mesma da permissão pendente.
  Cada leitura leva a revisão do banco lida sob o mesmo lock; o runtime só aplica leitura mais nova que a já
  aplicada, então atualizações fora de ordem não ressuscitam pergunta respondida ou retirada.
- **Audiência.** No BotJSON (`bot`/`hello`) e no `TowerSnapshot`, cada pergunta só aparece se a regra
  canônica de `runtime_audiencia` autoriza a conversa dela no Space consultado; canal não herda perguntas
  do membro. As perguntas são revalidadas pela conversa de cada uma, independentemente da disponibilidade
  do runtime, com o mesmo resultado no snapshot e no evento: runtime indisponível (atividade em contexto
  sem audiência) não esconde as perguntas autorizadas nem revela a atividade. No snapshot, `questions` é
  `{availability:"available", source:"hub.runtime.pendingQuestions",
  coverage:"ownerQuestionsInAuthorizedConversations", value:[{entryId, conversation, sessionId?, text,
  askedAt}]}` só com conversa aberta capturada na coleta; sem runtime observado, indisponível.
- **App.** `Bot.pendingQuestions` (Hub antigo sem o campo → `[]`); o cartão no chat mostra o texto e o
  estado; a Voz diz “tem uma pergunta para você no chat”, não aprovação. `MIRROR_SCHEMA` = **16** (15 é do
  líder do canal).
- **Board.** Pergunta coloca o bot em “Esperam você”. Cada pergunta mostra o texto e, no Pessoal com
  conexão, um campo **Enviar resposta** que usa o MESMO `send` do composer, com o mesmo relatório de
  origem humana (`desktopComposer`), na conversa da pergunta; o estado só muda quando o Hub registra a
  mensagem. Fora do Pessoal ou sem conexão: só Abrir conversa. Hub sem `questions`: indisponível.

## Abrir: somente navegação

`open` é nulo ou `{label:"Abrir conversa do bot", botId, conversation, sessionRef}`.
A `sessionRef` opcional só aparece se a sessão principal aberta da conversa
foi encontrada no catálogo; não é a sessão ACP ou a execução atual. Aponta a
primeira conversa aberta já cadastrada do bot no Space, priorizando a principal
local conforme a ordem do produtor. Não promete abrir a execução do runtime.
Mesmo se o bot estiver em um canal, não trocar o destino pela conversa do canal
nem inferir que o canal é um bot executor. Não há RequestRef/ExecutionRef nem
Ver andamento nesta etapa.

O futuro consumidor seleciona o mesmo Hub/Space/bot/conversa e usa a leitura
existente autorizada: `history` na principal legada (ID da conversa igual ao bot)
ou `donnaConversationHistory` nas demais. Para abrir a pendência original,
seleciona também sua `sessionRef` do catálogo e seu `entryId`, usando o fluxo de
histórico existente aplicável. Revalidar no destino; conversa fechada ou revogada
não redireciona o conteúdo à principal. Abrir/ler não cria sessão, envia, aprova,
retoma, inicia motor ou assume trabalho. Não há RPC nova de Abrir ou Parar.

## Atualização no canal atual

O Hub publica `donnaControlTowerChanged` no broadcast existente quando mudam os
campos medidos do runtime, incluindo início. A comparação usa as projeções
canônicas no Space anterior/atual: duas observações indisponíveis não emitem
sinal privado, inclusive na primeira inserção do runtime. Antes de entregar no SSE/canal, o
Hub revalida Device/lease/Control e a relação atual bot–conversa–Space pela
mesma regra canônica de `runtime_audiencia`, incluindo associação atual ao
canal. Bot no roster e conversa aberta isoladamente não autorizam a entrega.
O evento externo contém **somente** `{type, hubId, spaceId}`. Não contém
bot, título, atividade, status, contagem, snapshot, sessão ou aprovação.

A invalidação alcança o contexto anterior para remover o trabalho que saiu de
sua audiência e o contexto atual autorizado. A conversa capturada não precisa
continuar sendo a lane atual: se o bot saiu dela, mas ainda tem associação
autorizada, o hint antigo permite atualizar o cartão. Remoção do membro antes
do consumo descarta esse hint, mesmo que bot e canal continuem no Space.
Mudanças privadas posteriores não
invalidam o contexto anterior. Mudança para erro sem contexto não expõe motivo
ou audiência privada. Evento de runtime atrasado após exclusão/fechamento é descartado. Troca da
principal e exclusão capturam antes da mutação os Spaces de origem e das
conversas atingidas, incluindo a principal solicitada, e emitem invalidação só
após sucesso. Essa invalidação não exige que o vínculo antigo ainda exista:
revalida Caller/lease/Control e o Space e entrega o mesmo corpo mínimo. Não
depende de botDeleted, que não tem emissor no Host. Fechamento também chega
pelo evento existente de conversa. Conversa apenas histórica de bot que já
não é a principal não autoriza sinalizar sua exclusão em outro Space. Na troca
da principal, os candidatos anterior e novo continuam sendo considerados para
retirar ou incluir seus cartões; a cerca negativa da exclusão não apaga essa
invalidação de seleção.

O consumidor futuro assina o canal do Space **antes** da consulta, relê em
`donnaControlTowerChanged` e em mudanças autorizadas de `bot`,
`conversation`, `entry`, `session`, `spaces`, `hello` e `resync`. Esses eventos
legados são sinais para reler, nunca o DTO ou fonte para calcular o cartão.
Agrupar invalidações concorrentes e reler novamente se uma chegar durante a
consulta. Não exigir inferência de fila, polling de IA ou novo canal.

Troca de Hub/conexão/Space ou perda de acesso descarta a projeção; resposta velha
de outra geração da conexão não pode repovoá-la. Reconexão/hello exige nova
consulta. Se ficar desconectado, uma fotografia em memória só pode ser exibida
como antiga, com horário e sem controles de efeito. Não persistir dados privados
ou misturá-los entre Hubs. A implementação da UI e desse ciclo está na seção [Tela no app](#tela-no-app).

A projeção não adicionou campo a bot/hello/entrada (as perguntas ao dono adicionaram `pendingQuestions`, ver acima). O DTO é
retornado somente pela RPC efêmera, e a invalidação não é estado espelhado; por isso `MIRROR_SCHEMA` não
muda por causa da Torre: segue o valor da main consumida (**14**). A futura UI não deve incluir o DTO no mirror persistente.

## Tela no app

Na faixa e no título aparece **Mission Board**.

`apps/desktop/src/renderer/views/fork/torre/` (modelo em `src/shared/fork-torre.ts`).

- **Entrada.** Botão próprio da faixa, acima de Rotinas. Abre no Hub e no space atuais da
  faixa: o space aberto, ou o Pessoal desse Hub. Um lugar atual por vez: abrir o Mission Board fecha
  Rotinas, Equipes, Ajustes e Extensões, e vice-versa. **Abrir Equipes** abre a tela Equipes no mesmo Hub/space.
- **Ciclo.** `TowerSession` assina o stream do space antes de `donnaControlTower {hubId, spaceId}`,
  relê nos sinais listados acima (agrupando avisos e relendo de novo se um chegar durante a
  leitura) e descarta resposta de outra origem (Hub, link, geração). Resposta com `hubId`/`spaceId`
  diferentes do pedido é recusada. Sem link ou com o stream parado, a última fotografia fica só em
  memória como **Última atualização às…**, com “Sem conexão” em cada cartão e o estado antigo
  esmaecido; nada vira Livre. Outro Hub no mesmo computador apaga a fotografia; 403 também.
  Nada vai para o mirror, `localStorage` ou disco; `MIRROR_SCHEMA` não muda (segue **14**, da main).
- **Só o que precisa de atenção.** O Board lista Esperam você (Precisa de você,
  permissão ou pergunta pendente), Trabalhando agora e Aguardando e com erro, além das seções de
  Equipes. Bot `idle` (Livre) e bot com runtime indisponível ou status desconhecido não aparecem:
  sem seção, cartão, painel ou contador. Sem conexão, a lista “como estavam às…” usa o mesmo
  filtro. Nada listado: “Nada precisa de você agora.”, sem painel e sem contadores. Se algum bot
  ficou de fora por falta de estado, uma nota no topo avisa que o computador não informou o estado
  de alguns bots. O Hub continua enviando todos os cartões; o filtro é só da tela.
- **Rótulos.** `working` Trabalhando, `needsInput` Precisa de você, `waiting` Aguardando, `error`
  Erro. Fila indisponível: “Fila não disponível”. Uso: três linhas separadas, “Não informado”, com
  atalho para o Uso existente. A contagem por estado só conta os cartões listados desta fotografia;
  um estado sem bot não aparece (nada de zero).
- **Abrir.** Só navega. Num space, abre exatamente `open.conversation` (fluxo do
  `SpaceContext`); no Pessoal, a conversa própria do bot (ID igual ao do bot). Outra conversa no
  Pessoal não tem caminho hoje: Abrir fica desligado, sem cair na principal. A `sessionRef` é
  recebida mas não seleciona sessão: não há seletor de sessão no app nesta etapa.
- **Pendências.** Com permissão original pendente, o cartão vai para “Esperam você”. Responder ali é
  descrito abaixo; sem como responder ali, o botão é **Abrir para responder**, que leva à conversa do cartão.
- **Parar.** Só **Abrir para parar**, com trabalho informado, que navega à conversa onde está o
  controle legado. A nota diz que ele vale para o que estiver em andamento ao tocar, não só o
  turno, e pode cancelar a fila e os pedidos pendentes da conversa. Sem conexão, fica desligado.
- **Responder no Board.** Para cada `pending` listado pelo Hub, o Board procura o cartão
  original no store do Hub da faixa (`allEntries(conversa)`, o mesmo espelho que a conversa e a Ilha
  usam) e desenha o **mesmo `PermissionCard`**, com as opções do próprio cartão. Responder chama a
  mesma `BotStore.respond` → `respondPermission {botId, entryId, entryRev, optionId}`, sem `source`,
  RPC ou atalho novo (`views/fork/torre/responder.ts`). O Hub aceita só a primeira resposta e recusa
  cartão que não está mais pendente ou com revisão antiga, com a mensagem de sempre. Resolvido aqui ou
  na conversa, o cartão mostra o resultado e a nota **Já respondido** (ou **Não está mais esperando**,
  se cancelado/vencido); o que foi respondido daqui fica numa seção “Respondido agora” até fechar a
  tela, só em memória. Sem conexão, fora do Pessoal (a resposta no space exige o contexto da
  conversa do space) ou sem o cartão carregado: só **Abrir conversa**, com o motivo. Nenhum texto do
  cartão vem do snapshot: o conteúdo é o que a conversa já mostra ao dono.
- **Pergunta do bot.** O produtor é a ação explícita `ask_owner` (só no Pessoal); nada é inferido
  pela última mensagem. A pergunta aparece em “Esperam você” com o campo de resposta pelo mesmo `send` do
  composer (ver [Perguntas ao dono](#perguntas-ao-dono)). Enquanto envia, o campo fica travado
  com o texto (“Enviando…”); se o envio falha, aparece “Não foi enviada”, o texto continua e **Tentar de
  novo** reenvia com o mesmo nonce, sem gravar uma segunda mensagem. Respondida só quando a pergunta sai da
  lista do Hub. O rascunho (texto, origem e nonce) fica num store do renderer por Hub e space, chaveado pela
  pergunta, só em memória: sobrevive ao Board sem conexão e à reconexão (o campo volta com o texto e o estado
  do envio) e é limpo quando a pergunta sai da lista numa fotografia ao vivo.
- **Cadeia.** Aprovações que sobem por Equipes/Chefe não têm produtor; nenhuma aparece. Quando
  existirem, entram só com **Abrir** e uma etiqueta correspondente.
- **Fora.** Pausar próximos, folha nova de Parar e Ver andamento (sem botão, só a frase).

Prova visual feita com componente real e dados sintéticos; não é o app instalado.

## Pedidos entre bots

Seção no fim do Mission Board, depois dos blocos por estado, com os pedidos
`ask_bot`/`message_bot` dos últimos 7 dias que **não terminaram bem**, do mais novo
para o mais antigo.

- **Hub.** `host/src/fork/retornos/pendentes.rs`: `ler_na` é a leitura canônica e
  reutilizável dos registros de pedidos e resultados, só com a `Connection` recebida,
  tipada pelo mesmo leitor de estados; versão ou combinação estado/motivo
  desconhecida descarta a linha. `torre_na` roda na Tx da projeção: só no Pessoal (outro
  space devolve `unavailable` sem ler os registros), só pedidos cujos **dois** bots estão no
  roster já montado (faltou um, o pedido some inteiro), só terminais sem sucesso e
  incertos (nunca `accepted`, `running` nem `finished/completed`), até 20 itens e
  `omitted` com o resto. Fonte `retornos.pendentes`, cobertura
  `personalOwnConversationsLast7Days`.
- **Item.** `key` (ID opaco, só chave de lista), `requesterBotId`, `targetBotId`,
  `state` (`finished|cancelled|interrupted`), `reason`, `outcome`, `mode`
  (`wait|async`) e `endedAt`. Sem texto do pedido, resposta, digest, avisos, cadeia,
  sessões ou `ConversationRef`.
- **Autoridade.** A mesma `control_tower::access::require` do snapshot; não há RPC,
  evento nem ferramenta nova. O bot continua com `bot_request_status` para o próprio pedido.
- **Rótulos.** Incerto após reinício (`restart_uncertain`, nunca concluído nem falha),
  Não começou (reinício), Interrompido, Prazo estourou, O bot parou, Cancelado,
  Não começou, Falhou, Parado, Incompleto (Cancelado e Parado aparecem). Modo: “esperou
  resposta” ou “recado”; tempo contado da hora da fotografia.
- **Abrir** é o único botão: reaproveita o destino do cartão de quem pediu (`bot.open`
  → `goTo`); sem cartão aberto, fica desligado. Não retoma, não repete, não cancela e
  não responde.
- **Contadores.** Fora dos números do topo; o total aparece só no título da seção.
- **Vazio e indisponível.** Pessoal sem pedidos: “Nenhum pedido entre bots ficou
  pendente nos últimos 7 dias.” Outro space: “aparecem só no Pessoal”. Pessoal sem o
  campo (Hub antigo): **Não informado**, nunca “nenhum”.
- **Atualização.** Sem gancho novo: o aviso final de cada pedido atualiza entradas das
  conversas de quem pediu e de quem recebeu, e esse `entry` já faz a `TowerSession`
  reler. A saída da janela de 7 dias só muda na próxima leitura.
- **Mirror.** O campo só existe no snapshot efêmero; `MIRROR_SCHEMA` não muda.

App: `src/shared/fork-torre-pedidos.ts` (tipos, parser, rótulos) e
`views/fork/torre/PedidosEntreBots.tsx` + `pedidos-entre-bots.css`. Testes:
`host/src/fork/retornos/pendentes_testes.rs` e `src/shared/fork-torre-pedidos.test.ts`.
Prova visual feita com componente real e dados sintéticos; não é o app instalado.

## Permissões de turno delegado em Pontas soltas

Checkpoint de permissão delegada com **Abrir** apenas. O Hub já
grava o vínculo cartão ↔ pedido (`donna_delegated_permission_checkpoints`); esta etapa
define como o app o mostra. Campo novo, aditivo, do snapshot efêmero:
`delegatedPermissions: Observation<{ items, omitted }>` (`version` continua 1,
`MIRROR_SCHEMA` não muda).

- **Item mínimo.** `key` (ID opaco do pedido), `requesterBotId`, `targetBotId`,
  `entryId` e `conversation {conversationId, revision}` do cartão original (os mesmos de
  `pending` do bot que recebeu), `mode` (`wait|async`) e `requestedAt`. Sem texto do
  pedido, comando, argumento, cerca, tentativa, `ExecutionRef`, `SessionRef` ou ACP.
- **Audiência (Hub).** Mesma transação e mesma `control_tower::access::require`
  do snapshot; só vínculos cujo `space_id` é o Space pedido, com os **dois** bots no roster
  já autorizado desse Space, pedido ainda `running` e cartão ainda `pending`. Outro Space,
  falta de bot, cartão respondido ou leitura que falhe: `unavailable` ou item omitido, sem
  revelar que existe. Limite de 20 itens e `omitted` com o resto.
- **Fail-closed no app (pai).** `parseDelegatedPermissions` descarta item incompleto, com
  modo desconhecido ou com pedinte igual ao destinatário; campo ausente (Hub anterior) ou
  torto é `unavailable`. `delegatedRows` só aceita o item cujo cartão a **própria
  fotografia** ainda lista como pendente do bot que recebeu, na mesma conversa; sem isso a
  linha some. Sem conexão (fotografia antiga), a seção não aparece.
- **Apresentação.** Seção “Permissões pedidas por outros bots” antes de “Pedidos entre
  bots”, só quando há linha: “X pediu a Y”, pílula “Espera sua permissão”, modo e tempo, e
  só **Abrir**. Abrir leva à conversa de **quem recebeu** (onde está o cartão); no Pessoal,
  só a conversa própria do bot (`openTarget`), senão fica desligado. Não aprova, não nega,
  não retoma e não responde; responder continua no cartão original em “Esperam você”.
  Fora dos contadores e do selo do topo (o cartão já conta uma vez em `pending`).
- **Arquivos.** `src/shared/fork-torre-permissoes.ts`, `views/fork/torre/PermissoesDelegadas.tsx`
  (CSS reaproveitado de `pedidos-entre-bots.css`, só tokens), chaves `torre.permDeleg.*`
  nos 6 idiomas. Testes: `src/shared/fork-torre-permissoes.test.ts` e
  `views/fork/torre/permissoes-delegadas.test.ts`.
- **Hub.** A projeção é `host/src/fork/control_tower/delegated_permissions.rs` (fonte
  `retornos.delegatedPermissions`, cobertura `pendingCardsInAuthorizedConversations`). Hub
  anterior sem o campo: o app trata como indisponível e a seção não aparece.

## O que aconteceu

Coluna à direita dos blocos, acima do detalhe do bot, com o **último dia fechado** no
horário do dono (padrão 19:00): "O que aconteceu · ontem, até 19:00". Fora dos
contadores do topo e sem "atualizar agora".

- **Conteúdo.** Por equipe (área do quadro): a linha da equipe (do gerente da área ou junta
  pelo computador, sem IA, marcada assim) e uma linha curta por bot. "Bots sem equipe"
  para quem não tem área nem chefe com área. "Fora do resumo: <bot> (Codex) —
  resumo só com assinatura Claude" em cartão tracejado, também para chave de API,
  outro motor ou sem assinatura Claude própria. Rodapé: "Um resumo por bot por dia, sem
  ferramentas e sem memória · não aprova nada · próximo hoje às 19:00", com o botão
  **Horário** (liga/desliga e hora, de 15 em 15 min, de 02:00 a 23:45, no fuso do aparelho).
- **Hub.** `host/src/fork/resumo_diario/`. Agendador de 60 s; profundidade `d` (topo = 1)
  roda em `T − 15·d min` (no máximo 8 níveis), de baixo para cima, e o de cima espera os
  de baixo do dia. Entrada só com conversas próprias no Pessoal (`user`/`agent`),
  pedidos registrados (metadados) e resumos recebidos; sem atividade e sem resumo recebido, zero
  chamada. Chamada pelo executor de fundo (`one_shot_own`, `resolve_own`), sem
  ferramentas, sem memória e sem sessão; sem modelo escolhido, Haiku com esforço baixo (sem
  Haiku, fora do resumo). Pausa global (conferida antes de cada bot) e pausa do bot impedem sem
  consumir o dia. Resumos recebidos valem só no próprio dia. Idempotente por bot e dia;
  reinício retoma uma vez.
- **Contrato.** `{enabled, time, timeZone, day, closesAt, nextAt, teams:[{teamId, name,
  line, bots:[{botId, line}]}], loose:[{botId, line}], out:[{botId, reason}]}`, com
  `line = {text, joined}`. Fonte `resumo_diario`, cobertura `personalLastClosedDay`.
  Outro space: `unavailable`, sem ler nada; a tela explica que o resumo existe só no
  Pessoal. Só o snapshot efêmero; `MIRROR_SCHEMA` não muda.
- **RPC.** `donnaResumoDiarioSave {enabled, time, timeZone}`, com a mesma guarda do
  snapshot (`control_tower::require`, Local ou Control). Emite
  `donnaControlTowerChanged` com `rosterChanged`; o fim de cada passada também.

App: `src/shared/fork-torre-resumo.ts` e `views/fork/torre/OQueAconteceu.tsx` +
`o-que-aconteceu.css`. Testes: `host/src/fork/resumo_diario/testes.rs` e
`src/shared/fork-torre-resumo.test.ts`. Prova visual feita com componente real e
dados sintéticos.

## Validação

Os testes focados em `host/src/fork/control_tower/tests.rs` cobrem o fluxo API, os cinco
estados, runtime desconhecido, ausência de fila/uso, navegação sem efeito,
avatar oficial preservado conforme o reader canônico sem cruzar Spaces,
separação entre Spaces, principal em conversa diversa, fechamento, canal sem
executor, repouso limpo e associação atual de canal pela fonte canônica,
pendências originais únicas e catálogo, Device/lease/revogação após
espera, invalidação sem corpo com revalidação no canal, interleaving de membro
removido depois de enfileirar o hint real e antes do consumo, controle positivo
com associação preservada, saída legítima do contexto anterior e stream real após
troca/exclusão da principal com vínculo estrangeiro já removido. Há também
consulta realmente suspensa na trava administrativa e hint enfileirado antes
de revogar/reconceder o mesmo Device: ambos recusam a autoridade original
encerrada, enquanto nova autenticação permanece válida. Isto não é aceite nem
prova de app instalado.


## Dependência de Privacidade e limites futuros

O bloqueio legado foi resolvido pelo produtor de
Privacidade, mesclado na main consumida. A Torre não recria o
redator: `runtime_audiencia::project_in_space` é apenas a entrada contextual da
mesma projeção canônica já usada por bot/hello/evento. Na entrega de hints de
runtime, `runtime_audiencia::permits_conversation_in_space` reutiliza a regra
de audiência para a conversa capturada sem exigir a lane atual. A conexão DB de coleta é
liberada antes de observar runtime/chamar esse produtor; não há Store recursivo.
A fonte antiga permanece histórica; seu diagnóstico A/B não é bloqueio novo.
A integração preserva memória, cadeias, upstream, catálogo nativo,
skills, sessões, conectores e Equipes. Nenhum campo ou false de pausa é enviado.

Pausa pública não existe na main fixada. Controles ficam em etapas futuras; o
Stop inicial da UI será somente Abrir para parar pelo fluxo já existente. Sem
RequestRef exata de execução aceita, Ver andamento fica indisponível; não
inferir execução por bot, nome, Lane, estado ou catálogo nativo. Ficha/avanço/uso
de ajudantes e árvore por etapa dependem de identidades duráveis e
permanecem futuras, sem painel nesta etapa. Torre própria/navegação de Equipes
virá depois da RPC na main. Não há consulta engine.accounts, probe privado
ou rede de provedor por render.
