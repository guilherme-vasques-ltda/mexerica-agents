# Delegação: regras escritas pelo dono

## O que o dono faz

- **Ajustes → Delegação** (grupo Pessoal, ao lado de Contas de IA): o bloco **Regras de
  delegação**, um texto livre que vale para os bots do Space selecionado. Por exemplo: "Para
  código, peça ao Rafa. Coordenação: Opus 5.5 high."
- **Equipes → Editar área**: o campo **Regras desta equipe**. Vale para o chefe e os bots da
  área e das subáreas, antes das regras gerais. Ele grava à parte, pela mesma RPC: salvar a
  regra não muda o quadro, e a área precisa estar salva no Hub antes.
- Cada campo mostra o contador (até 4.000 caracteres), **Salvo** / **Não salvo** e, se as
  regras mudaram em outro lugar, **Tentar de novo**, que relê a versão nova e grava o texto
  da pessoa. Um envio sem resposta é reenviado com a mesma chave, sem gravar duas vezes.
- A caixa **Limite** lembra que as regras são orientação.
- Acima do campo: **Como funciona** (quando um bot delega, por que e
  como se faz) e um **Exemplo de regras** com o botão **Copiar** (chaves
  `delegacao.como.*` e `delegacao.exemplo.*`, nos seis idiomas).

Regras por bot e o bloco "Como o Hub entendeu" ficam para uma etapa futura. A lista dura de modelos
("Modelos que os ajudantes podem usar") entra na mesma
página, acima das regras, quando existir.

## Como os bots recebem

- As regras entram **no início de cada turno**, num bloco `<regras_de_delegacao>` "Regras
  do dono para delegar", com a versão, como o bloco "Computadores do dono". Não entram no
  perfil guardado da sessão. Uma mudança vale **a partir do próximo turno** de cada bot.
- O texto do dono e o nome da área vão com `&`, `<` e `>` escapados (`&amp;`, `&lt;`,
  `&gt;`): não fecham o bloco nem imitam marcação; o título, os rótulos e o aviso são só do Hub.
- Quem recebe: o Berga, os chefes e os bots do Space com o MCP `team`. Ordem: primeiro as
  regras da área do bot e das áreas acima dela, da mais específica para a mais geral, depois
  as gerais. O Berga não tem área e recebe só as gerais. Um chefe sem área própria conta pela
  área que chefia.
- O bloco termina sempre com o aviso fixo do Hub: "As regras orientam a escolha. Elas não
  liberam modelo fora da lista aprovada nem permissão acima da sua."
- Sem nenhuma regra aplicável, nada é injetado.
- Bots de outros Spaces não recebem as regras do Pessoal; cada bot recebe apenas regras do seu Space. Execuções com identidade alheia não recebem regras.
  Turnos em **Canais** também não, mesmo de um membro do Pessoal: a decisão é pela conversa
  do turno, não pelo tipo do bot.
- Área apagada no quadro: a regra dela é **ignorada na leitura** (não aparece no app nem nas
  instruções), mas a linha **continua no banco**. Se um Desfazer restaurar a área com o mesmo
  ID, a regra volta a valer junto com ela. Uma área criada de novo tem outro ID e começa sem
  regra.

## Recibo

