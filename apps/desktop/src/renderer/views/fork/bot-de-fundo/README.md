# Bot de fundo — UI D246 A

Implementa Configurações → Bot de fundo a partir da proposta R4 aceita pelo revisor
GPT-6-Sol xhigh independente `cr_child_853d643c894b88eb77a83f80`. Os cinco artefatos
aceitos em `(arquivo de design privado do projeto de origem, fora deste repositório)` ficam congelados;
provas do renderer são separadas. Não inclui Parte B.

`functionModels`, `saveFunctionModel` e `checkFunctionModel` são administrativos
Local somente. Conferir é resolução local, não execução, cota ou identidade por
trabalho. As contas/modelos são os devolvidos pela API, sem registry paralelo.
Voz abre a página existente. Salvar não muda permissões ou a política de aprovação.

Arrumação (auditoria D249, E5; texto aprovado sem mudança): Regra do dono 15/11: sem abas; a página mostra as funções e o cartão **Neste Space**, só com a seleção do Space ativo (`selecaoDoSpace` em `model.ts`; o Hub e o contrato não mudam, e as seleções dos outros Spaces continuam salvas). A linha Computador é um cartão `surface`; o estado (sem conexão, carregando, aviso ou erro de leitura) e **Recarregar escolhas** dividem uma linha; Atividade recente usa o título de seção das outras páginas (13 semibold).

`backgroundSpaceSelections` e `saveBackgroundSpaceSelection` usam o contrato do
root: referências/revisões, sem cópia do Binding. O Hub valida Local ou a capacidade
SAAS-04 `engine.accounts` com guarda original. A UI não infere autorização pela rota,
por Device/Control ou por parâmetros do corpo. Autorizar exige a seleção carregada
coerente com a revisão da referência; remover conserva CAS mesmo sem current.

`BackgroundStore` guarda estado efêmero por computador/conexão. Respostas antigas
não atravessam a troca de destino. Rascunho usa a revisão de abertura; releitura de
outra edição não rebaseia a intenção. Timeout/mutação incerta exige releitura, sem
resend automático. Eventos incrementam apenas backgroundEpoch para refetch, sem
catálogo no BotStore. Erro ou RPC ausente deixam a seção indisponível.

Histórico #154 integrado pela API real `backgroundHistory {}` da main875d0587,
Local somente. Retorna array de até50 registros, na ordem entregue pelo computador.
O parser exige status tipado: response_received/applied/unchanged/proposed/deferred/
failed. Resposta recebida não confirma aplicação; Adiado não promete nova execução.
effectiveModels pode ser vazio (Não informado), sem inferir modelo da seleção.
failureStage é null/ausente ou selection/account/usage/execution/revalidation;
quando a falha não traz etapa, aparece Não informada, sem inventar causa detalhada.
usage é opcional: apenas inputTokens/outputTokens/cachedInputTokens/cachedWriteTokens,
inteiros seguros e não negativos. Ausência não vira zero. Não há conta detalhada,
moeda, preço, custo estimado ou inferência D257.

`demo.ts` é uma fixture opt-in importada manualmente no Vite; não está na árvore de
imports do aplicativo. setHistory permite metadados sintéticos apenas para provas;
por padrão o histórico é indisponível. Nenhum modelo é executado. Provas em Chromium da
sandbox, sem Mac/app instalado. Testes/typecheck/gates são centralizados no root;
o dev UI não executa os runners nem mutações Git nesta etapa.

## Correção após revisão independente UI R1

Parecer `cr_child_4078250b0bf680ca0ed04eac`: NÃO, com três achados. O retorno de
saveBackgroundSpaceSelection pode chegar depois do evento e de uma leitura mais
nova. O snapshot da mutação agora só é validado; sucesso ou erro fazem nova leitura,
sem reenviar a intenção. O contador de leitura impede que uma reconciliação em voo
substitua evento posterior. Conferências são invalidadas no início de toda leitura;
respostas de check anteriores a essa leitura são descartadas, mesmo sem mudar a
revisão do documento (a disponibilidade da conta pode ter mudado).

A disponibilidade usa connectedClient público, incluindo heldDrop, incompatibilidade
e aposentadoria do store. O painel não chama ready para aguardar reconexão. Modelo,
leitura e mutação conservam também a identidade do cliente utilizável antes/depois
do await. Durante a tolerância visual de queda, os controles ficam fechados e não
há intenção enfileirada para uma conexão futura.

