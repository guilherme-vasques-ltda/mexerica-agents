# Tarefas (Projetos, Lista e Quadro)

Em cada Space válido do mesmo Hub (por exemplo, Estúdio), o dono pode criar Projetos com
nome, objetivo e coordenador opcional, organizar tarefas e registrar descrição, aceite e
evidência textual. Os registros pertencem ao par Hub+Space e são duráveis; não vêm do
snapshot do Mission Board.

O dono organiza e move as tarefas; atribuir responsável ou coordenador, mover cartões ou
arquivar um Projeto não avisa nem inicia bot. Os bots do Space só
mudam tarefas pelas ferramentas próprias, na execução autenticada: o responsável atualiza
estado, nota e evidência da própria subtarefa, e Concluída exige prova (o app mostra
“Concluída (declarada)”); supervisor e coordenador podem mudar a subtarefa de outro bot, com
registro no histórico; qualquer bot cria subtarefa dentro dos limites. Os estados são A
fazer, Em andamento (declarado), Em conferência, Concluída, Bloqueada e Cancelada.

No app, a área Tarefas abre na **Lista** (padrão); o **Quadro** é outra vista dos mesmos
dados e filtros. A Lista começa por **Esperando você**, atalho para as tarefas Em
conferência de qualquer Projeto (não é um estado), e segue por grupos de estado;
Concluída e Cancelada ficam recolhidas no fim. Subtarefas aparecem recuadas sob a
principal; com filtro, a lista fica plana e cada linha mostra “Parte de: …”. O Quadro
tem as seis colunas, com as tarefas filtradas de todos os Projetos. A busca (título,
descrição e nome do Projeto) e os filtros por Projeto e por bot valem para as duas
vistas. A vista escolhida e o último Projeto usado ficam só neste aparelho, por Space
(`localStorage` `mexerica.tarefas.v1`); nada disso vai ao Hub.

No detalhe da tarefa, digitar `@` no campo de comentário lista os bots do Space. Sem
`@`, o comentário só anota e nenhum bot o vê; com `@`, o app manda só a intenção
(`mentions`) e o Hub decide quem é avisado. O interruptor **Só anotar** só liga com
`@`. Embaixo de cada comentário do dono, a linha de aviso mostra o que o Hub devolveu,
por destinatário (avisando, avisado, respondeu, não avisado com o motivo, incerto); a
tela nunca diz “ninguém avisado” se alguém recebeu. Um envio incerto fica para Tentar
de novo com o mesmo pedido, e o texto só sai do campo quando o Hub confirma esse pedido.

O Hub aceita até 50 Projetos por Space, 100 tarefas por Projeto e três níveis contando a
raiz. Cada tarefa pode ter um pai do próprio Projeto; ciclos são recusados. Não há
exclusão de tarefas: cancelar preserva o registro. Nome comporta 120 caracteres;
objetivo e aceite, 2000; título, 200; descrição e evidência, 4000 cada. Controles
perigosos e pedidos acima de 512 KiB são recusados. Evidência é texto digitado pelo
dono: nenhuma referência é buscada, expandida ou aberta automaticamente.

Somente bots nativos (`BotConfig.space_id` igual ao Space atual), vivos e visíveis
aparecem na seleção, com ID e nome. Um responsável ou coordenador removido ou oculto
conserva a referência, sem revelar seu nome privado. O app apresenta essa ausência como
indisponibilidade e pode salvar outras alterações conservando o ID. Novas atribuições
exigem um bot elegível do Space atual. Sem bots nativos, o dono pode usar Sem
responsável e nenhum coordenador; não há fallback para Berga global ou para um bot do
Pessoal por conversa estrangeira.

As RPCs privadas do app são `donnaProjects`, `donnaProject`, `donnaProjectCreate`,
`donnaProjectSave`, `donnaProjectEnsureDefault`, `donnaTasks`, `donnaTaskMetaSave`,
`donnaTaskComments` e `donnaTaskComment` (Projetos) e `donnaTaskStart`,
`donnaTaskSchedule`, `donnaTaskAgenda` e `donnaTaskSessions` (início, agenda e sessões
vinculadas). Todas exigem `hubId` e `spaceId` explícitos e recusam campos extras.
Só Local e Device com Control e autoridade original do transporte podem acessá-las; não
são ferramentas MCP nem recursos de bots. Hub diferente da conexão e Space inexistente
são recusados. IDs de outro Space não resolvem Projetos, pais, responsáveis ou
coordenadores no Space atual. A autoridade é revalidada depois da espera, na mesma
transação que confere a existência do Space e na saída. Um dispositivo revogado não
conserva autorização antiga.

