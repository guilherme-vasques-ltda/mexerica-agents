# Equipes da Mexerica

Na tela e na barra o nome é **Equipes**. Times continua só
como nome técnico: RPCs `donnaTimes*`, evento `times`, arquivos e este documento.

O dono organiza os bots existentes em áreas e subáreas, subbots estruturais,
gerentes, Reporta a e contato. A relação administrativa não concede `manageTeam`
nem muda modelos, contas, memória, prompts, pastas ou sessões. A hierarquia de reportes de cada bot continua só pela
própria cadeia (um nível acima e um abaixo) por `my_team_chain`, no MCP `team`;
Reporta a não concede poder. `list_bots` mostra a equipe de cada bot visível e quem
é o chefe; use para responder sobre as equipes. O restante do grafo continua só do dono. As rotas são **propostas**;
`ask_bot`, `message_bot` e notificações conservam suas regras atuais; elas incluem
o limite por cadeia (5 níveis, 50 execuções),
o registro durável de pedidos e conclusões só com metadados, a
consulta só leitura `bot_request_status` do estado do próprio pedido na mesma
conversa do Pessoal e o `requestId` no erro de `ask_bot` depois da
admissão, descritos em [colaboração entre bots](bot-collaboration.md).
O registro e a consulta não entregam nem devolvem resultados após reinício. O quadro não cria cadeia, orçamento ou retorno próprio.
O Mission Board mostra ao dono, no Pessoal, os pedidos entre bots
dos últimos 7 dias que não terminaram bem, só com Abrir ([Mission Board](mission-board.md#pedidos-entre-bots)).

Esta etapa não executa encaminhamento, chat de equipe, Andamento, pasta compartilhada
ou política de acesso privado. Zoom, trava e disposição local não são permissões
nem pausa. A principal vem exclusivamente do ID configurado no Hub, sem escolha
por nome; fora do Space atual, `primaryBotId` é nulo.

## Organizar pelo app

No menu da engrenagem, abra **Equipes**. A tela usa os bots do Space aberto;
**Nova equipe** cria uma área com nome e gerente entre os bots que já existem.
Você pode organizar subáreas e bots abaixo de bots. **Mover…** muda o lugar;
**Reporta a…** registra a quem irão relatórios novos sem um pedido anterior
quando os pedidos pelas Equipes existirem; nesta versão nada é encaminhado pelo quadro.
O resultado de um pedido continua pertencendo a quem o fez.

Ao organizar, o padrão do Reporta a é o nível de cima: **Mover…** e a criação de
subárea já trazem marcada a caixa para reportar ao bot de cima ou ao gerente da
área de cima (ex.: Marisa → Angela → Ana → Você). Editar uma área só oferece a
caixa se o gerente ou a área de cima mudar. Desmarque para manter o Reporta a
atual. A oferta fica desligada, com o motivo, quando o destino não é gerente, é o
próprio bot ou fecharia um círculo; a principal continua reportando a Você. Nada
muda no que já está salvo até você confirmar e **Salvar**.

**Membro único vira o chefe.** Uma área sem chefe informado com
exatamente **um bot membro direto** (`member.teamId == node.id`) tem esse bot como
chefe: o Hub grava `managerBotId` e papel Gerente na prévia e no Salvar, e a
prévia do app já mostra assim. Subáreas e os bots delas não contam como membros da
área de cima. Se entrar um 2º bot, o chefe continua. Quando o chefe é o único bot
membro e sai, a equipe fica sem chefe, tenha ele sido escolhido ou automático; o
papel dele não muda. Um chefe de fora que nunca foi membro não é tocado; equipe com
chefe escolhido e membros não muda. Área vazia ou com 2+ bots sem chefe não ganha
chefe, e o Berga principal nunca é promovido. No Salvar e na prévia o Reporta a não muda sozinho: continua sendo a
oferta marcada do app, editável. Uma migração única aplica a regra às equipes que
já existiam, de cima para baixo; o chefe promovido que reportava a Você passa a
reportar ao chefe da área de cima, se houver e sem círculo (ex.: Youtube só com o
Joio e a subequipe Roteiros só com o Crivo → Joio chefe da Youtube, Crivo chefe
da Roteiros reportando ao Joio). A migração avança a revisão como um Salvar e não
muda nada na segunda vez.

### Modelo Equipe Marketing

**Nova equipe ▾ → Modelo: Marketing…** (ou **Usar modelo Marketing** no quadro vazio)
monta a equipe padrão da biblioteca: **Hype** como chefe e **Feed, Prosa, Rank, Canal,
Traço e Pixel** como membros. A janela lista as sete funções antes de qualquer gravação:
- **Já existe**: um bot de **Sem equipe** com aquela função (o campo Função do bot, por
  exemplo "Redes sociais" para o Feed). Com mais de um, você escolhe qual.
- **Criar agora**: nenhum bot de Sem equipe tem a função. Um bot da função que já está
  em outra área aparece como "está em X" e não é tirado de lá; um bot com subbots
  aparece como "tem subbots" (eles iriam junto sem estar na lista). Nos dois casos o
  modelo cria outro.
  O Berga principal e os bots escondidos não entram.

Nada acontece antes de **Criar N bots e conferir equipe**; **Cancelar** não cria nem
grava. Ao confirmar, os bots que faltam nascem pelo mesmo caminho do
[Novo bot](criar-bot.md): nome, função e aparência do preset, motor e modelo padrão,
autonomia **Pedir** e conectores e contas desligados, sem skill escolhida. As
regras comuns da equipe (falar com o dono só pela resposta final, não inventar número,
"pronto" só com a entrega feita, não descrever arquivo que não abriu, tentar outro jeito
uma vez e depois dizer o erro) vão nas instruções **só** desses bots novos; bots que já
existem não mudam. Depois, a equipe entra no quadro como qualquer mudança: área
**Marketing** de primeiro nível com o Hype como chefe (`managerBotId`, papel Gerente),
e os seis membros na área com **Reporta a** Hype. É uma prévia: só vale depois de
**Salvar**. O chefe não ganha gestão, aprovação, conta, pasta, memória, ferramenta nem
delegação automática.

Logo antes da prévia, o app relê o quadro e confere de novo cada escolha. Se um bot
saiu de Sem equipe ou surgiu uma área Marketing, o modelo para sem tocar no quadro e diz
o que mudou, para você confirmar de novo. Se uma criação ou a releitura falhar, nada
vai ao quadro e a janela diz quais bots já foram criados. Eles ficam em Sem equipe
(nada é apagado), e **Tentar de novo** os aproveita sem criá-los outra vez. A execução
pertence ao quadro, não à janela: fechar e reabrir durante **Criando…** mostra o
andamento e não começa outra. Uma área **Marketing** de primeiro nível que já
exista bloqueia o modelo, sem alterá-la; mudanças ainda em conferência também
bloqueiam (salve ou descarte antes). Nesta versão o modelo só aparece no Pessoal.

O Quadro permite ajustar o zoom, mover cartões e travar a disposição. A Lista
oferece as mesmas escolhas pelo teclado. A disposição fica neste aparelho;
as relações só ficam guardadas no Hub depois de **Salvar**. Durante a edição,
o app confere as mudanças e indica que ainda não estão salvas.

Se outro aparelho alterar o quadro ou o cadastro dos bots, seu rascunho continua
na tela para revisão. Se não houver resposta ao salvar, **Tentar de novo** repete
a mesma intenção sem gravar duas vezes. Sem conexão, o quadro fica só para leitura.

### Equipe na criação de bot

O [Novo bot](criar-bot.md#equipe-em-dois-passos) também escolhe equipe e chefe. Usa o mesmo caminho
do modelo: `createBot` e depois prévia e gravação do quadro, com uma mudança só (o bot novo entra no topo da equipe
e reporta ao chefe). Falha no segundo passo deixa o bot criado, em Sem equipe, com aviso e Tentar de novo.

## Equipes na barra lateral

Na seção **Equipes**, a sidebar do Pessoal por computador mostra as equipes salvas, com subáreas e caminho; bots sem equipe ficam em **Agentes**. O Berga principal fica no cartão próprio; canais conservam sua ordem, pin, seleção, menu, teclado e escrita de ordem no Hub. A sidebar não acompanha o rascunho do quadro.

`member.teamId` determina pertencimento. Chefe é somente `node.managerBotId`. A sidebar não promove ninguém; o chefe do membro único é gravado pelo Hub e chega salvo. Equipe com chefe gravado sempre aparece pela linha do chefe: chefe que é membro visível da equipe é a linha dela, primeiro, antes de fixados, mesmo chefiando outras equipes. Chefe visível que é membro de outra equipe (ou de Agentes) vira uma **linha de referência** no topo, mais leve: rosto de 24px, nome, etiqueta da equipe e "Chefe · em Youtube". O clique abre a conversa dele e dois cliques abrem ou fecham a equipe. A referência não é membro: fica fora da contagem, da ordem local, do arraste, de ⌥↑/↓ e dos resultados da busca, e não duplica a linha normal do bot. Como a linha de qualquer chefe, ⌥↑/↓ nela move a equipe entre as irmãs. Na sala da equipe, a mesma referência aparece no topo; lá, ⌥↑/↓ nela não reordena nada. Ex.: Youtube (Joio membro e chefe) › Roteiros (só o Crivo; chefe Joio) mostra o Joio em Youtube e a referência do Joio em Roteiros. Chefe oculto ou no cartão do Berga continua **Confira o chefe no quadro**, sem realocar, duplicar ou revelar nome oculto. O DTO administrativo não acrescenta bots à lista autorizada pela store.

Recolher e ordenar equipes/membros são preferências locais por contexto, computador, Hub, Space e IDs; renomear não perde identidade. Arraste e ⌥↑/↓ só reordenam irmãos no mesmo domínio, respeitando Chefe e pin. O menu oferece alternativa ao arraste. Nenhuma dessas ações salva o organograma ou altera associação. O menu **Organizar no quadro** abre o Pessoal daquele computador, inclusive quando outro Space está aberto; abrir não grava.

Para mover um membro pela barra, use ⌥↑/↓ ou **Mover para cima/baixo** no menu. Essas ações pulam filhos e bots de outros pais até encontrar o próximo irmão visível do mesmo Time e computador. Bots fixados só trocam de lugar com outros fixados, e os demais só entre si. O Chefe permanece no lugar; sem um irmão disponível, nada muda. Só a ordem neste aparelho muda, sem alterar o quadro.

**Buscar bots** filtra por nome sem acento. **Todas as equipes**, **Agentes** e os nomes das equipes substituem o seletor de seção somente na barra. Busca/filtro revela resultados sem gravar recolhimento; setas seguem as linhas desenhadas, com cabeçalhos acessíveis, menus por teclado e nomes/funções completos na dica/leitor. O atalho tem piso de 11px, ⋯ aparece também no foco e tag longa não vira fragmento de uma letra.

Na primeira leitura aparece **Abrindo Equipes…** e lista simples. Sem snapshot válido ou em Hub antigo, não se inventa Sem equipe nem se volta às seções antigas. Bot autorizado ainda ausente dos membros do snapshot fica em lista simples aguardando atualização; ausência não prova vínculo nulo. Uma leitura válida vazia permite Sem time para vínculos conhecidos nulos. Offline conserva somente a última organização da mesma origem em RAM, com aviso. Erro transitório conserva leitura anterior com aviso/Tentar de novo; recusa de dono, logout, retire ou troca de origem descarta conteúdo e respostas pendentes. Reconexão força releitura.

A leitura usa donnaTimes e o stream existente, sem polling permanente, SSE adicional ou cache espelhado. Respostas validam origem, cliente, geração, sequência e epoch; catálogo novo é aceito mesmo com a mesma revisão Times. Preferências e dados de seções antigas são preservados sem conversão. Rotinas continua filtrando essas seções legadas. Esta etapa não entrega delegação, conversa do Time, Retornos, Chefe executor nem autoridade adicional.

## API administrativa v1

As três RPCs usam a autoridade do editor: transporte Local ou Device com Control.
`spaceId` só seleciona contexto; omissão seleciona o Pessoal canônico. O Hub
recusa Space inexistente, bot de outro Space, canal e bot apagado. A origem
autenticada nunca vem de campos JSON. Não há concessão extra nem endpoint MCP.

- `donnaTimes({spaceId?})` retorna o snapshot canônico.
- `donnaTimesPreview({spaceId?,expectedRevision,catalogVersion,nodes,members,routes})`
  valida o estado final e retorna o snapshot proposto com `preview:true`, sem
  gravar revisão, nonce ou recibo. `clientRequestId`, mesmo nulo, é recusado.
- `donnaTimesSave` recebe os mesmos campos mais `clientRequestId` UUID canônico
  e retorna `{snapshot,receipt:{clientRequestId,revision}}`.

Snapshot: `{version:1,spaceId,revision,catalogVersion,primaryBotId,bots,nodes,members,routes}`.
`bots` contém somente `id`, `name`, `spaceId`, `title`, `avatarColor`, `avatarShape`,
`avatarConfig` canônico quando presente e `hidden`. Não inclui configurações de
motor/conta, memória, prompt, pasta, sessão ou estado de trabalho.

Cada área tem `{id,name,parentId,managerBotId,position}`. Cada membro tem
`{botId,teamId,parentBotId,role,reportsTo,ownerContact}`. `role` é `manager` ou
`executor`; `reportsTo` é `{kind:"owner"}` ou `{kind:"bot",botId}`; `ownerContact`
é `reports`, `urgent_only` ou `on_request`. Cada rota tem
`{fromBotId,toBotId,state:"proposed"}`. Campos desconhecidos são recusados em
todos os objetos de entrada, inclusive variantes de `reportsTo`.

O grafo enviado deve conter exatamente uma associação para **cada bot elegível**
do Space, inclusive ocultos. Ausência de área/subbot é `null`, não exclusão do
bot. Antes de validar, o Hub aplica ao estado final a regra do membro único:
área sem `managerBotId` com exatamente um membro direto (não principal) recebe esse
bot como `managerBotId` e `role:"manager"`; chefe que era membro e saiu da área,
deixando-a vazia, vira `null`. A regra é determinística, e o digest do recibo
continua sendo o do pedido: o replay da mesma chave devolve o recibo original.
Cada gerente de área e destino de reporte deve ter papel `manager` no estado
final. Isso não exige nem altera o poder legado de gestão; gerentes podem executar
e um bot pode gerenciar mais de uma área. As relações de área, subbot e reporte
são distintas; mover uma relação preserva as demais, salvo intenção explícita.
As rotas propostas podem ser bidirecionais; nunca são consumidas pelo runtime.

O Hub recusa ciclos de áreas, de subbots, de reporte e da composição estrutural
com reporte. Todos os caminhos têm até oito níveis, contando áreas e bots na
composição. Reporta a Você exige `ownerContact:"reports"`; `on_request` exige
gerente destinatário elegível. Remover área ou rebaixar gerente exige realocação
explícita de suas referências no mesmo grafo final. Excluir bot por caminho legado
também recusa enquanto houver área gerenciada, subbot, reporte ou rota dependente;
a orientação é resolver essas relações em Equipes antes de excluir. Não há substituto
automático, remoção de chat ou alteração de pasta por ação do quadro.

## Concorrência, principal e recuperação

`revision` protege o grafo por CAS. Alterações reais recebem `next_rev` da conexão
existente; `catalogVersion` é SHA-256 canônico do DTO elegível ordenado e da
principal relevante. Mudança de identidade principal, rosto, nome, título,
ocultação ou roster exige releitura e revisão. A fonte é lida dentro da mesma
transação que valida e grava, sob `team_lock` conservado pelo worker bloqueante.
As três RPCs conservam a autoridade original antes de esperar `team_lock` e a reconferem na guarda Cloud → DB antes do commit; revogação ou prazo encerrado impedem gravação e recibo.
Não há callback que reentre em Store/Cloud, await ou I/O externo dentro do SQL.

A principal atual aparece como raiz protegida: sem área/pai estrutural, gerente,
Reporta a Você e contato Relatórios. Essa projeção permite abrir o quadro após
troca de principal sem alterar as relações persistidas durante a leitura. Os
reportes dos outros bots são preservados; o dono revisa e confirma explicitamente
o estado final. Um save pode materializar essa confirmação, mesmo quando coincide
com a projeção legível. O editor de perfil continua independente do organograma.
Novos bots aparecem sem área/pai e reportando ao dono; principal é gerente, demais
bots começam executores. Nenhum cadastro de bot é modificado pelo quadro de Equipes.
Com o Berga fixo, o principal não troca nem é apagado enquanto existir, e nenhuma
gravação o coloca abaixo de outro bot ([contrato](donna-fixa.md)).

Grafo e recibo são gravados atomicamente em tabelas próprias normalizadas
`donna_team_nodes`, `donna_bot_org`, `donna_team_routes`, `donna_team_revisions` e
`donna_team_receipts`. A migração é idempotente e não importa árvores preparatórias.
Um save sem mudança conserva a revisão e só registra a intenção. Recibos não
expiram nesta versão: a mesma chave e conteúdo canônico retornam o recibo original
**antes do CAS**, com o snapshot atual, sem reaplicar após resposta perdida.
Reordenação dos arrays não muda o conteúdo canônico. Alterar revisão esperada,
catálogo ou grafo reutilizando a chave é conflito; uma nova intenção usa outro UUID.
O recibo registra a revisão do commit original e pode ser anterior à do snapshot.

Falha de validação ou SQL reverte grafo, revisão e recibo. Depois de commit com
mudança, o Hub emite somente `{type:"times",spaceId,revision}`, sem snapshot ou
`rev` global: o aviso não avança o cursor de conversas dos clientes.
A integração filtra esse aviso por Space exato e dono Local/Control.
Não há campos novos em hello/bot/entrada espelhados nem mudança de `MIRROR_SCHEMA`.
Após reconexão ou mudança no cadastro de bots, releia a RPC; o evento não
substitui snapshot nem recibo.

Limites v1: 128 áreas, 512 bots e 2048 rotas por Space; corpo de até 512 KiB;
IDs legados de até 128 bytes, sem espaços/controles; UUIDs canônicos minúsculos
não nulos para áreas e intenções; nomes de área de 1 a 80 caracteres, sem controles
ou espaços nas pontas; posição inteira entre 0 e 1.000.000. IDs de área e nonce
são emitidos no app e conferidos pelo Hub. Chaves de recibo são únicas por Space.

Conflitos usam mensagens legíveis: `times_conflict`, `catalog_conflict`,
`request_conflict`; outras recusas incluem `bot_catalog_conflict`,
`manager_unavailable`, `primary_protected`, `times_dependents`, `times_depth` e
códigos de ciclo. Falha preserva o rascunho no app; após catálogo/revisão vencidos,
reler, revisar e usar uma nova intenção. Prévia nunca é confirmação.

## Alcance desta etapa

O quadro administrativo é a primeira parte do plano de Times. Esta etapa não
ativa os pedidos duráveis nem a consulta privada do Berga ao Bem-estar.


O controle interno das tentativas de Rotinas acrescenta
identidade persistida e motivo de
interrupção. Não ativa rotas do quadro, retorno entre bots, parada por execução
ou integração MCP nova. Veja [Rotinas](routines.md#tentativas-duráveis-de-rotinas)
para a recuperação, a migração conservadora e as lacunas de cobertura.

A cadeia própria dá ao
bot a ferramenta `my_team_chain`, sem argumentos e só leitura, no MCP `team`. Ela
mostra a quem o bot reporta (`owner` com o rótulo "Você (o dono)", sem e-mail ou
nome de conta; um bot visível com id e nome; ou `hidden`), os bots visíveis que
reportam direto a ele (ocultos só contam em `hiddenDirectReports`), a função e o
nome da própria área. Usa a mesma leitura e projeção do quadro: a principal e um
bot ainda sem registro aparecem reportando ao dono. Só o Space do próprio bot;
nada de outras áreas, rotas, avô/neto, configuração, conta, memória, pasta ou
sessão. Não grava nem muda a revisão. Reporta a não dá poder: pedidos seguem
`ask_bot`/`message_bot` e o limite por cadeia.

O Hub mantém, só dentro dele, o
escopo pessoal positivo e o ponto interno de leitura Agenda →
visão privada que a Agenda e o Foco vão consumir. Não há Agenda conectada,
escrita, RPC nem tela nesta etapa.

O Mission Board usa o quadro para o
**resumo do dia** no Mission Board ("O que aconteceu"). Ele só lê: o quadro não muda,
nem permissões, nem rotas. Cada bot do Pessoal que trabalhou no dia escreve uma linha,
de baixo para cima pelo Reporta a: quem reporta a Você 15 min antes do horário
escolhido (padrão 19:00), cada nível abaixo mais 15 min antes. O gerente da área
(`managerBotId`) escreve também a linha do Time. Se ele não pode (Codex, chave de API,
outro motor, sem conta ou cota), o Hub junta as linhas sem IA. Bot fora de Time que
reporta a outro bot entra na cadeia de cima; bot solto aparece em "Bots sem equipe".
Spaces compartilhados ficam fora nesta etapa.

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

Se houver um ciclo nos pais já persistidos, `list_bots` interrompe o caminho antes
de repetir uma equipe, sem reparar nem gravar o Quadro.

### Regras desta equipe

Na folha **Editar área**, o campo **Regras desta equipe** guarda um texto do dono para o
chefe e os bots da área e das subáreas. Ele entra nas instruções desses bots antes das
regras gerais de Ajustes → Delegação, da área mais específica para a mais geral. Grava à
parte, pela RPC `donnaDelegationRulesSave`, sem mudar a revisão do quadro; a área precisa
estar salva no Hub. Área apagada: a regra dela é ignorada, mas fica no banco e volta se um
Desfazer restaurar a área com o mesmo ID. Contrato completo em
[Delegação](delegacao.md).