Seis testes focados adicionais cobrem as ordens de resposta/evento/refetch, erro
sem replay, check concluído/em voo e queda sem fila. Runners continuam centralizados
no root. Prova no renderer: após evento, check available cedeu a account_disconnected;
33ms depois de derrubar o stream real da demo, zero controles administrativos ativos
e zero chamadas novas, antes dos 5s da tolerância visual. Prova não executa Host ou
conta real. Os cinco artefatos aceitos R4 permanecem congelados.

## Correção após revisão independente UI R2 — projeção Space

functionModels permanece Local-only. Um dono remoto autorizado por engine.accounts
pode receber a projeção administrativa mínima de Space.current sem ler o documento
global: accountLabel/modelId obrigatórios junto de functionRevision/accountId/
accountRevision. A UI mostra essa escolha por espaço e permite vínculo/reautorização
via o RPC Space original, cuja guarda continua no Hub. Não cria cadastro ou catálogo,
nem trata rótulo/modelo como prova de identidade, cota ou execução.

Se o documento global estiver acessível, divergência de revisão/conta/modelo bloqueia
a autorização e exige releitura; a UI não combina nome de uma fonte e modelo de outra.
Sem projeção válida a seção falha fechada. Payload de salvamento enumera somente as
três referências e CAS/space/function/enabled; accountLabel/modelId nunca vão ao RPC
com deny_unknown_fields. Remover continua possível por CAS, inclusive current null.

Três testes focados novos cobrem Global recusado + Space válido para vincular,
reautorizar/remover, payload exato, divergências e projeção incompleta. Os seis
ensaios de races da correção R2 foram preservados. Não foram executados pelo dev UI.
Prova no renderer com a API global recusada completou as três intenções; ao tornar
o global acessível com revisão divergente, a UI exigiu releitura e bloqueou envio.
Os cinco artefatos aprovados R4 permanecem inalterados.

Clicar na função já selecionada não modifica o rascunho, mesmo em conflito.
Descartar continua sendo ação explícita de releitura; teste de regressão cobre
a perda encontrada pela revisão automática após o primeiro CI verde.

## Integração do histórico após merge #154

Leitura direta do contrato atual em modelos_funcao/api.rs, fork/api.rs e
fork/background_history.rs nesta worktree. Sem RPC extra, fallback, cadastro ou
ampliação da fronteira Local. 403/API ausente mostram indisponibilidade neste acesso;
resposta inválida não mostra registros como se fossem confirmados. Erros livres,
campos extras e botId não fazem parte da projeção visual. Histórico vazio só é
apresentado após array vazio confirmado.

Cada load invalida o histórico anterior imediatamente. Sua consulta publica
independentemente das escolhas, protegida por generation/reading e identidade do
cliente utilizável; evento backgroundHistoryChanged usa backgroundEpoch existente.
Releitura, erro e troca de computador/conexão não reaproveitam linhas antigas.
Nenhum gancho do BotSync/HostClient foi alterado. Rascunhos, seis testes de racesR2,
projeção remotaR3 e o teste15 do no-op ao selecionar o mesmo ID foram preservados.

Seis testes novos cobrem status obrigatório e metadados ausentes, resposta malformada,
projeção dos quatro tokens sem extras/valores inválidos, indisponibilidade/403 sem
fallback, consultas atrasadas com publicação independente e cerca scope/link/close.
Não executados pelo dev UI; gates e revisão NOVA são centralizados pelo root.
Fontes limitadas aos seis arquivos próprios desta pasta, fora da raiz D259 congelada.
Root cuida das referências documentais do produto em docs/features/bot-de-fundo.md.

Prova autoral no renderer real via Vite/browser-harness: três registros sintéticos
mostraram status, modelo efetivo, quatro tokens (inclusive zero informado), ausências
de modelo/uso/etapa e distinção entre resposta recebida e aplicação. DOM sem conta,
custo, erro bruto ou ações no histórico. Evento de refetch removeu linhas antigas;
403 exibiu indisponibilidade Local sem fallback; status ausente exibiu consulta
inválida. Provas separadas em /tmp/memauto-ui/panel-history, fora dos5artefatosR4.
Aba e Vite próprios encerrados após a prova. Nenhum runner/Host/appMac executado.

## Entrega 4 (MX-7)

A página explica o que é, agrupa as funções pelo `consumer` do Hub (Em uso, Em preparo, Voz) e abre a escolha
na própria linha. Atividade recente saiu daqui: é a página **Ajustes → Atividade** (`fork/atividade/`), com
o mesmo `readHistory`. O botão Conferir saiu da tela (`BackgroundStore.check` continua, sem uso na view).
Proposta e capturas: `(arquivo de design privado do projeto de origem, fora deste repositório)`.
