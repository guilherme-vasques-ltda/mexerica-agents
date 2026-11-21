# Spaces e conversas

Contrato tipado: `apps/desktop/src/shared/fork-spaces.ts`; wrappers `SpaceClient` e `ConversationClient` em `fork-spaces-client.ts`. Campos camelCase; IDs opacos no escopo do Hub; revisões inteiras monotônicas.

Spaces são contextos do mesmo dono. Pessoal é criado uma vez, sempre primeiro, sem renomeação, movimentação, troca de tile ou remoção. Sua apresentação (`owner`) vem somente de escolha explícita do dono, nunca de uma conta de modelo. A exclusão definitiva existe somente em `donnaSpaceDelete`, com as guardas descritas na seção de ciclo de vida. O endpoint legado `donnaSpaceCreate` cria apenas metadados vazios, sem bot, conversa, memória, conta, perfil, ferramenta ou rotina. O novo fluxo `donnaSpaceBirth`, descrito abaixo, cria principal próprio; isso não é uma invariante global sobre todo Space.

## RPCs

As novas RPCs recebem `hubId` além dos campos abaixo. Ele é capturado de `hello.hostId`, nunca do rótulo/Computer.id, e validado antes de consultar o contexto. Os wrappers o injetam e recusam outra identidade; respostas dessas RPCs repetem `hubId`. O token/canal autentica o transporte; o campo fixa o destino da intenção, sem criar ACL/grant. APIs legadas continuam com omissão de Space exclusivamente Pessoal.