Quando um bot usa `ask_bot` ou `message_bot` com regras em vigor, o aviso dele na conversa
(o "Asked …" / "Messaged …") guarda `delegationRulesVersion`. O recibo do turno ("Como esta
resposta foi feita") mostra **Regras de delegação v N**. É a versão no Hub no momento do
pedido. Pedido feito num turno de Canal não marca versão. Campo novo no espelho: `MIRROR_SCHEMA` 20.

## Contrato da tarefa

Quem delega por `ask_bot`/`message_bot` (o Berga, um chefe ou qualquer bot) pode levar um
contrato curto, proposto pelo próprio bot. O dono não aprova nem escreve nada.

- Campo opcional `contract` nas duas ferramentas: `result` (resultado esperado; obrigatório
  quando há contrato; até 500 caracteres), `acceptance` (como conferir; até 1.000), `scope`
  (onde pode mexer; até 500), `limits` (prazo/tentativas em texto; até 300) e `rule` (regra
  seguida; até 300). Campo desconhecido, texto que não é texto, caractere invisível ou de
  controle e texto acima do limite são recusados antes da admissão (`contract_invalid`,
  `contract_limit`), sem pedido nem ID. Campo vazio é ignorado.
- **Orientação, não permissão.** O contrato não amplia pasta, ferramenta, modelo, conta nem
  teto: permissões, a intenção de execução e as regras de audiência de `ask_bot`/`message_bot` continuam como
  estão. O destinatário não muda o contrato.
- **Onde fica:** em `botMessage.contract` dos dois avisos do pedido (conversa de quem pede e
  de quem recebe), onde já fica o texto do pedido. O registro durável de Retornos não muda. A
  intenção de execução cobre a mensagem e o contrato estruturado (`("team", ferramenta, de, para,
  mensagem, contrato)`); sem contrato, é exatamente a tupla e o digest de antes.
- **Como chega:** depois do pedido, num bloco `<contrato_da_tarefa>` montado pelo Hub, cada
  campo no seu `<campo rotulo="…">` (rótulos em português, só do Hub), o texto escapado (`&`,
  `<`, `>`), de modo que um valor não imita outro campo, e o aviso fixo: orienta, não libera
  permissão nem muda o teto; não altere os critérios para passar.
- **O que o dono vê:** no cartão do pedido (a conversa entre os dois bots), sob o pedido,
  "Com contrato", que abre os campos. Recibo do turno e Mission Board não mudam.
- Módulo: `host/src/fork/delegacao/contrato.rs`; app: `apps/desktop/src/renderer/views/fork/contrato/`.
  Campo novo no espelho: `MIRROR_SCHEMA` 22.
- Chaves de texto: `contrato.titulo`, `contrato.resultado`, `contrato.aceite`, `contrato.escopo`,
  `contrato.limites`, `contrato.regra`, `contrato.nota`. O texto do contrato e o bloco do bot
  não são traduzidos.

## Contrato do Hub

Módulo próprio: `host/src/fork/delegacao/`.

- `donnaDelegationRules` `{spaceId?}` → `{spaceId, version, limit, general, teams: {areaId: texto}}`.
- `donnaDelegationRulesSave` `{spaceId?, teamId: string|null, text, expectedVersion, clientRequestId}`
  → `{spaceId, snapshot, receipt: {clientRequestId, version}}`.
- Autoridade: Local ou aparelho com Control e autoridade original do transporte, capturada
  (`Autoridade::capturar`) antes de esperar a trava do quadro e reconferida na mesma
  transação da gravação (`with_db`): aparelho revogado durante a espera não grava nada. `spaceId`
  seleciona o Space; ausente usa o Pessoal, `null` é recusado (`invalid_space`), e Space inexistente retorna `space_not_found`.
- Texto aparado nas pontas, quebras de linha `\n`; outros caracteres de controle ou
  invisíveis são recusados (`delegation_invalid`, a regra da função do bot); mais de 4.000
  caracteres, `delegation_limit`. Texto vazio apaga a regra.
- Versão por Space: avança 1 a cada mudança real; gravar o mesmo texto não avança.
  `expectedVersion` diferente: `delegation_conflict`. Mesmo `clientRequestId` com o mesmo
  pedido devolve o mesmo recibo; com outro pedido, `request_conflict`.
- `teamId` precisa existir no quadro do Space (`delegation_team`). A gravação usa a trava
  do quadro de Equipes.
- Nenhuma ferramenta, MCP, rotina ou webhook lê o texto cru ou o altera. O bot só o recebe
  nas instruções.

## Textos

No formato da base de idiomas (`useT`, catálogos em `apps/desktop/src/renderer/lib/fork-i18n/`), com as chaves
nos seis idiomas (pt-BR, en, es, fr, zh-CN, ar-AE). A página e o campo declaram `superficieIdioma` (lang/dir) e a
classe `idioma-superficie`; o texto do dono usa `dir="auto"`. O modelo (`delegacao/regras.ts`) só devolve chaves;
a view traduz. Não são traduzidos o texto do dono, o bloco que vai aos bots, o aviso fixo do Hub e o manual do
harness: esses ficam em português.

Chaves: `delegacao.pagina`, `delegacao.pagina.lead`, `delegacao.regras.titulo`, `delegacao.regras.ajuda`, `delegacao.regras.exemplo`, `delegacao.equipe.titulo`, `delegacao.equipe.ajuda`, `delegacao.equipe.exemplo`, `delegacao.equipe.antesDeSalvar`, `delegacao.estado.salvo`, `delegacao.estado.vazio`, `delegacao.estado.naoSalvo`, `delegacao.estado.salvando`, `delegacao.estado.lendo`, `delegacao.estado.conflito`, `delegacao.acao.salvar`, `delegacao.acao.tentarDeNovo`, `delegacao.falha.envio`, `delegacao.falha.leitura`, `delegacao.falha.limite`, `delegacao.falha.invalido`, `delegacao.falha.equipeSumiu`, `delegacao.falha.soDono`, `delegacao.falha.hubAntigo`, `delegacao.semConexao`, `delegacao.limite.titulo`, `delegacao.limite.texto`, `delegacao.contador` (`{n}`, `{max}`), `delegacao.recibo` (`{versao}`), `delegacao.recibo.rotulo`.

## Contrato por Space

Regras, versões e recibos são separados por Space; os dados existentes permanecem no Pessoal.
A gravação confere a versão, a existência do Space e a equipe na mesma transação. Sem `spaceId`,
a RPC usa o Pessoal. A capacidade `spaceDelegationContract` ainda não é anunciada: sua publicação
nos dois `hello` e a entrega de eventos ao stream com a autoridade existente ainda não estão disponíveis.
A integração do cliente para limitar a RPC legada ao Pessoal ainda está pendente.
