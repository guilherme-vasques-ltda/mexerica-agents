# fork-cloud — cliente da ponte no app do Mac

Transporte do acesso pela nuvem (contrato `donna-acesso/v1`, guia
Acesso pela nuvem). Processo principal, só
`node:crypto` e o WebSocket do Node; nenhuma dependência nova. `main/index.ts` já liga
a fábrica interna de callbacks ao serviço de conta quando `cloudAccess` é habilitada.
O módulo só pode ficar disponível na release Dev qualificada descrita abaixo;
main e builds sem configuração permanecem fechados, assim como o transporte legado.
O candidato inclui serviço Main e IPC de vínculo,
pins e conexão; sua composição, configuração e interface exigem validação própria.
Código não comprova Clerk configurado ou acesso pela nuvem instalado.

PR3 C1 candidato: `personal-protocol.ts` inspeciona M3/C canônicos e
`personal-authority.ts` seleciona somente o manifesto fixo do build, comparado
com o principal capturado e sua época atual. A projeção `personalBootstrap`
do serviço publica apenas `available:false` e o motivo `updateRequired` ou
`helperRequired`; não inicia rede, consentimento ou posse por essa observação.
Fixture de teste não fornece raiz ao runtime. Helper/origem/journal/canal e
paridade SAAS-04 ainda não estão entregues por C1. SSH permanece. Consulte o
checkpoint C1.

C1 passou nos focais do root. C2 candidato adiciona `personal-shapes.ts` e
`personal-wire.ts`: 30 formas fechadas, assinaturas e composição real dos
originais/T, POP, abertura ligada ao HTTP, cópias imutáveis e modos internos
current/committed/historical. História verificada não é autoridade atual.
Nenhum signer livre, raiz do renderer, IPC ou oferta PL1 foi acrescentado.
Main continua sem M3 ativo. Consumidor de captura/journal/canal é C3; paridade
e UI integrada são C4, ainda obrigatórios neste mesmo PR3. C2 não foi testado
na Cloud; o root executará os focais no fluxo autorizado.

Boas-vindas, menu da conta e Ajustes usam `showsAccountControls`, condicionado a
`cloudAccess`, para apresentar login, saída e reconciliação. O corte do transporte
legado `cloud=false` não esconde essas entradas no Dev qualificado e continua
sem habilitar o transporte legado. Mostrar uma ação não concede autoridade no Hub.

## Entrada obrigatória no aplicativo