`donnaTasks` é uma leitura adicional, com a mesma autoridade e escopo. Retorna por
vez um Projeto ativo e suas tarefas, em ordem de ID, e um cursor `next` para a próxima
página; `afterProjectId` nulo inicia a leitura. A paginação não é um snapshot: uma
alteração concorrente exige reiniciar a leitura. A edição continua exigindo releitura
do Projeto completo e sua revisão; essa leitura não executa bots.

`donnaTaskMetaSave` grava prioridade, prazo, fases e fase atual em `donna_task_meta`,
fora do JSON de tarefas, para que clientes antigos de `donnaProjectSave` não apaguem
campos que não conhecem. A gravação usa a mesma revisão CAS do Projeto, recibo
idempotente e histórico `task_meta_updated`; fases só existem em tarefa raiz. O
snapshot de `donnaProject`/`donnaProjectSave`/`donnaTaskMetaSave` também carrega `meta`
atual, para que a releitura que antecede a escrita traga o mesmo estado que a revisão
CAS protege. A data/hora de início opcional não faz parte desses metadados: é
salva por `donnaTaskSchedule`, com a capacidade `taskSchedule`, para agendar o
acionamento do responsável. A capacidade antiga `taskStartAt` não é anunciada.

Comentários do dono exigem `@Nome` real no texto para cada menção declarada: a
gravação recusa lista divergente e grava `mentionCheck: 2` no entry — marcador de
validação que só o Hub produz (o corpo da RPC é `deny_unknown_fields`), provando
que cada menção teve `@` real. A audiência persistida são os IDs dos bots,
estável a renomeação posterior. Entradas anteriores sem o marcador não têm prova
de `@`: entrega e leitura por menção as tratam como não verificadas (fail-closed)
até revalidação explícita — política documentada, sem reescrever registros. `declarations` é um mapa por tarefa
(`{botId, at}`) gravado somente na transição real para `done` por escrita de bot,
na mesma transação; some quando a tarefa sai de `done`, some do Projeto ou quando
o dono conclui a tarefa (transição para `done` = conferência humana). Tarefas já
`done` antes desta versão não têm declaração e ficam desconhecidas — nunca
atribuídas por suposição. O campo é aditivo nos snapshots e em `donnaTasks`;
clientes antigos o ignoram.

`donnaTaskComments` lê os últimos 100 comentários de uma tarefa com `omitted`;
`donnaTaskComment` grava comentário do dono com `clientRequestId`, texto de até 4000
caracteres, menções limitadas a 20 bots visíveis do Space e `onlyNote`. O autor é
sempre `owner` nesta RPC; `author` no corpo é recusado. Menções são resolvidas contra
o roster do Space e duplicadas são descartadas. Sem `@`, `notify` fica
`not_notified/no_mention` e nada é entregue a bot. Com menção e `onlyNote`, fica
`not_notified/only_note`. Com menção ativa, `notify` nasce `notifying` e o Hub chama
`fork::tarefas_runtime::avisos::deliver` depois do commit; a leitura recompõe o estado final a
partir de `donna_task_deliveries` via `avisos::status_of`. Comentários de bot e sistema
nunca chamam `deliver`.

`donnaProjectEnsureDefault` é idempotente por Space: devolve o Projeto padrão ativo ou
adota um Projeto ativo chamado `Geral`, ou cria `Geral` como Projeto normal editável.
`donnaProjects` expõe `defaultProjectId` e `capabilities`; a capacidade só aparece
quando o método correspondente existe e foi testado. Nesta etapa são anunciadas
`manualProjects`, `taskList`, `taskMeta`, `taskComments`, `defaultProject`, `taskStart`,
`taskSchedule`, `taskAgenda` e `taskSessions`; o app só mostra cada controle com o anúncio.

“Concluída (declarada)” é um fato gravado, não uma dedução do histórico: `declarations`
(`{[taskId]: {botId, at}}`, na página de `donnaTasks` e no snapshot do Projeto) recebe a
entrada quando um bot leva a tarefa a Concluída (com prova) e a perde quando a tarefa sai
de Concluída ou quando o dono a leva a Concluída. Sem a entrada, o app mostra “Concluída”.

