# Contas de motor

O Hub mantém um registro comum das contas de motor já configuradas. Ele é separado
dos logins salvos e das contas dos aplicativos. Não há credencial por bot.

Nesta etapa, a consulta `donnaEngineAccounts` recebe `{}` e retorna `accounts`, com
ID estável, rótulo seguro, marca, backend, tipo de cobrança, estado, modelos/opções
comprovados e `hiddenModelIds` — os modelos que o dono ocultou naquela conta
(preferência global no Hub; ver [setup e recuperação](setup-provedores.md)). A consulta exige a capacidade própria `engine.accounts` do dono e não passa pelo mirror.
A gestão e seleção explícita estão em [Contas de provedores e setup guiado](setup-provedores.md), sem escolha automática de motor. A única troca automática é a do [fallback entre contas do mesmo motor](#fallback-entre-contas-do-mesmo-motor), só pela lista que o dono montou e sempre com aviso.

O resultado fica em cache por cinco minutos. Consultas simultâneas aproveitam a
mesma sondagem, e o registro só é regravado quando muda. O dono local ou o Device da posse ACS4 com capacidade vigente pode
enviar `{ "refresh": true }` para forçar uma consulta. Control ou owner.devices
isolados não permitem consultar ou atualizar contas. Uma renovação já em andamento também
atende quem pediu atualização enquanto esperava.

Uma assinatura só é comprovada quando o próprio CLI informa o tipo de autenticação,
um plano reconhecido e uma identidade de conta. Claude Code usa `auth status`;
Codex usa `account/read` no app-server com `refreshToken=false`. O CLI ainda pode
renovar o token e atualizar seu `auth.json`, como em qualquer uso normal, além de
atualizar o próprio cache de modelos. O Hub não lê nem escreve `auth.json`. Login genérico,
sucesso de sessão e nome do provedor não bastam. Respostas desconhecidas ficam
`unverified` e não podem participar da reserva. Falhas de consulta ficam
`unavailable` nas contas de perfil CLI já registradas daquele motor, mantendo seus
IDs e sem criar outra conta. A recuperação reconecta o mesmo ID. CLI ausente ou
resposta definitiva sem login (`loggedIn=false` ou `account=null`) desconecta as
contas anteriores; uma falha não é tratada como logout.

O catálogo contém somente modelos e opções observados, sem enviar prompts. Codex
fornece `model/list` e seus esforços. Claude Code não anuncia catálogo no `auth status`:
por enquanto, sua lista fica vazia. Isso não comprova execução, cota disponível ou
compatibilidade de um modelo. Os consumidores precisam revalidar antes de usar.

Configuração no cofre só é identificada quando o cofre já estiver desbloqueado e
o perfil cifrado contiver algum valor não vazio. A consulta não desbloqueia o cofre
nem migra registros antigos; perfis que não podem ser conferidos são omitidos.
Contas anteriores ausentes da sondagem permanecem `disconnected` no registro.
Um slot vazio não cria conta. Conteúdo existente continua não comprovado: sua
presença não comprova chave nem cobrança. Autenticação explícita de
API é rotulada `paid_api`, sem autorizar gasto, reserva ou execução por API.

IDs e identidade de cota são derivados com um sal privado persistido. Novos logins
não substituem silenciosamente o ID anterior; contas iguais não viram duas cotas.
No Codex, a identidade inclui o workspace informado em
`workspaceRouting.chatgptAccountId` e o plano. Sem workspace, usa o plano como
distinção; sem ambos, fica `unverified`. Trocar workspace ou plano cria outro ID e
desconecta o anterior. Quando o CLI só informa o plano, dois workspaces do mesmo
plano não podem ser distinguidos por esta consulta. Claude inclui a organização.
O registro privado guarda referências de perfil/cofre e escopo do dono; a resposta
nunca contém credenciais, e-mail, organização, caminhos, referências ou saída crua
dos motores. O Hub não lê arquivos de autenticação, não cria login e não faz
logout/login global. A proteção continua sujeita à limitação de Hub e bots rodarem
sob o mesmo usuário do sistema.

O [resolvedor de modelos por função](modelos-por-funcao.md) consome este cadastro
persistido, sem iniciar sondagens ou executar modelos. O lançamento isolado e a
reserva são etapas posteriores. Esta consulta não habilita execução nem cadastro
por pessoa.

A consulta conserva a posse original durante a sondagem e a espera pelo cache.
Antes de persistir o catálogo e entregar a resposta, revalida essa mesma posse.
Troca de dono/grant/geração, revogação ou expiração recusa a resposta antiga, sem
substituí-la por uma conta do novo titular. Consulte [setup e recuperação](setup-provedores.md).

O motivo de uma falha de preparo (`reason` no aviso e na mensagem) cita a conta
por `accountId` opaco e o app a procura nesta mesma lista: achada, mostra rótulo
e `state`; não achada, a frase admite que não confirmou em vez de inventar
estado. Ver [falha de conta ou motor](setup-provedores.md#falha-de-conta-ou-motor-ao-preparar-um-pedido).

## Uso por conta e por Space

`donnaEngineAccountUsage` lê a cota da própria conta gerenciada, por conta e por
Space: `spaceId` ausente vale o Pessoal, `null` ou tipo errado responde
`invalid_space`. **O Pessoal também é um Space:** qualquer Space só atende quando há
vínculo comprovado com a conta — padrão de um bot do Space, conversa do Space ligada
à conta ou seleção da conversa (atual ou próxima) de um bot do Space; é a mesma
lista de bots que bloqueia a remoção da conta (`bot_ids_in`), só que filtrada pelo
Space. Sem vínculo, `account_wrong_space` e a conta não é consultada. Fazer o Pessoal
enxergar todas as contas seria decisão do dono, não desta etapa.

Cada backend confirma a identidade no processo/perfil da consulta antes de medir:
Codex no mesmo app-server (`account/read`), depois `account/rateLimits/read`;
Claude pelo `auth status` do perfil isolado, depois `/usage` com sessão não
persistente, fontes de configuração desligadas e MCP vazio. A cota global do
provedor nunca é reaproveitada. As janelas chegam normalizadas como `session` e
`week` com `percent`, `resetsAt`/`resetsText` e `nearLimit` (a partir de 90%).
Sem leitura comprovada — CLI ausente, identidade divergente, resposta inválida —
o estado é `unavailable` ("desconhecido"), nunca 0%, e a janela ausente simplesmente
não aparece na lista.

**Codex.** A leitura reaproveita o `codex_usage::parse` do painel global: prefere o
bucket `codex` de `rateLimitsByLimitId` e descarta a janela cujo reset já passou
(uma janela que renovou não marca uso antigo e não vira "perto do limite"). Sessão e
semana são classificadas pela **duração** da janela (300 min = sessão, 10080 min =
semana), nunca pelo nome do campo: numa conta só semanal a `primary` é a semana. O
nome do campo só vale quando o Codex não informa a duração; duração de outro
tamanho não é nenhuma das duas.

**Custo.** Cada leitura sobe um processo do CLI. O Hub guarda a leitura por conta
(e identidade da cota) por 60 s (15 s se falhou), faz uma leitura por vez por conta
(quem chega durante ela espera a mesma resposta) e corta a leitura em 25 s. Uma tela
com várias linhas ou trocas rápidas de conversa não empilha processos.

**Limitação conhecida (Claude).** A prova de identidade (`auth status`) e a medição
(`/usage`) rodam em dois processos do mesmo perfil isolado; existe uma janela curta
em que o login do perfil poderia mudar entre os dois. O plano aceita ("auth status
do perfil") e a pasta é isolada por conta.

O app mostra as duas janelas nos Detalhes da conta (Ajustes → Contas de IA) e um
resumo na linha da conta da conversa — "62% da sessão, 40% da semana, volta
14:00" — em desktop e web. O iOS só lê. Sem lista de reserva (ainda o padrão), cota esgotada para o turno com o aviso de limite da assinatura, sem
relançar outra conta, modelo ou motor; a troca é a do fallback, abaixo.

## Fallback entre contas do mesmo motor

Código em
`host/src/fork/fallback_contas/` (estado, escolha, prova, API, cartão do Mission Board) e `host/src/agent/bot/fork_fallback.rs`
(ganchos do ator). **Só a conversa do dono no Pessoal** (principal e sessões extras); rotina, grupo, canal e conversa de outro Space
seguem como antes (espera por limite da assinatura), com o mesmo portão antes do envio e no limite — ficam para uma etapa futura.

**Ordem de reserva.** `donnaEngineFallbackSave {spaceId?, backend, botId?, accounts}` grava a ordem do Space por motor, ou
a ordem só de um bot (`accounts: null` com `botId` tira a do bot). Até 3 reservas, cada conta uma vez, mesmo motor, só **assinatura
comprovada e ligada** (`subscription_verified`); chave de API e conta `unverified` são recusadas (`fallback_not_subscription`) e, se
deixarem de ser comprovadas depois, deixam de ser escolhidas. A conta da conversa é o passo 0. **Sem lista, nada muda.** Leitura:
`donnaEngineFallback {spaceId?}` (ordens, contas descansando, conversas numa reserva). Mesma capacidade `engine.accounts` das demais
chamadas de conta; só o dono, com a autoridade original reconferida na transação. **A reserva precisa de vínculo de verdade com o Space**, independente da própria lista: um bot do Space tem de usar a conta
(padrão do bot, conversa ou seleção) — constar da ordem, ou rodar numa reserva por causa do fallback, não conta. Sem isso o
`Save` recusa (`account_wrong_space`) e a prova falha. Nesta versão o `Save` só aceita o Pessoal
(`fallback_space_unsupported` nos demais); a tela só aparece no Pessoal e só oferece contas vinculadas a ele.

**Como a troca acontece.** A troca é uma **sobreposição por conversa** (`fork.fallback-contas.sessao.<id>`) lida dentro de
`engine_choice::read`: permit, recibo do Roteamento, carimbo e lançamento enxergam a conta nova; a escolha do dono não é reescrita, e
a conversa volta sozinha quando a conta dela volta a ser a primeira livre. A conta nova abre sessão própria (pasta isolada: a sessão
da outra não é lida) e leva a passagem em resumo, como a troca de motor (`chat::handoff`).

**Antes do envio.** No início de um turno humano novo, o Hub percorre `origem → reservas` e fica com a primeira conta
**livre**: assinatura comprovada, mesmo motor, modelo comprovado, sem bloqueio de 1 h, sem descanso e sem leitura **>= 90%** da
sessão **ou** da semana (a leitura é a de uso por conta, válida por 60 s por conta). Sem leitura comprovada só valem esgotada e erro — mas uma conta com descanso por "perto do limite" vigente **continua parada** sem leitura, e só volta com leitura comprovada abaixo de 85% (histerese contra o vai-e-volta no limiar) ou quando o horário de volta chega. Aviso
na conversa: "Conta A perto do limite; seguindo com Conta B." Sem conta livre o preparo **para** (`quota_near_limit`/
`quota_exhausted`, o mesmo caminho das falhas de preparo: a fila tenta de novo sozinha a cada 5 s), com o aviso e a hora em que
volta. `donnaEngineFallbackAction` `allowExtra` ("Permitir uso extra até o limite voltar") vale **só até o reset informado** e nunca
sem esse sim; `backNow` ("Voltar agora") encerra o descanso e o bloqueio. Limitação dita ao usuário: o pedido em curso que cruza o
limite pode ser cobrado, e uso da mesma assinatura fora da Mexerica não aparece na leitura — por isso 90% e não 100%.
Trocar de motor (ou de modelo) pela **cadeia do Space** não está nesta etapa (desligada por padrão); sem reserva livre o Hub para.

**Cota esgotada no meio.** A conta que esgota descansa até o reset informado (ou 1 h sem horário). Havendo reserva
elegível, o pedido **sem efeito possível** (nenhuma ferramenta, mensagem ou texto) volta à fila e sai na conta nova no turno
seguinte; com efeito possível o turno **para como incerto** ("confira antes de refazer"), nada é refeito às cegas, o próximo pedido
segue na reserva e a passagem leva a marca de incerto. Cartões de permissão abertos expiram com o turno (nada migra de
conta) e toda aprovação nasce de novo no destino. **Gatilho de autenticação** (login caiu) **não** entra nesta etapa: só cota,
limite e crédito (e o limiar de 90%).

**Guardas.** A prova da reserva — vínculos (conta ligada e comprovada, escopo do dono, bot vivo, conta na ordem do Space **e**
vinculada a ele) e o **próprio CLI** confirmando a identidade no perfil isolado — roda a cada troca, **de novo imediatamente antes do
envio** (`send_admitted`) e uma última vez **dentro da transação do permit** (`admissao::dispatch` e, na retomada da espera por limite, `admissao::resume`): revogar entre a prova e o
despacho recusa ali, sem recibo e com a mensagem ainda na fila. Sem prova nada é enviado e o destino fica **bloqueado por 1 h**. Toda tentativa, de qualquer
conta, passa pelo **mesmo permit** e pelo mesmo [teto de gasto](teto-de-gasto.md) (reserva, ledger, "desconhecido", falha fechada);
a troca não contorna teto travado. Só assinatura → assinatura, e o fallback vale na conversa com o dono.

**Mission Board.** `donnaControlTower` ganhou `accountSwitches` (Observation, últimos 7 dias, até 20 cartões: conta de
origem, destino, motivo, hora e quando volta); seção "Trocas de conta", só informação. `MIRROR_SCHEMA` sobe para **52**: a linha de
troca é uma entrada de aviso com `Entry.data.contaTroca` e a falha de preparo ganha os códigos `quota_near_limit`, `quota_exhausted` e
`fallback_no_proof` em `reason`; o cache do schema 50 não tem nada disso (as chamadas `donnaEngineFallback*` em si são RPC fora do
mirror).