`AccountEntryGate` só monta o conteúdo com a conta confirmada pelo Main; sem módulo ou configuração de conta, a entrada fica indisponível. Pareamento e resumos nativos seguem a mesma projeção, sem transformar login em grant do Hub. Consulte a [regra canônica de entrada](../../../../../docs/features/manual-do-harness.md#entrada-obrigatória-no-aplicativo).

| Arquivo | Papel |
|---|---|
| `principal.ts` | Captura interna do selo, época, resumo público fixado e namespace; JWT permanece no processo principal |
| `account-api.ts` | Operações enumeradas de conta, origem fixa e health comparativa; recusa troca de época e conserva `unknown` sem replay |
| `device-keys.ts` | Reuso da custódia cifrada por namespace; leitura não cria identidade e registro explícito não substitui uma chave legada |
| `pins.ts` | Health comparativa; leitura e reconciliação explícita do recibo cifrado de pin, com a mesma chave de aparelho; não concede grant atual |
| `enroll.ts` | Produtor fechado A1/A2; fixa chave/época/nonces, confere decisão ou código, publica recibo cifrado sem substituir outro pin |
| `connection.ts` | Ciclo de conexão por época/encarnação, pin válido, binding e chave de aparelho existentes |
| `service.ts` | Serviço retido no Main; registro, diretório, vínculo e conexão por intenções enumeradas, sem credenciais no renderer |
| `managed-poll.ts` | Ritmo da leitura de fundo do computador oferecido pela nuvem (health + managed-offer): 2 s só com o fluxo gerenciado avançando; fora dele 60 s→5 min com jitter; pausa com janela escondida/minimizada, tela bloqueada, suspensão ou 5 min ocioso; lê na hora ao voltar, ao agir ou ao entrar |
| `shared/fork-cloud.ts` | DTO de IPC e estados; dado de apresentação não concede autoridade |
| `crypto.ts` | Ed25519, X25519, HKDF-SHA256, ChaCha20-Poly1305, `Codync-Sig`, aperto de mão, quadros e código de 6 dígitos, conferidos pelos vetores compartilhados |
| `channel.ts` | Canal E2E depois do aperto de mão: RPC interno, assinaturas, limites (32 requests, 4 assinaturas, 1 MiB de saída, 16 MiB de entrada) e resultado `ok`/`err`/`unknown`/`notSent` |
| `relay-socket.ts` | Socket de saída `wss://…/v1/relay/device/<computerId>?v=1`: presença, `hello` a cada `online:true`, ping exato, silêncio de 90 s, recuo 1 s→30 s com jitter e política de códigos de fechamento |
| `fake-hub.ts` | Somente testes: lado do Hub/ponte em memória |

## Regras que o código garante

- A chave do Hub é a **pinada** no vínculo; `welcome` assinado por outra chave (ou malformado) para
  a conexão como `hubUnverified` ("Não foi possível verificar a identidade do Hub"): pode ser outro
  Hub ou um caminho adulterado, e o módulo não afirma qual. Não tenta outro caminho para a mesma
  chave, não troca pin nem chave.
- Socket aberto, `presence` ou `health` não autorizam nada; só o canal autenticado entrega dados.
- Cada socket tem uma geração: mensagens e quedas de um socket antigo são ignoradas, e `stop()`
  descarta o canal antes de qualquer coisa nova. Um `RelayLink` parado não reabre: outra conta,
  outro vínculo ou nova tentativa da pessoa criam outro objeto (encarnação nova). Isso também vale
  quando `onState` ou `onChannel` chama `stop()` sincronamente: ao voltar do callback, a geração e
  o estado terminal são conferidos antes de instalar socket, timer ou entregar canal não nulo.
- Chamada enviada sem resposta (queda, revogação, prazo) vira `unknown` e **não é repetida**; o
  prazo manda `cancel`, que no Hub só descarta a resposta (§6.5).
- Um `reject` pode chegar **antes** do `welcome` ou depois de o canal ficar pronto; a mensagem
  não autentica o Hub. Antes do `welcome`, `unauthorized`, `badSignature` e
  `pairingClosed` param como `handshakeRefused` ("O Hub recusou o aperto de mão deste Mac"), com o
  código em `rejectCode`; `badSignature` só diz que a assinatura do aparelho no `hello` não conferiu
  do lado de lá, sem provar Hub trocado. `revoked` para como `revoked`; `unsupportedVersion` como
  `updateRequired`. Após `ready`, `unauthorized`/`badSignature`/`pairingClosed` encerram como
  `unauthorized`, sem atribuir falha de identidade. Fechamentos `4001`, `4003`, `4410` e `4400` sem rejeição já classificada também
  param. `leaseExpired` e `rateLimited` mantêm recuo inclusive após `ready`: descartam o canal,
  pendentes ficam `unknown` e o fechamento da geração antiga não encerra a nova tentativa. Quedas
  comuns também repetem com recuo; `4011` reconecta na hora. Código desconhecido não dá autoridade.
- Falha do socket antes de abrir (o DO recusa com HTTP e o WebSocket não mostra o status) aparece em
  `connectFailures`; quem usa o módulo confere o acesso pela conta antes de insistir.

Custódia cifrada por namespace, captura de vínculo, logout e reconciliação dos recibos
por conta/ponte/Hub estão ligados internamente no candidato. Continuam pendentes a
configuração real, interface e provas do agregado integrado. Não há confirmação
de revogação global por sucesso no D1 ou socket fechado isoladamente. O guia acima
distingue o código candidato das provas de execução. `forkFeatures.cloud=false`; `cloudAccess`
continua falso em builds sem configuração e na variante main. Somente a etapa electron-vite
de uma release Dev com configuração pública e M2 completos, correspondentes e com hash
conferido fixa a disponibilidade do módulo. A conferência não cria P, autenticação ou autoridade.
O caminho legado exige `cloud && !cloudAccess` em todas as
entradas com efeito, antes de IPC, assinatura, preferência ou mudança no Hub. A conta nova
usa `cloudAccess`, mas sua ativação depende da composição e das provas próprias; não basta
alterar uma flag. Sair cerca imediatamente apresentação, época, JWT e selo. Uma saída
pedida durante restore fica marcada no mesmo guard persistente e não pode ser restaurada
como sessão ativa depois de reabrir o app; falha conserva resultado incerto para reconciliação.

O resumo de build exige `deploymentEnv` próprio, comparado com `environment`; ausência ou divergência mantém o cliente indisponível. A forma dos pins não comprova a aplicação Donna nem uma implantação real. A factory já valida registro, persiste o vínculo e publica uma captura única; a aposentadoria exige a guarda específica da saída e seus recibos. A publicação de pin só é emitida pelo produtor fechado de Enrollment; ela não nasce de JSON, health ou resposta bruta do diretório. Não usar resposta bruta de registro, listagem ou health como autorização ou vínculo confirmado.

`AccountApi` retém a mesma captura de signer durante a vida do cliente; o commit de `Enrollment` usa a chave pública copiada dessa captura. Falha não recarrega outra geração e uma captura existente não passa a registro no mesmo cliente. `Enrollment` aceita A1 somente pela captura Local do gerenciador SSH possuído pelo Main, com conta/tentativa atuais e comparação da própria chave, SAS e transcript antes do expect exato. A2 exige código digitado após comparação com o canal do dono e approved da mesma tentativa; nenhuma confirmação é automática. Sem canal de dono, continua esperando. A captura SSH é uma origem Local disponível, não dependência global do produto ou autorização do primeiro dono remoto (ACS4).

`register.ts` liga os callbacks internos ao AccountService único, sem rede no construtor, IPC genérico ou registro automático. A reconciliação depois de reinício tem verificador somente de GETs, com JWT e guarda capturados da saída, health comparativa e aparelho/chave/recibos conferidos. Somente essa validação produz a captura privada usada no retirement; mudar época ou a guarda impede seu uso. Campo ausente/pending não é prova, nem uma ACL vazia. Uma lista explícita completa de zero cercas para o aparelho explicitamente revogado é o caso sem grants do contrato v3. Os produtores C1c/1e estão compostos no candidato; implantação e provas reais continuam pendentes. Nenhum callback confere prova Local/humana ou pin de Hub por login.

## Testes

```bash
npm test   # inclui src/main/fork-cloud/*.test.ts
```

Ponta a ponta com o Hub real: `cloud/test/e2e/donna-acesso.e2e.ts`.

## Selo de conta

A factory interna `account-relay.ts` captura o selo no processo principal e verifica época antes de iniciar o transporte. A mudança de época encerra somente o relay capturado e suas chamadas em voo ficam `unknown`; a mesma instância não reabre para outra sessão. O serviço Main integrado conserva essa factory e expõe intenções enumeradas pelo IPC próprio; o login continua sem conceder autorização no Hub. Sem a release Dev qualificada, o módulo permanece indisponível no build. A factory de conta não emite pins: a associação é feita pelo produtor Enrollment e pela captura Local qualificada.


## Pin e resultado incerto

`linked` significa recibo de vínculo gravado; `ready` exige o canal E2E com o Hub e sua autorização atual. O recibo v2 cifra namespace de implantação/titular, identidade do Hub, chave de aparelho, pedido/grant e elementos da cerimônia. A assinatura local usa domínio próprio e sela o recibo histórico; sua autoridade vem do produtor fechado e não da assinatura isolada. O transcript A1 é capturado no Local: a encarnação do Hub não é exposta e não pode ser inventada ou substituída pelo attemptId do app. A2 não fabrica transcript do Hub.

A leitura confere derivação da identidade, commit/SAS, assinatura, chave de aparelho existente e namespace. Nunca cria chave, promove formato legado ou troca outro pin. Isso fixa identidade histórica, não acesso atual; o handshake/ACL do Hub continua obrigatório. A publicação usa arquivo privado exclusivo, sync e fence `.pending`. Falha mantém resultado incerto e preserva os bytes anteriores; reconnect/restart não promove esse fence. `check` só reconcilia a publicação exata da tentativa em memória. Depois de reinício, a intenção explícita `reconcilePin` verifica o mesmo recibo cifrado completo e sua chave existente antes de sincronizar/finalizar a mesma publicação; não repete claim nem aprovação. Arquivo parcial, divergente ou legado permanece recusado.

O IPC aceita somente o frame principal de uma janela própria e origem esperada. Registro, descoberta, início Local/remoto, comparação, check, cancelamento, reconciliação de pin e conexão são intenções fechadas; não recebem JWT, origem, token Local, chave ou objeto de prova. Uma decisão Local enviada sem resposta não é repetida; um grant remoto sozinho não reconstrói esse aceite. O serviço não inicia rede no construtor e a ponte legada fica fechada quando o caminho novo é selecionado.

As bordas de saída e recuperação da conta preservam o envelope sem token e todas as referências antigas, capturam principal/credencial exatos em falha e serializam os caminhos de resumeFence, retain legado e fence de failStorage na mesma fila da saída. Essa lista não afirma que todos os escritores antigos foram serializados; a borda de saída durante limpeza de confirmações antigas permanece pendente antes da ativação. Verificar uma gravação não pode apagar a intenção de saída concorrente. Essas mudanças não declaram revogação global nem ativam uma sessão inexistente.


### Ponte de dados do app

O Main conserva um ID opaco novo por canal. Antes de publicar attachment/ready, executa hello dentro do canal cifrado e confere computerId/signKey contra o pin, além de hostId/nome; faz isso novamente após reconexão. A interface recebe somente identidade pública, contexto de cache por titular/implantação e esse ID, nunca chave privada, token, JWT ou URL de destino. A ponte de RPC do Host não recria o REST de conta: usa apenas a Connection corrente e preserva Caller::Device, ACL e escopos no Hub. Ações Local/owner.devices não são liberadas pelo transporte.

Chamadas preservam ok/err/unknown/notSent. O transporte do renderer trata unknown como erro não transitório para impedir que a recuperação comum reenvie uma mutação; não muda para Loopback. Eventos usam assinatura por frame/conexão, cancelamento pelo titular e descarte de retorno tardio. Troca de conta/canal/navegação cancela as assinaturas anteriores. O preload usa Web Crypto do contexto isolado, sem importar módulos Node indisponíveis no sandbox. Essa composição inclui a interface candidata e ainda exige gates finais, revisão independente e QA instalada; sem a release Dev qualificada, o módulo continua fechado.


### Tela de vínculo e conferência do registro

Com cloudAccess, Ajustes › Computadores › Pela nuvem apresenta registro explícito do aparelho, descoberta de Hubs, vínculo A1 por conexão Local verificada ou A2 por código digitado do canal do dono. A tela não preenche o código por conta própria, não registra/claim no login e não repete ações incertas. linked indica vínculo gravado; conectado exige attachment Main após hello e pin conferidos. O anexo reutiliza BotStore e cache/rascunhos do namespace da conta; Local/SSH têm prioridade e só a instância criada pelo anexo é removida. A troca de conta fecha a apresentação antes da leitura nova; eventos atrasados ficam cercados até uma resposta atual do Main.

Verificar registro é uma intenção explícita separada: lê a mesma chave existente, health comparativa e a lista de aparelhos da conta, exige uma única linha exata não revogada e só então persiste o binding e a captura de revogação. Nunca repete POST de registro, cria/troca chave ou infere falha por ausência de linha. Um registro pendente continua incerto se a lista for incompleta, duplicada, divergente ou revogada. Sem chave existente nem intenção pendente, a consulta informa que nada foi enviado e não impede registro explícito posterior. Essa consulta também pode recuperar o registro após reiniciar o app. Pendências são por implantação/titular, não contaminam outra conta, e voltar à mesma conta não remove a incerteza. A recuperação do pin é outra operação e não confirma claim ou decisão Local sem recibo.

A interface é candidata: configuração Donna/Clerk, implantação própria, primeiro dono remoto ACS4, revisão do agregado, gates e QA instalada continuam necessários. Nenhuma ferramenta de bot ou autoridade Local/dono é criada pela tela. Sua disponibilidade no Dev depende da admissão estática do build; main e builds sem configuração continuam fechados.


Na tela, Verificar registro aparece no cartão Registro não confirmado como única ação para o registro incerto. No cartão Registrar este Mac, fica como ação secundária para conferir um registro anterior depois de reabrir o app. Cada consulta depende de clique; só o Main pode publicar registrado. Uma recusa não troca o estado anterior, a lista, o anexo nem a recuperação. Sem registro anterior para conferir, o aviso informa que nada mudou e Registrar permanece disponível.


### Consultas, efeitos e recuperação

Enrollment conserva `pendingEffect` com id, Hub e fase somente quando um efeito pode ter
saído (request/reveal/approval/publication). Falha em GET preserva a fase estável, código e
prazo, com `readUnavailable`; notSent comprovado pela captura Local e recusas definitivas
não criam uma cerca de mutação. Cancelamento captura o efeito antes de invalidar a geração;
assim, cancelar GET não prende a conta, mas cancelar durante envio não apaga sua incerteza.
O Main mantém namespace/operação/fase e não permite reconciliar pin para ocultar claim,
pedido ou decisão sem confirmação. Nenhum efeito incerto é reenviado automaticamente.

O DTO de recuperação informa a fase sem credenciais. A tela oferece Verificar gravação
somente para publicação. O reader distingue `PIN_PUBLICATION_PENDING`; após reinício,
Connection recusa conectar e o Main publica essa recuperação para o mesmo Hub. O clique
valida o recibo completo e finaliza somente seus bytes, sem nova aprovação. As referências
Main de tentativas e conexões terminais são encerradas quando não há efeito pendente.


#### Conferir a posse que ainda está chegando ao Hub

Adicionar um Hub à conta pode ser confirmado pela nuvem antes de o Hub atualizar seu estado.
**Verificar posse** conserva a identidade da captura Local, aparelho e operação enviada e
faz somente leituras explícitas: confere a mesma conta, chave, conexão Local, estado do dono
e entrada do diretório. Não repete claim, assinatura, pedido de acesso ou aprovação. Se o
Hub ainda não confirmou, outra conta aparecer ou a leitura falhar, a cerca permanece.
Depois da conferência, Vincular é outro clique; verificar posse não cria um pedido oculto.
Esse contexto de conferência existe nesta execução do app, sem prometer recuperação durável
própria de claim após reinício. Uma criação de desafio cujo envio ficou desconhecido não é
confirmada por essa ação nem por uma consulta de pin. Reabrir apaga a cerca somente desta execução; confira ou encerre o pedido antigo no Hub antes de pedir outro. A preparação ACS4 de primeiro dono remoto é distinta deste fluxo Local A1.


Recuperar publicação interrompida não aceita um pin arbitrário: o Main conserva a operação,
o Hub, o identificador do recibo e SHA256 dos bytes cifrados ANTES de alterar arquivos. Se
pending já foi removido quando houve cancelamento/troca de época, só os mesmos bytes e
recibo, após nova validação e sync do arquivo/diretório, podem liberar a cerca. Bytes diferentes,
mesmo com recibo válido, permanecem recusados. Isso não repete pedido, claim ou aprovação.
Esse estado de arquivo é diferente das cercas sem recibo mantidas somente na memória.


### Primeira vinculação ACS4 no app — candidato integrado

O Main oferece somente as intenções fechadas `startBootstrap`, `checkBootstrap`, `proveBootstrap` e `reconcileBootstrap`. O início fixa identificador e nonce no journal cifrado antes do POST. A prova exige clique explícito, usa um canal bootstrap de uso único e grava Grant, desafio e prova originais antes de enviar. Troca de conta, aparelho, frame ou época fecha a captura. Após retorno incerto ou reinício, consulta-se apenas a mesma intenção; nenhuma prova, criação ou publicação perdida é repetida automaticamente. Verificar pedido só lê. Verificar e concluir é outra ação explícita: lê e valida a mesma intenção e cadeia e, somente se a nuvem ainda mostrar hubPending, pede a publicação CAS idempotente do mesmo recibo. Ela retorna a publicação existente se já aplicada; não repete criação, grant, assinatura, autorização ou prova. notSent e recusas pré-efeito permitem novamente a primeira publicação do recibo conservado; nenhuma leitura ou retomada envia por conta própria.

O journal conserva os originais, as fases incertas e os bytes cifrados exatos de publicação do pin. O pin3 tem domínio próprio de assinatura do aparelho, valida toda a cadeia e o recibo de ativação do Hub no instante de publicação; uma interrupção depois do unlink só pode concluir os mesmos bytes, com sync de arquivo e diretório. O leitor3 mantém a identidade histórica, enquanto lease/revogação/hello continuam sendo a autoridade atual. Pins A1/A2v2 não são promovidos por nome ou formato. Reabrir o app não apaga a incerteza ACS4.

A raiz ACS4 vem do manifesto M2 fechado (`schema:2`, `schemaVersion:6`), cujo hash inclui `bootstrap:{issuerKeyId,issuerPublicKey}` em ordem canônica. O artefato fixo do app inclui esse mesmo manifesto e confere hash, ambiente, origem, emissor e conta capturada; health só compara o resumo. M1/schema5 permanece compatível sem oferecer ACS4. A migração6 conserva a linha antiga de implantação sem mudar sua identidade ou configurá-la automaticamente. O gerador do app só lê o manifesto público qualificado no arquivo fixo de configuração da variante; sua ausência deixa ACS4 indisponível, sem fallback para raiz de variável de ambiente, binding ou tela.

Esta integração inclui a interface candidata e ainda aguarda gates, revisão independente e QA aplicáveis. Manifesto de produção, configuração Clerk Donna, emissor/provisionador, segurança do guest e primeiro vínculo real continuam separados. Nenhum serviço foi ativado por esta documentação.


### Tela da primeira vinculação ACS4

Em Ajustes › Computadores › Pela nuvem, um Hub novo ainda sem dono oferece Pedir vínculo com confirmação explícita. O pedido mostra quatro etapas e uma ação por fase, apenas conforme as capacidades publicadas pelo Main. O identificador pode ser informado ao suporte; a tela não cria pedido, prova ou conferência automática. published significa que o Hub ainda não confirmou a ativação; Vínculo gravado não significa conectado, e Conectar continua outro clique.

Enquanto o pedido estiver em andamento ou com resultado pendente, seu cartão é o único da seção e outros vínculos/conexões ficam bloqueados, inclusive após encerrar e reabrir o app. Só blocksOtherOperations:false explícito do Main libera; o relógio da tela nunca concede acesso. prepared sem efeito pode reenviar o mesmo pedido apenas por clique/canStart. Isso só ocorre quando ele comprovadamente não saiu ou foi recusado antes do registro; um resultado incerto nunca é reenviado.

O prazo é o menor entre grant e recurso. Se vencer antes de qualquer prova sair, inclusive com o canal aberto ou desafio recebido, o pedido deixa de bloquear outros Hubs apenas pelo estado do Main, permanece visível e não pode ser retomado, substituído ou refeito pela tela; informe seu identificador ao suporte. Abertura ou desafio interrompidos ainda não enviaram prova: após o prazo mínimo o Main libera outros Hubs, mas não reabre esta geração. proofUnknown pode ter enviado a prova, permanece bloqueado e só permite Verificar pedido; prazo não libera esse estado. Conferir de novo apenas relê o Main local, sem envio à nuvem. Sucesso de um passo remove o erro antigo; complete indica vínculo gravado, não conectado.

### Emissor e inspeção do Hub novo

O cliente de operador está documentado em cloud/tools/acs4-issuer/README.md. Ele exige M2 e artefato de release admitidos, perfil privado fixo e captura nominal pela API do provedor, com ELF/configuração protegidos e tempo do mesmo boot; não aceita autoridade de JSON, health, tela ou bot. O segredo do emissor é separado e fica somente em RAM pelo 1Password. Recurso e grant originais são sincronizados no journal antes de cada efeito, sem replay após unknown; a conferência só observa o original e não ativa o Hub nem concede acesso. Nenhuma chave, configuração protegida, conta, VM ou serviço real é criado por esta implementação.

bootstrap inspect --json e o RPC bootstrapInspect são somente Local, exigem corpo vazio e devolvem configuração presente/ausente, geração, identidade pública, everClaimed, identificador/hash do grant e estado. Não exibem token, chave privada ou envelopes e não abrem slot, renovam prazo ou concedem permissão. A observação funciona após o prazo inicial, mas só o adaptador qualificado do recurso pode usá-la como uma das provas de origem; sua saída isolada não é atestado.


Journals ACS4 novos são separados por implantação, titular, deviceId e chave de aparelho. O formato antigo só é retomado pelo binding exato; o arquivo válido de uma identidade aposentada permanece histórico e não bloqueia registro/conexão do novo binding. Nenhum journal é apagado ou sobrescrito por troca de chave; corrupção do arquivo corrente continua falha fechada.

A inspeção Local publica também qualifiedClock (instante e margem da referência em memória, ou null); não carrega nem renova relógio. O Hub lê a configuração e a referência root somente ao iniciar: após instalar ou atualizar essa referência, o provisionador precisa reiniciar o Hub pelo procedimento autorizado. Instalação do grant, desafio e prova devem caber no menor prazo do grant, recurso e referência do mesmo boot, no máximo 300 segundos. O emissor confere esse relógio efetivamente carregado antes de criar grant. Prova/posse já gravadas são recuperadas pelos originais enquanto o prazo do recurso ainda admite publicação, sem renovar a autorização inicial. Depois desse prazo, um recibo pode continuar consultável sem permitir publicar a posse; a geração não é reaberta e a tentativa permanece bloqueada.

No emissor, publish-grant pode recuperar hubInstallUnknown somente quando uma captura nova e qualificada observa o mesmo bootstrapId e hash do grant original em active; persiste hubInstalled e faz a primeira publicação, sem reinstalar nem assinar de novo. install-grant só existe para hubInstallNotSent produzido antes do POST de instalação: instala os mesmos bytes por clique do operador. Uma instalação possivelmente enviada nunca é repetida. Nem isso cria raiz, relógio, usuário, VM ou autorização administrativa.

O POST assinado de `bootstrapReconcile` não envia corpo: a assinatura cobre zero bytes. O Worker continua recusando qualquer corpo; confirmar e concluir apenas publica o mesmo recibo pela transição CAS documentada.

### Encerramento histórico ACS4 (A4)

A fase closed é distinta de complete e nunca indica vínculo ou conexão. O Main só a grava depois de validar a cadeia imutável e uma leitura autenticada da mesma intenção: canceled exige revogação assinada do Hub, mesmo grantId/hashHubReceipt e confirmação das cercas; expired exige que o Worker tenha encerrado a intenção não publicada por CAS após o prazo do recurso. O POST explícito de reconciliação faz esse encerramento; GET e restauração não escrevem no Worker. A revisão CAS também impede que uma publicação com relógio capturado antes do vencimento ultrapasse o terminal. Se a publicação já venceu a corrida, ela conserva seu estado e precisa da ativação assinada normal.

O journal conserva os originais e o encerramento, sincronizados antes de blocksOtherOperations:false. O histórico não reabre esta geração, não aceita nova cerimônia ou publicação do pin e é conferido ao restaurar. Só os outros Hubs são liberados. Relógio local, 409, revogação pendente, recibo diferente ou proofUnknown sem recibo não encerram o bloqueio. O renderer mostra o histórico encerrado com o identificador e sem ação de vínculo, conexão, repetição ou exclusão. Sem blocksOtherOperations:false explícito, conserva o bloqueio e só oferece a leitura local Conferir de novo por clique. Nenhuma nova ação é oferecida pelo Main. Código candidato, sem configuração real ou QA instalada.

Quando a publicação do arquivo já começou (pinUnknown), o pedido não vira closed: pode existir um .pendingv3, e a finalização original precisa continuar disponível. A ação explícita Concluir gravação sincroniza somente os mesmos bytes já retidos, inclusive após reinício, sem reenviar grant, prova ou publicação ao Worker. Não apagar o pendente nem encaminhá-lo ao leitor de pinv2. Esse recibo é histórico; a finalização local não renova grant, lease ou permissão revogados, e cada conexão continua sujeita à admissão atual do Hub. Em published ainda não começou essa gravação: o encerramento assinado continua possível.

Fase B candidata: descoberta managed (`available`) sobrevive à janela inicial;
`canStart` controla apenas cadastro/cerimônia novos e precisa true explícito.
Janela vencida não bloqueia Connection de journal complete nem leitura da mesma
recuperação. Pin, conta/device, publicação/fences e lease continuam na admissão
normal; não renova grant, prazo ou prova por esse caminho.
O processo main consulta a oferta managed da conta após login,
mesmo sem abrir a tela Computadores. Só oferta designada pelo operador permite
cadastro de aparelho e ACS4 automáticos. managed-connection mantém unknowns em
leitura/reconciliação; a prova continua na chave privada local. Completo permite
connect pela rota normal com pin verificado. Desconectar pausa nessa sessão;
epoch invalida o trabalho. Nada disso liga bootstrap em recurso sem marca managed
nem exige organização. Não é QA/instalação: implantação e configuração continuam
separadas, com admissão e revisão próprias.

### Cifra do canal no Electron

Os quadros usam ChaCha20-Poly1305 de `@noble/ciphers` 2.4.0. O BoringSSL do
Electron não fornece essa cifra em `node:crypto`; Node/OpenSSL verde sozinho
não qualifica o pacote. Nonce, AAD, contador, tag e vetores são os mesmos do Hub.
Não há cifra fraca, canal em claro ou aceitação sem tag. A qualificação nativa
precisa selar e abrir os vetores no Electron embarcado, recusar adulteração e
contador errado e conferir a dependência no artefato. Falha antes da prova
conserva o pedido: não repetir grant/prova nem apagar journal ou slot.


### PL1 C3b.1 — candidato R24, oferta fechada

Capturas pessoais são separadas do SSH legado: Linux usa pathname helper root
protegido; SSH usa processo próprio certificate-only, fonte embutida vazia,
truststore exclusivo e diagnóstico -E separado de stderr remoto. Metadados do
certificado só são considerados com CA, principal, aceitação/autenticação e
processo/truststore capturados, e validade conferida contra time E. Diferenças
ou versões sem evidência completa recusam; TOFU/known_hosts não qualificam.
Nenhum SSH/CA/servidor foi executado ou qualificado nesta frente. O registro
privado junto de trustHostKey registra apenas proveniência TOFU, sempre recusada
para PL1, preservando o caminho legado.

O journal pessoal usa safeStorage seguro (basic_text recusa), FS/fsync e CAS,
originais e envio desconhecido antes do efeito. Principal mantém closures e o
selo originais, DeviceBinding é copiado como dado; suspend/resume corta a captura
sem restaurar lease por parede. DTO não entrega envelopes, token, comando ou
assinador. MIRROR permanece10: não há campo novo de hello/bot/entry.

O candidato C3b.1 agora liga ForkCloudService.invoke e PersonalService reais
até R, via captura/time/helper, /personal-prepare E2EE e /personal-possession
estrito K/F/R. Bootstrap §11.3 fixa payloads/custódia.
Candidate usa somente token original no slot secreto cifrado e A de I; Hub
retém digest/declaracão, nunca token em histórico. Enrollment devolve somente
userCode público e datas Worker exatas, sem deviceCode ou identidade de dono.
DeviceCode permanece cifrado exclusivamente no Hub após G/R e prazos, pois
read H ainda o exige. Queda do begin não reemite código ou cerimônia.

Journal schema2 tem originais públicos separados de slots secretos cifrados,
fsync/CAS, um envio em voo e digest imutável por rota. Remoção exige prova e
não permite ressurgimento. Restore/state só lê; reconcile explícito consulta
mesma operação/IDs/bytes/prazo, sem auto-reenvio. Device sem I pode apenas
interromper observação, sem cancelamento Worker falso; command perdido não
regenera token. Contexto privado insuficiente mantém indisponível. Cada await,
assinatura, fila, write e retorno revalida Principal/binding/captura originais.
Corte de suspensão exige nova prova de tempo/captura antes de qualquer efeito.

D70 candidato compõe invoke/service/journal FS/crypto/POP/G/R reais no Main,
com os transportes/helper/OS externos explicitamente controlados. Os focais
Rust cobrem o handler/Store/ACL/SQLite reais; mesmos vetores públicos de
transcript/frame cruzam os consumidores, sem alegar processo Electron/Hub
instalado nem atomicidade em mocks. Teste SSH controla o processo externo e
usa arquivos/adapter/parser/frames reais; não é OpenSSH remoto qualificado.
NOVA24 deu NÃO estático. Na fonte96a36d8d o root reproduziu três falhas Main
(contadores de efeito1!=0) e Rust59 pass/1 fail por commit begin tardio. O patch
R24 é candidato sem prova pós-fix; SIM23 continua somente contratual.
A guarda de efeito fecha sobre a MESMA captura e relê origem/proveniência,
Principal/DeviceBinding/signer e tempo após fsync/journal, token HTTP e esperas
de preparo/posse, antes de assinar/enviar e no retorno. Inclui status do helper
(U/G), finish/op e F após mark. Falha conserva unknown e originais sem reenviar;
renovar time E pré-A não depende da validade da referência vencida.
Publicação/E/V/relay/pin/ready pertencem a C3b.2, Authority/paridade/UI a C4;
SSH preservado e oferta real fechada. Não há API de segredo para o renderer,
OwnerDevices→contas, ferramenta do bot ou campo hello/bot/entry: MIRROR10.

### C3b.2 M1 — runtime e tempo candidatos até R

Fonte anterior76f0: SIM33 Host/SIM34 integração/SIM35 ACL e focais root
fmt0, personal71, ACS4 8, OAuth upgrade1, Clippy HOST0. Esses resultados NÃO
validam o novo Main. M1 agora integra beforePack existente → Policy12/Receipt14
→ consumidor runtime → clock → invoke/service/preparo/POP/K/F/R. M2 publicação/
E/V/probe/P30/cold-S e C4 contas E chaves Caller::Local continuam no mesmo PR3;
sem attachment/ready genérico ou oferta com None/null/fontes vazias.

`personal-runtime-format.mjs` é codec único do produtor/Main, com declaração
TS; policy real por release/arch e metadados de93 fontes/67 patches/20 membros
builder/osx-sign ficam em resources. `personal-runtime-receipt.cjs` é chamado
pelo beforePack JÁ existente e seleciona dist oficial44.5.1. Não muda deps,
CI, scripts npm, Root ou confiança SSH. Verifica somas/ZIP inteiros, fontes e
builder/lock; extrai confinadamente e grava recibo antes ASAR/sign. Lock de pack
exclusivo e seleção única por processo/worktree impedem out/main entre arches;
falhas preservam artefatos para reconciliação, sem apagar trabalho alheio.

Consumidor confere próprio ASAR/recibo/artefato/instância. Darwin usa Mach-O8
arm64 e unsigned-x64-v1/itens12 conforme SIM31, Framework carregado, ASAR/plist,
codesign e hrtime nativo contínuo admitido. A closure atual é original-zero;
pacote assinado real ainda deve provar isso. Delta strsize/-p/fora da máscara
recusa; não ampliar. Linux localHelper exige ELF inteiro realmente executado,
procfs nativo/FD/PIDself/starttime/boot/ns/offsetzero e pread0; sem addon/child,
code signing fictício, timestamp de parede ou política por VPS. Ausência ou
divergência fecha apenas PL1, não o SSH legado. Funções nativas são capturadas
antes do IPC; getters/FDs e qualificações não são DTO/argumento do renderer.

`PersonalClock` recebe somente sampler qualificado, retém send/receive originais
com q1/q1000 e fornece lower para emitir/upper para vencer. E pré-A/T assinado
é a fonte de época; detector acumulado M/performance só corta. Não soma RTT
duas vezes, não max(timestamp) como presente e não reancora T/G. Causalidade
antes de assinatura/freeze de create/attach/CA/B/AP/F/Read/cancel; HTTP timestamp
fresh após token/guard. Guardas R24 permanecem depois de FS/fila e antes de
helper/status/F/HTTP. Unknown retém mesmos originais, sem reenvio automático.

D70 novo usa codecs/crypto/FS/journal reais; runtime/tempo/transporte externos
controlados só nos loadMainModule privados dos testes. q1000 composto termina
em R, não finge Hub Rust/Electron instalado. Testes sintéticos do normalizador
NÃO provam assinatura. Gate de pacote oficial pós-sign/ELF extraído e sampler
nativo precisam do ambiente alocado pelo root/Operação; ninguém desta frente
instala/copia/abre app no Mac. MIRROR12 intocado (hello normal não muda).

### M1 — fronteira física do ASAR (candidato)

A prova nativa do root com Electron oficial44.5.1/Node24.21 Linux arm64 mostrou
ASAR válido e leitura virtual interna correta, mas RuntimeFile.open do arquivo
inteiro falhou ENOENT: o fs virtual o apresenta como diretório. A prova pontual
não qualifica pacote, sampler, Owner ou app instalado. Separadamente, a API real
process.getBuiltinModule('original-fs') retornou o mesmo backend raw do builtin.

O consumidor agora captura esse getter antes do IPC, com nome fixo, e retém
funções raw da mesma API para RuntimeFile, FDs Linux e rechecagens físicas.
Somente stat/read do recibo dentro do próprio ASAR usam a visão virtual.
Sem builtin válido, importar o serviço continua possível, mas o uso pessoal
recusa; Electron nunca recorre a node:fs como fallback. Fora de Electron, o FS
Node real atende fixtures/dev e não concede qualificação. Não há noAsar global,
flag, dependência, caminho de IPC ou mudança nas guardas de arquivo/tempo.

D70 candidato acrescenta captura única e indisponibilidade do builtin usando
VM só na fronteira do carregador, com arquivos/FDs reais. Os42 casos anteriores
permanecem literais; total previsto44. Prova nativa rawASAR pós-fix, typecheck,
focais, pacote completo/sampler e revisão independente ainda cabem ao root.
M2/C4 continuam pendentes; SSH, None/null e Operação exclusiva permanecem.

### M1 — R36: corte antes do POST e do freeze (candidato)

Root reproduziu dois RED na cd02 (produção54): POST1!=0 no retorno do signer
HTTP e CA persistida1!=0 após fsync/origem trocada. O patch transporta uma guarda
privada fechada sobre a captura/Principal/binding originais e amostra qualificada.
HTTP repete check assíncrono DEPOIS de assinar; em seguida, sem outro await,
sample valida corpo original, geração e timestamp HTTP original por upper<at+30s.
Emissão usa lower; falha não restampa/reenvia nem troca o mark unknown.

remember leva check/current para dentro da fila advance. A guarda completa e
amostra síncrona são repetidas após leituras/cifra/FS e antes do rename, incluindo
validade das novas assinaturas A. O outro ingresso de novo original, I da resposta
command junto ao slot secreto, também carrega a guarda original e validade de I.
O journal genérico não resolve Authority/captura; quem fornece o callback é o
service. Atualizações só de estado/unknown e os testes FS genéricos mantêm seu
contrato sem exigir uma cerimônia. CAS/originais/marcadores continuam imutáveis.
Aborto pré-rename preserva apenas o temporário cifrado exclusivo daquela escrita,
sem publicá-lo, recifrá-lo para retomar, remover desconhecido ou escolhê-lo como
journal atual; o arquivo canônico anterior continua a fonte de recuperação.

Os46 casos, inclusive os dois RED inteiros, permanecem intactos. Focais/TC
pós-fix e fresh38 ainda pendem do root; nenhum gate pelo dev. ExtraASAR/SIM37,
pacote pós-sign, M2/C4 e Operação D138 continuam separados. SSH/None/null mantidos.

### M1 — segundo ASAR oficial Darwin (SIM37, candidato)

O dist custom oficial conserva default_app.asar; builder26.15.3 sela duas
entradas. Agora o candidato exige EXATAMENTE Resources/app.asar e
Resources/default_app.asar, com algorithm/hash fechados. O segundo tem tamanho
110862B, SHA inteiro0eb2491b0a9ac94790389d39c09dd5005c6c1f0665842943829bbd0193f6cc4f
e headerSHAf80143a17fad2075fe6cf176e07033a41a878dba7f31e7ab4f6f14fcc34162dc,
derivados dos dois ZIPs Darwin44.5.1 admitidos. Produtor confere membro único
regular e extração por FD antes receipt/pack; Main mantém FD raw do ASAR extra,
identidade/path/permissões e fechamento junto aos outros arquivos do runtime.
Não executa código extra, não remove arquivo nem muda recibo/app.getAppPath.

personal-runtime-asar é helper próprio exposto pelo formato compartilhado;
normalizadores e Policy12/Receipt14 permanecem literais. O plist bruto do FD
Info é lido antes plutil e aceita somente gramática XML observada: prólogo/DTD
literais sem resolução externa, dict/array/string/boolean, bounds64KiB/16níveis/
1024valores e128chaves por dict. Rejeita duplicatas em TODOS os dicts, entidades,
comentários, atributos/formatos extras, BOM/binary e aliases, sem fallback.
Depois confere as duas entradas contra JSON efetivo plutil e calcula digest de
AMBAS na ordem UTF16 app/default. original-zero/itens8/12/assinatura não mudam.

R36 f692 tem TC0, mesmos dois focais1/1 e pessoais46/46/0skip segundo root.
Essas três implementações/testes permanecem intactos neste delta. Seis testes
novos candidatos cobrem rawXML/dup/alias/bounds, hash inteiro negativo/FS real e
FD/path/links/modos; não trazem código/asset oficial para CI nem fingem positivo
wholeASAR com hash mock. Root ainda precisa provar os dois ZIPs/pacotes assinados
com o novo helper/consumidor, inclusive mutação fora do header, além de TC52 e
fresh38 M1 total. Não é aceite runtime/Owner/D138; M2/C4 pausados mesmo PR3.

### M1 — R38: união exata da VM assinada x64 (candidato)

SIM38 admite no consumidor unsigned-x64-v1 somente VM igual a
max(originalVm,align4096(filesize)) OU max(originalVm,align16384(filesize)).
O gate x64 real observou o segundo resultado; não deriva quantum do SO nem
aceita valores intermediários. O patch altera uma igualdade: nenhuma máscara,
edit, hash ignorado ou origem unsigned nova; restaura a mesma VM original.
Produtor, ARM64/ELF, ASAR37 e guardas R36 permanecem iguais.

Root reproduziu na e482 o positivo16K RED (0/1, RC1 personalRuntimeInvalid) e
controle4K/negativas verde (1/1, RC0), ambos0skip. Os54 casos ficam byteidênticos.
Nenhum gate pós-fix pelo dev. Root repetirá os mesmos casos, runtime12/TC54,
consumidor REAL dos dois artefatos assinados SEM projeção e fresh39 código M1
total; SIM contratual não é aceite de pacote/runtime/Owner. M2/C4 permanecem
obrigatórios no mesmo PR3; SSH/None/null e Operação exclusiva no Mac mantidos.

### M1 — R40: captura original e última guarda antes do envio (candidato)

Queda da captura vinculada corta serviço/clock/canais; drive não recaptura ou
renova a referência após esperar. Restore em nova instância só lê história;
recuperação depende de ação explícita e conserva os mesmos originais/prazos.
Posse confere upper atual contra G/T/K/F, preparo contra Q, inclusive depois
de assinatura/cifra e da última espera, antes de enviar bytes já materializados.
Os5s de cada operação continuam originais; falha fecha sem retry/restamp.
R40 teve quatro RED reais e três controles verdes na5da; este fix ainda aguarda
gates e revisão independente. Sem M2/C4/ready, sem alteração de None/null/SSH.

Cancel device sem I aposenta a referência da captura só depois de stopped
durável e guardas atuais da mesma operação. A própria ação retorna stopped;
perda inesperada da captura continua cortando o serviço. Não há nova fonte.

R42 candidato conjunto: ancorar uma vez por prazo RPC o teto conservador
lower anterior + restante5s após M, retendo teto/geração sem renovação.
Uma NOVA janela final depois de todas as leituras M confere simultaneamente
geração, teto RPC e mesmos G/T/K/F/Q. A incerteza pode recusar mais cedo;
unknown só consulta operação original, sem novo envio/replay. Ainda sem aceite.

M2 em desenvolvimento na fonte C3: check/reconcile explícito após receipt
confere R no Read, fixa publish11/revisãoS no journal antes de HTTP e observa
S/V/activationCurrent em estado separado dos originais PIN. Probe novo é
cliente fechado Open13/hello/proof/Read da MESMA V,5s originais, sem Caller.
Host retém o mesmo permit após2RPC até peerclose/deadline; extras recusam.
P30/17 e CLOCK-S têm consumidores privados e focais próprios escritos, ainda
sem gates. WriterPIN/FS/cold/revoke compostos ainda não entregues: Darwin
aguarda primitiva/probe coordenado, sem alias/fallback presumido. Nenhum ready.
Executor único confere delta; regras históricas de revisor/SIM foram substituídas
pela ordem do dono de04/11.63 testes anteriores permanecem byteintactos.