Falha da guarda de autoridade retorna HTTP 400 com
`not_allowed: autorização original encerrada`. Esse erro pode decorrer de queda
de sincronização e não comprova revogação definitiva: preserve a fotografia somente
na mesma origem, marcada como antiga e sem escrita até uma nova leitura autorizada.
O pedido recusado não recaptura autoridade nem prossegue com efeitos. Recusas explícitas
continuam HTTP 403; Hub divergente e Space indisponível têm a mesma resposta genérica,
sem revelar existência ou nomes de outro contexto.

Cada Projeto tem revisão própria. Saves usam `expectedRevision` e `clientRequestId` UUID
canônico. Conflito exige releitura; não sobrescreva. Reenvie um pedido incerto com o
mesmo ID e conteúdo. O Hub conserva o recibo original e retorna o snapshot **atual**,
inclusive após alterações posteriores. O mesmo ID com outro conteúdo no mesmo Hub+Space
é recusado; o mesmo UUID em outro Space é independente; recibos armazenam digest
canônico, sem cópia do texto.

A edição compara o rascunho com o conteúdo base visto ao abrir a folha e com a versão
atual do Hub. Uma alteração concorrente não recebe silenciosamente a revisão nova;
havendo conflito, o dono escolhe usar a versão atual ou manter os campos que editou.
O envio também confere a fotografia usada como base. As setas de foco e Alt+seta para
mover seguem a direção visual, inclusive em RTL. Em Projeto arquivado, criar,
editar e mover tarefas ficam bloqueados tanto pelos controles quanto pelo teclado.

O histórico é derivado pelo Hub, com autor `owner`, `system` ou `bot:<id>`, campos
alterados e transições manuais de estado por tarefa, sem nomes privados. A resposta
inclui os últimos 100 registros e `historyOmitted`. Histórico e recibos permanecem
conservados no armazenamento do Hub, isolados por Hub+Space; não há quota vitalícia
de gravações por Projeto.

Leitores que exibem `system`/`bot:<id>` enviam `historyAuthors: true` em toda chamada
que devolve snapshot (`donnaProject`, `donnaProjectCreate`, `donnaProjectSave`,
`donnaProjectEnsureDefault`, `donnaTaskMetaSave`), inclusive em replays idempotentes:
o campo é opcional, não entra no conteúdo do recibo e cada pedido projeta conforme a
própria flag. Sem ele, o Hub projeta só entradas `author:"owner"` — leitores antigos
continuam válidos, sem reescrever autoria — e, havendo entradas de sistema ou bot no
Projeto, acrescenta `historyPartial: true` e `historyHidden` com o total omitido;
`historyOmitted` passa a contar apenas a janela do dono.

`donnaProjectsChanged` invalida a leitura no stream existente. Seu payload público
contém só `hubId` e `spaceId`, com filtro de dono, autoridade vigente e Hub+Space do
stream original. O app assina antes de ler e relê ao receber invalidação. Desconectado,
só leitura da fotografia marcada como antiga. `BotSync.principalEpoch` avança a cada
`hello`, inclusive na reconexão: identifica um ciclo do stream, não o dono. Reconectar
na mesma fonte (`BotStore` do contexto/conta), Hub e Space preserva fotografia e
rascunho em memória, antigos e sem escrita até uma nova leitura autorizada. Respostas
do ciclo anterior nunca pintam. Um pedido em voo cujo resultado ficou incerto conserva
o mesmo ID e corpo para tentativa explícita, sem reenvio automático.

Trocar a fonte/conta, Hub ou Space, ou receber recusa explícita HTTP 401/403, limpa a
projeção, os rascunhos e os pedidos incertos; respostas anteriores não devem repovoar
a tela. Preservar a fotografia durante reconexão não renova a autoridade de pedidos
antigos no Hub. O chip identifica o nome do Space atual. Hub sem `manualProjects`
fica indisponível, sem alternativa por chat.

Não há dependências automáticas, execução observada nem alteração de Equipes nesta etapa;
iniciar, agendar o início e sessões vinculadas só aparecem no app quando anunciados. Esta etapa não cria o bot principal de um Space, modelos, contas
ou sessões. Recuar a interface preserva os registros no Hub.

O esquema inicial de Projetos, histórico e recibos contém `hub_id` e `space_id`, com
chaves e consultas compostas e referências ao Projeto do mesmo escopo. Não há migração
de dados legados.