- `donnaSpaces {}` → `SpacesSnapshot`.
- `donnaSpaceCreate SpaceCreate` → `SpacesSnapshot`.
- `donnaSpaceUpdate SpaceUpdate` → `SpacesSnapshot`. `expectedRevision` é a revisão da coleção recebida; revisão obsoleta retorna conflito.
- `donnaSpaceReorder SpaceReorder` → `SpacesSnapshot`. A lista contém todos os IDs uma vez, Pessoal primeiro.
- `donnaOwnerPresentation {expectedRevision, owner: OwnerPresentation}` → `SpacesSnapshot`. Não altera ID/nome/ordem de Pessoal.
- `donnaConversations {spaceId}` → `{spaceId, conversations: SpaceConversation[]}`.
- `donnaConversationCreate {spaceId, botId, title?}` → `{conversation: SpaceConversation}`. Conversas legadas podem referenciar a identidade principal existente; isso não transforma essa identidade no principal de outro Space. Nenhuma execução ocorre ao criar.
- `donnaConversationUpdate {spaceId, conversationId, expectedRevision, title?, closed?, engineAccountId?, engineSelection?, expectedAccountRevision?}` → `{conversation}`. Seleção contém `{backend, model, modelOptions}`. Binding exige revisão atual da conta e modelo comprovado no catálogo; `null` remove o binding e impede execução adicional. Metadados só mudam com o ator ocioso. A conversa principal não pode ser fechada; fechamento conserva histórico e não substitui Stop.
- `donnaConversationHistory {spaceId, conversationId, beforeSeq?, limit?, query?}` → `{context: SpaceContext, entries, hasMore}`. Busca, corte e paginação validam o mesmo destino antes de consultar.
- `donnaConversationSend {spaceId, conversationId, expectedRevision, text, clientNonce, attachments?, replyTo?, contentOrigin?}` → `{context, entry, replayed, queued?}`. Só conversas adicionais; recusa conversa fechada, IDs cruzados e binding ausente/alterado. `queued: true` confirma fila durável, não admissão pelo motor. O selo privado do Hub prende intenção, revisão da conversa, conta/revisão/modelo/opções e contexto até a admissão e o dispatch. Nonce repetido precisa da intenção original exata e devolve a entrada anterior com `replayed: true`, sem executar novamente, inclusive após rebind/ABA. Uma intenção nova com revisão antiga falha; o cliente deve reler e pedir uma intenção nova.
- `donnaConversationChoiceAnswer {spaceId, conversationId, expectedRevision, questionId, revision, answer}` → `{context, question, entry, replayed}`. Resposta do dono ao cartão de escolha (`entry.data.genuiChoice`). Mesma audiência de `donnaConversationSend`: Local ou Device com Control, `hubId` e Space conferidos, só conversa adicional aberta. `answer` é `{kind:"option",id}`, `{kind:"none"}` ou `{kind:"text",text}`, objeto fechado. CAS por estado `pending` e `revision`; revisão antiga, estado final, outra conversa/Space (`genui_question_scope_mismatch`), pergunta anterior à limpeza do contexto (`genui_context_reset`) ou resposta diferente recusam sem efeito. Repetir a mesma resposta já gravada devolve a mesma entrada com `replayed: true`. A resposta vira entrada `user` com `choice_response`, `status: queued` e o selo da conversa, entregue à fila canônica do ator; não é grant, aprovação nem fala autoral.
- `donnaConversationRead {spaceId, conversationId}` → `{conversation}`. É a marca de leitura do cliente humano (`readRevision`), nunca recibo de que o bot leu/consumiu e nunca grant. Continua exigindo sinal próprio explícito.
- `donnaConversationStop {spaceId, conversationId, expectedRevision, entryId}` → `{context, stopRequested, entryIds}`. A entrada deve pertencer à conversa. Cancela essa entrada na fila ou o turno que a contém, inclusive mensagens agrupadas desse turno; não cancela outro contexto. O cancelamento do motor é cooperativo, com limite existente do ator.
- `donnaConversationNewSession {spaceId, conversationId, expectedRevision}` → `{context, conversation, reset: true}`. Exige ator ocioso e conversa sem fila durável. Conserva transcript/arquivos/memória; fixa uma nova âncora para não recompor mensagens anteriores. Não move a sessão de outra conversa. `context_control_pending` é resultado ainda desconhecido: releia o estado antes de repetir.
- `donnaConversationRetry {spaceId, conversationId, expectedRevision, entryId}` → `{context, retryRequested: true}`. Exige espera de assinatura explícita do ator, com cartão e intenção original capturados; aceita o ID da mensagem original desse turno, nunca outro cartão/turno. Turno comum em execução retorna `conversation_not_waiting`. Só confirma após persistir a transição para retomada da mesma intenção/binding/conta/modelo; não confirma execução concluída. A espera adicional é exclusiva de pedido direto do usuário, nunca rotina/grupo/delegação. Cada retomada revalida o selo original e as revisões até dispatch, sem resolver outra conta/modelo. Após reinício durante uma tentativa, fica pausada para conferência explícita de efeitos incertos; uma espera limpa conserva o horário de retomada. Stop encerra somente a espera capturada e reset/rebind continuam recusados enquanto ela existir.
- `donnaConversationUpload {spaceId, conversationId, expectedRevision, uploadId, name, offset, data, done}` → `{context, attachment, revision?, replayed?}`. Base64 em blocos de até384KiB, máximo100MiB. Manifesto persistido por conversa, nome imutável, repetição byte a byte e SHA256 final. Arquivos/ancestrais não seguem symlinks e hardlinks são recusados. O manifesto persistido é revalidado em append/read/resolve: nome é um único componente relativo sem separadores, controle, `.`/`..` ou path absoluto; tamanho, digest, completude e revisão devem ser válidos. O helper de abertura também recusa nomes inválidos; não sanitiza para outro arquivo. Uploads contextuais exigem filesystem Unix suportado; outras plataformas falham explicitamente.
- `donnaConversationReadUpload {spaceId, conversationId, uploadId, offset}` → `{context, attachment, data, size}`. Só manifesto completo e digest atual da conversa selecionada.
- `donnaConversationReadFile {spaceId, conversationId, entryId, fileId, offset}` → `{context, data, size}`. A entrada e o snapshot publicado precisam pertencer ao destino capturado.
- `donnaConversationMemory {spaceId, conversationId, method, expectedRevision?, ...campos}` → resultado da API de memória mais `context`. Leitura: `memory`/`memoryFile`. Escrita: `saveMemory`/`pinMemory`/`archiveMemory`/`forgetMemory`/`clearMemory`/`saveMemoryFile`/`deleteMemoryFile`/`restoreMemoryFile`, com revisão atual. IDs numéricos, versões e arquivos continuam validados no namespace real, sem fallback para memória pessoal.

`id === botId` identifica a conversa principal legada de um bot real: envio/upload/binding continuam nas APIs existentes de bot, agora com `spaceId` fixado. Não enviar essa entidade à nova API de envio/binding: ela recusa explicitamente em vez de ignorar a escolha. Uma conversa legada do Berga do Pessoal em outro Space (`foreign`) é adicional, com ID próprio, sem bot/grupo substituto. O Berga nativo de um Space tem bot próprio e conversa canônica com `id === botId`. `primary` indica a conversa inicial daquele Space/identidade; isoladamente não escolhe transporte. `donnaConversations.principal` traz somente ID/nome/avatar de apresentação do principal próprio do Space (ou do principal global no Pessoal), sem prompt/configuração/credenciais. Em Space legado sem `principalBotId`, retorna `null`, sem substituir pelo Pessoal.

`revision` é o CAS de metadados/binding. `readRevision` é exclusivamente humana. `stateRevision` ordena projeção/eventos, incluindo leitura/continuidade/invalidação de memória, sem invalidar uma intenção por marcar lido. Mudanças de memória invalidam somente as conversas que veem esse namespace, sem publicar seu conteúdo no evento. Não usar esses campos como grants ou recibos.

As mutações de metadados exigem transporte local autenticado e não criam grants. Preferências de Space não relaxam regras, Guard ou aprovações existentes. `SpaceAppearance` conserva os controles de piso de contraste do renderer; não substitui o cálculo do vidro. `appearanceConfigured: false` no Pessoal migrado manda conservar preferências locais até uma escolha/importação explícita; novos Spaces começam com aparência própria configurada. Wallpaper customizado permanece no armazenamento do dispositivo até existir transporte próprio de assets. Logos aceitam PNG estático limitado ou um subconjunto passivo de SVG, até256KiB/1024px; foto do dono aceita PNG. URLs, paths, SVG ativo/rede/entidades e assets fora dos limites são recusados. No SVG, `width`/`height` explícitos são decimais finitos entre1 e1024 (sem unidade ou `px`); porcentagens e outras unidades não resolvidas são recusadas. Sem ambas as dimensões, exige `viewBox` válido para resolver a ausente. `viewBox` tem quatro decimais finitos, origem entre−1024 e1024, largura/altura entre1 e1024 e extremos dentro desse intervalo; valores não finitos, overflow e geometria fora desses limites falham antes da persistência.

## Contexto, compatibilidade e eventos

`spaceId` acompanha chamadas contextuais, bots e eventos. Omissão no protocolo legado significa exclusivamente Pessoal; um ID de outro Space nunca muda esse destino. Seletores explícitos legados também vinculam o recurso efetivamente consultado: `conversationId` diferente do destino de `botId`/`chatId`/`peerId` ou da entrada é recusado antes da consulta, sem redirecionar para outra conversa. Recursos adicionais de memória/upload usam as RPCs contextuais, sem fallback pelo bot original. Clientes novos devem fixar o Space em um wrapper por montagem e descartar resposta/evento cujo contexto/geração já mudou. Persistência local e rascunhos devem ser chaveados por Hub/Space/conversa. MIRROR_SCHEMA 37 invalida os caches anteriores, inclusive os que não registram `activityCode`. O Hub preserva `activity` como texto legado e projeta `activityCode` opcional com a mesma audiência e redação; clientes que não reconhecem o código usam o texto legado. BotSync só projeta hello/Spaces/conversas e respostas auxiliares se o cliente e a geração capturados ainda estiverem vivos, inclusive A→B→A ou substituição de store/transporte; nomes e Computer.id não validam identidade de Hub.

O catálogo/registro de skills atual é exclusivo de Pessoal. Os métodos `skills`,
`marketSkills`, `skillDetails`, `installSkill` (catálogo/manual) e `removeSkill` recusam
Space adicional antes de leitura, contagem, metadados instalados ou efeito, com
`skills_personal_only`. Não devolvem vazio como se houvesse catálogo naquele Space.
Pessoal explícito passa pelo mesmo guard comum de Hub/Space/bot/conversa; só então uma
adaptação interna tipada retira o seletor Pessoal validado do payload estrito de skills.
Seletores conflitantes e campos desconhecidos não são apagados. Caller e contexto são
revalidados antes de dados e após esperas de mutação. Skills por domínio permanecem
plano próprio; esta contenção não ativa heranças ou ferramentas adicionais. Veja o
[contrato do catálogo](catalogo-skills.md#escopo-pessoal-e-seletores-de-contexto-space-skills-1).

SSE `/events?...&spaceId=<id>` e assinatura channel `events` com `spaceId` entregam somente recursos daquele contexto. Sem seletor, entregam Pessoal. Eventos de Spaces são metadados do Hub. Eventos desconhecidos sem classificação de contexto não entram por fallback no stream de outro Space. Cada entrada nova identifica seu contexto persistido; resposta atrasada conserva o destino original.

O legado migra transacionalmente para Pessoal preservando os IDs das entradas/bots, sessões, contas, grants, arquivos e vínculos. A migração de Spaces tem versão própria, distinta do schema SQLite herdado e do mirror. Reabrir o banco não duplica dados.

Uma conversa é uma entidade persistida, distinta da sessão ACP. A sessão, transcript e pasta são próprios da conversa. Memória explícita da mesma identidade/Space pode atender suas conversas; não atravessa Spaces. Fora de Pessoal, memória pessoal/shared, fontes, skills, ferramentas e rotinas legadas não são herdadas. Conversas adicionais no próprio Space do bot anunciam `memory` e, em Pessoal, também `chat` e `team`, pela mesma função de decisão usada pela principal. Nesta etapa, `connectors` e proxies stdio/HTTP atendem somente conectores já ligados; setup/cartões conservam a origem. `composio`, `computer`, `computers`, `routines` e `manage` não são anunciados nas adicionais; chamadas contextuais a esses servidores são recusadas antes de efeitos. A principal conserva seus critérios atuais. `search_history` e `open_history` continuam exclusivos da sessão `main`, mesmo quando o servidor `memory` está disponível. Nada é ativado ao criar Space. Contas são recursos do Hub selecionados por referência, sem copiar perfil/credencial.

Nas adicionais próprias, a Alma já é a do bot e a memória explícita usa seu namespace original e o compartilhamento efetivo da principal; a pasta gerenciada da adicional não amplia esse acesso. O Hub emite slot e vínculo de sessão, captura conversa/sessão/turno/selo por chamada e revalida após esperas. Fechamento, outra identidade/Space, troca de sessão/turno/revisão/conta/binding ou origem inválida recusam a chamada, sem redirecionar à principal. Mensagens, reações, arquivos, avisos e cartões permanecem na conversa emissora; paths relativos de arquivos usam sua pasta. Memória conserva o contrato de propostas da main: aplicação só após sucesso, recibo e Desfazer nessa origem; sem confirmação antiga. Codex omite e recusa mutantes, inclusive nas adicionais. A autoedição do perfil continua recusada pelo fork.

A admissão de `team` acontece no ator emissor: a última validação, a reserva de capacidade, os avisos e o envio ao destinatário formam um passo síncrono, ordenado com Stop, a persistência da reconfiguração e a exclusão do bot. Capturas e cartões das adicionais próprias incluem uma geração monotônica privada do perfil, persistida na mesma transação de cada mudança de configuração, inclusive por gestão/Undo: restaurar valores antigos não restaura uma aprovação. Read marks não mudam essa geração. A exclusão só encerra o ator e cancela trabalho depois do commit durável; se esse commit falhar, as reservas são liberadas sem invalidar trabalho ou publicar estado candidato. Selos próprios anteriores à geração exigem nova admissão, sem promover cartões antigos.

A resposta de permissão nativa revalida o cartão após esperar o pipe e em cada tentativa de escrita. IDs e opções válidos não têm um limite adicional de tamanho. O corpo pode sair em partes, mas o Hub retém a chave de fechamento do JSON e a newline até admiti-las juntas sob a barreira de revogação. Esses dois bytes cabem na escrita atômica do pipe POSIX; antes deles, o prefixo é JSON incompleto, inclusive no EOF. Revogação recusa a conclusão e fecha o pipe daquela sessão invalidada; uma opção longa válida conserva a sessão. Nenhuma barreira síncrona atravessa espera de I/O. O contrato de `foreign` permanece separado.

Em `team`, o aviso do remetente fica na adicional. O destinatário explícito do mesmo Space conserva ferramentas e autoridade próprias: `ask_bot` devolve a resposta à chamada original; `message_bot` mantém o relatório na conversa do destinatário, conforme seu contrato de trabalho independente. Trocar a conversa do remetente não transfere a resposta, e resultado incerto não autoriza repetir efeitos.

Berga atendendo um Space diferente do original (`foreign`) continua sem Alma, conectores, skills, logins, computador, gestão, memória pessoal e ferramentas; sua memória contextual continua separada. O histórico recebe uma vez por conversa o aviso: “Neste espaço a Donna não usa suas instruções, ferramentas nem a memória do Pessoal.” Reinício, reabertura e novos turnos não o duplicam. O aviso usa uma entrada `notice` existente, sem campo novo no mirror.

Continuidade adicional usa a mesma sessão somente enquanto aquele processo/contexto está vivo. Após troca de processo/conta/contexto ou reinício, não chama `session/load` usando um raw-SID potencialmente ambíguo. Abre sessão fresca e, quando há mensagens anteriores, recompõe exclusivamente texto visível da conversa persistida do Hub. `continuity.mode` é `fresh` ou `fresh_reconstructed`, com `evidence: prepared_by_hub` e cobertura real. Isso não promete retomada provider-native nem consumo pelo modelo. Limites:200 entradas examinadas,40 mensagens,8192 caracteres por mensagem e64KiB de dados; cobertura/truncamento são explícitos. Texto antigo é citado como dado com origem/IDs/revisões e marcado externo pelo Guard. Não reproduz toolcalls, aprovações/grants ou efeitos incertos. Trabalho em voo interrompido por reinício é sinalizado, sem replay automático.

## Consumidor no app do Mac (leitura)

O seletor no topo da barra lateral troca a loja que alimenta o app inteiro.
`SpaceMirrors` conserva uma `BotStore` por Space conhecido, mais a raiz do Pessoal,
com a mesma rota autenticada e `HostClient` preso ao `spaceId`. A faixa fica só com
ferramentas. O principal vem de `SpacesSnapshot.principalBotId`; o app não consulta
nem altera a principal global para escolher o Berga de um Space.

As conversas canônicas (`id === botId`) reutilizam `send`, `upload`, `readUpload`
e `respondPermission`. Upload usa o mesmo transporte RPC do Pessoal, em blocos de
384 KiB, até 100 MiB; o Hub valida o bot/entrada no Space antes do efeito.
Aprovação permanece sujeita ao cartão e à revisão originais. Grupo/canal não exige
motor nem conta e seus membros precisam pertencer ao mesmo Space.

O hello RPC confirma o Space solicitado; Space inexistente é recusado. O hello do
stream traz `spaceId` e as conversas canônicas, inclusive a de cada bot não principal.
Eventos são filtrados no Hub e no cliente. A reconexão invalida respostas da geração
anterior; seleção, cache, rascunho e anexo não atravessam Space. Cache novo usa
contexto da conta, computador, `hello.hostId` confirmado e Space; schema 36 descarta
transcript antigo sem remover os rascunhos. Rascunhos sem namespace de Hub são
copiados uma vez, mantendo a chave original. Anexos de rascunho ficam só em memória.

Busca, ilha e bandeja usam referências com Space explícito; abrir um bot troca o
contexto antes de selecioná-lo. Command+Space aponta ao Berga do Space ativo.
⌘1…9 escolhe na ordem do seletor, ⇧⌘K abre o menu, ⌃Tab/⌃⇧Tab percorre Spaces.

A tabela `fork-space-capabilities.ts` concentra os recursos habilitados. Conversa,
anexos, aprovações e canais funcionam. Equipes, Rotinas, Sessões, Conectores/cofre,
Projetos e Skills permanecem apagados; Skills indica “Só no Pessoal”. Isto é
contenção de interface, sem ampliar nem substituir a autorização do Hub.

`SpaceView` deixa de ser a tela principal. `SpaceContext` fica restrito à leitura
do histórico adicional, com Hub/link/geração/Space/conversa capturados e descarte
de páginas atrasadas. Não se oferece criar uma conversa adicional do Berga do
Pessoal em outro Space. As existentes permanecem legíveis como “Berga do Pessoal”,
sem compositor, envio, nova aprovação ou migração de dados. Space sem Berga oferece
a ativação do Berga. Hub antigo e queda conservam a identidade do contexto e mostram
indisponibilidade, sem substituí-lo pelo Pessoal.

## Audiência do runtime no BotJSON

Hello/catch-up completo, sync incremental e evento `bot` usam a mesma projeção
de runtime do Host. O enum interno mantém cinco estados; o JSON acrescenta
`status: "unavailable"`. Sem comprovação da conversa canônica aberta no Space
do bot, a projeção envia `activity: ""` e `startedAt`, `resumeAt`, `workingChat`
nulos, sem indicar onde há trabalho. Lane desconhecida/estrangeira, vínculo
revogado, conversa fechada, bot excluído ou falha de leitura não viram idle,
waiting ou error fictícios. Erro sem lane também não prova audiência. Idle
sem lane só é exposto limpo e com conversa própria canônica aberta.

Grupo do mesmo Space exige associação atual; outra conversa da mesma identidade
exige o vínculo canônico. Eventos já enfileirados revalidam audiência na entrega.
A projeção não muda Runtime, identidade, perfil, memória, última mensagem,
capacidades, revisão, roteamento ou autoridade existentes. `team.list_bots` usa
o mesmo status redigido, conservando sua seleção de bots. Indisponibilidade não
comprova contagem zero nem autoriza inferir conclusão ou tentar outro Space.

O consumidor deve tratar unavailable explicitamente e invalidar o cache antigo. No app do Mac, o normalizador
trata `unavailable`, status desconhecido ou ausente como **Estado indisponível**,
sem atividade, horários ou conversa de trabalho. Caches anteriores, inclusive
10/11/12, entram assim até o Hub responder, sem apagar rascunhos ou histórico.
Principal, linha da barra lateral, detalhes, rosto, conversa, caixa, ilha e bandeja
não mostram relógio, trabalho, Parar, contagem ou aviso de concluído/erro para
esse estado.
A TUI faz o mesmo (`◌ Estado indisponível`, sem hora nem aviso).


## Audiência do uso da tela e de Live Activity

Na Mexerica, estado de tela acrescenta `agentUse: "idle" | "using" | "unavailable"`.
`agentBot` contém ID somente com using; nos demais estados é null. Idle exige
que o broker não observe posse ativa. Posse sem audiência canônica fica
unavailable, sem nome, destino, contagem, horários ou afirmação Ready. Os demais
campos de conexão/permissões conservam suas regras. Ver
[estado da tela por Space](../features/remote-screen.md#estado-da-tela-por-space-na-mexerica).

Hello RPC/stream, respostas de tela e evento screen usam a mesma fonte. Eventos
são revalidados por Space na entrega, incluindo os que estavam na fila antes
de mudança de lane, principal, grupo/conversa ou exclusão. A projeção usa Space
solicitado separado da configuração nativa; para a principal em outro Space,
confere a seleção atual e a conversa real aberta que a referencia. O BotJSON
continua nativo e não é clonado para simular uma permissão de outro contexto.
O evento de tela projetado chega também aos Spaces adicionais, sem conceder
controle ou modificar a posse do broker.

`registerActivity` mantém `{}` e os tickets existentes; o emissor comum relê
BotJSON, inclusive quando chamado por mudança de runtime do Hub ou agregado.
Unavailable/unknown/ausência não viram Idle/end e não consomem ticket. Idle
sem lane não comprova o término daquele ticket; por segurança não envia end,
inclusive depois que um turno privado limpou sua lane. Mensagem humana ainda
sem resposta também conserva o ticket. Com contexto comprovado, os estados
reais e o fio móvel existente são preservados; não há nova mensagem móvel de
indisponibilidade, relay habilitado ou alteração de cliente iOS/Android.

A UI de Screen usa `MIRROR_SCHEMA` 13: o app
normaliza todo estado de tela, só nomeia com `using` e, ao perder o link (ou
com resposta de outro link), fica sem a identidade de quem usa, mantendo
`computerUse`, conexão e permissões. A vista de um space lê o estado e os nomes
dos próprios bots no stream daquele space, aberto desde 0 para o catch-up
trazer os bots que já existiam.

Composto sobre o `ExecutionRef` de Sessões, cada pedido MCP adicional conserva um `ToolContext` até o retorno, inclusive unlock e renovação. IDs e sequência do pedido não recapturam uma concessão após expirar. A geração da definição cobre handshake/descoberta anteriores à ativação ACP, sem autorizar efeitos. Cartões de setup só retomam na conversa original com geração, sessão e turno conservados; confirmação/entrada e fila são revalidadas. Não há promessa de desfazer efeito já admitido nem retry de resultado incerto.


## Nascimento do Space com seu principal

O desktop exige `hello.spaceBirthContract === 1`. Novo Space passa por identidade
e escolha da conta do principal: nome fixo Berga, motor e conta conectada do Registry daquele Hub,
sempre visível e confirmada. A referência padrão conectada pode ser sugerida;
nenhum perfil, credencial ou autorização é copiado. Hub antigo bloqueia esse fluxo
com orientação de atualização, sem voltar à criação vazia.

`donnaSpaceBirth {hubId, expectedRevision, clientNonce, name, tile,
principal:{name?,backend,engineAccountId,appearance?}}` devolve Snapshot. Space, bot nativo,
conversa canônica e recibo de idempotência são gravados juntos. Conta/autoridade
são verificadas no Hub; `hubId` só determina origem, nunca concede acesso. Retry
conserva a intenção: mesmo nonce e conteúdo não duplicam; conteúdo divergente
com o mesmo nonce é recusado. Eventos e ator vêm depois do commit.

`Space.principalBotId` é opcional no contrato aditivo. Pessoal conserva o principal
global. Spaces antigos e clientes legados conservam o contrato de metadados de
`donnaSpaceCreate`; não há migração, promoção do primeiro bot, cópia ou escolha
silenciosa. O principal novo tem memória própria, sem memória compartilhada,
ferramentas, logins, skills, preferências ou poderes de gestão do Pessoal. Sua
referência é protegida contra exclusão e transferência. O nome do principal é fixo
("Berga"): Birth aceita nome ausente ou "Berga". Principais legados conservam seu
nome até o dono escolher "Renomear para Berga"; nenhuma terceira identidade é aceita.

Birth aceita `Caller::Local` ou aparelho do dono com Control e vínculo ACS4 ativo, ativação, sincronização e lease válidos. A autoridade original é capturada antes da espera em `team_lock` e revalidada antes e dentro da transação; revogação não recaptura uma concessão nova. A conta conectada é relida na transação. Isso não promove o transporte a Local: `donnaSpaceCreate`, `donnaSpaceUpdate`, `donnaSpaceReorder` e `donnaOwnerPresentation` permanecem locais.

A garantia de principal próprio cobre somente o novo fluxo Birth. O endpoint legado e Spaces antigos podem continuar sem principal; não há migração nem imposição de uma invariante global nesta entrega. A limpeza de falha remove somente pastas novas vazias; pastas pré-existentes não são adotadas pela preparação e conteúdo prévio não é removido.

## Principal em Space antigo e exclusão

O desktop exige `hello.spaceLifecycleContract === 1`. Um Space `kind:"space"` sem
`principalBotId` mostra "Ativar o Berga" na vista e na folha de edição; a ativação é explícita,
nunca migrada. `donnaSpacePrincipal {hubId, spaceId, expectedRevision, clientNonce,
principal:{name?,backend,engineAccountId,appearance?}}` devolve Snapshot. Nome é
fixo "Berga" e a persona é `donna_fixa::BASE_SPACE`, sem promessa de gestão; `appearance` aceita somente o estilo
Simples v2 sem `modeloId` (figuras do app são conveniência de UI). Mesma autoridade
do Birth, mesma idempotência por nonce+digest em `donna_space_principal_receipts`;
Space, vínculo `principalBotId`, bot, conversa canônica e recibo gravam juntos.
`principalBotId` já gravado recusa (`space_principal_exists`); `updateBot` só conserva o nome legado ou o troca para "Berga"; outro nome
recusa com `space_principal_name_fixed`. Descrição, papel, instruções, preferências
e poderes são fixos (`space_principal_fixed`). Cor/figura e a seleção confirmada de
motor/conta passam pelo caminho autorizado do dono. A ativação conserva corpo, nonce
e revisão para replay após resposta perdida; a vista aberta relê o principal novo.

`donnaSpaceDelete {hubId, spaceId, expectedRevision, confirmName}` devolve o Snapshot
sem o Space ou `{code:"space_busy",busy:[{botId,name}]}` quando algum bot do Space tem trabalho vivo
(fila, permissão, espera de retomada, admissão, pergunta do dono, cartão GenUI,
handoff ou rotina ativa), inclusive nas conversas foreign do Berga do Pessoal. O
ator confere turno, admissão, conclusão, fila e colaboração em RAM e fica reservado
até terminar a transação; a recusa traz IDs/nomes, sem atividades. `confirmName` precisa
ser exatamente o nome do Space; Pessoal e Space inexistente são recusados. A mesma
autoridade do Birth é exigida e, para aparelho do dono, a plataforma registrada em
`devices` precisa ser de computador (lista de permissão `macos`, `linux`, `windows`; ausente/desconhecida recusa). Numa única transação:
Space sai do snapshot, bots são marcados `deleted`, e conversas, entradas, sessões,
uploads, recibos de Birth/ativação, times, delegação, catálogo nativo, roteamento,
plugins, seções, memória automática, projetos e KV do Space são removidos; marcadores
de triggers SQL (`donna_space_delete_*`) autorizam a remoção do vínculo dentro da
Tx e somem com o commit. Erros antes do commit não publicam estado nem encerram
atores. Depois do commit:Shutdown dos bots, publicação de eventos, cancelamento de
times/canais e remoção de arquivos gerenciados, como no delete de bot. O app só
oferece a exclusão no aplicativo do computador, com o nome digitado.

O compositor e o + de bot, o seletor no topo trocando todo o app, sessões,
rotinas, conectores (MCP/contas de serviço e cofre por Space), Equipes
e Projetos/Kanban ficam fora deste fluxo.


A exclusão mantém Pessoal, outros Spaces, contas de IA e itens do cofre. Os grants
somem junto dos bots, sem apagar os logins do Hub. Histórico de custo e recibos de
custódia sem conteúdo são mantidos; runs de rotinas, eventos e detalhes do alvo são
apagados. Pastas gerenciadas e camadas UUID de memória foreign são coletadas antes
da remoção e descartadas depois do commit em melhor esforço. Falhas auxiliares não
convertem um commit em recusa. Backups e cópias externas dos fornecedores podem
permanecer. A confirmação informa esse alcance e começa com foco em Cancelar.
