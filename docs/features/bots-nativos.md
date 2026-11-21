# Biblioteca de especialistas da Mexerica

O Hub oferece prévias e inclusão explícita de Pesquisador, Redator, Financeiro,
Operador e Planejador, um bot por perfil por Space. O app mostra a biblioteca
([abaixo](#no-app)).
Prévia não significa instalado: só a resposta concluída da RPC comprova inclusão.
Ler, atualizar ou reiniciar não cria bot, rotina ou memória nem inicia IA.

Os cinco perfis são autorais em PT-BR: chave estável, versão, nome, função,
avatar oficial, Alma inicial integral de até 4.000 caracteres, dois exemplos e
sugestões “Pode precisar de…”.
Os avatares preservam os IDs oficiais:
`research`, `escritor`, `financeiro`, `estoque` e `pm`, respectivamente,
com `donna=false`. Não há role, ativo ou estilo novo. Nome/função são apresentação;
`description` é a Alma, nunca mecanismo de autorização. Na versão 2, o rótulo
editorial do Planejador é `objetivos e próximos passos` (27 caracteres), dentro
do limite MAIN de função (40). O Planejador preserva sua Alma
integral e não cria estilo. Os perfis acompanham a versão 2 do catálogo.

## Ações do app e autoridade

| Ação/RPC | Efeito e limite |
| --- | --- |
| `donnaNativeCatalog` | Lê perfis públicos autorais e vínculo somente no Space solicitado; não cria nem reconcilia ator |
| `donnaNativeCatalogAdd` | Inclui ou devolve o bot comprovado; recibo idempotente conserva identidade histórica |
| `deleteBot` | Exclusão normal do bot autorizado; marca slot removido na mesma transação |
| `donnaBotEditorSave` | Edição normal da cópia do dono, com sua versão esperada; catálogo não a sobrescreve |

Todas usam o transporte autenticado normal do app. As RPCs do catálogo exigem
`hubId` e `spaceId` reais no envelope, validados pelo produtor MAIN de Spaces;
`hubId` é retirado antes da desserialização fechada. Campos desconhecidos são
recusados. Nenhuma RPC é ferramenta MCP, ação do chat ou concessão aos bots.
Não chamar pelo shell nem converter narrativa/flag de origem em gesto humano.
Local/Device identificam transporte, não autoria humana de turno ou autorização
para conta/recurso. Permanece o limite de isolamento do Hub, sem nova prova humana.

A leitura usa a autoridade do editor: Local autenticado ou Device com Control.
A inclusão conserva a Authority original de `engine_setup::access` antes de
filas/awaits. Local segue a guarda local normal; Device exige Control **e**
posse ACS4, ativação assinada e lease atuais da capacidade `engine.accounts`.
Control ou `owner.devices` isolados não bastam para escrever. Isso reutiliza a
guarda, sem consultar/selecionar conta nem promover Device a Local. Executor,
Screen, pairing e bootstrap não autorizam inclusão. Replay também revalida a
Authority e a audiência atuais, sem acesso concedido pelo recibo.

## Leitura de prévia

`POST /api/donnaNativeCatalog`:

```json
{"hubId":"id-do-hub-conectado","spaceId":"id-real-do-space"}
```

A resposta efêmera contém `hubId`, `spaceId`, `catalogVersion` (atualmente 2),
`profiles` e `links`. Não há cache novo nem campo em bot/hello/entrada/status;
`MIRROR_SCHEMA` permanece igual.

| Campo de `profiles[]` | Contrato |
| --- | --- |
| `catalogKey` | `pesquisador`, `redator`, `financeiro`, `operador`, `planejador` |
| `profileVersion` | Inteiro positivo, atualmente 2 |
| `origin` | `donnaOriginal` |
| `name`, `title`, `avatarConfig`, `description` | Valores autorais iniciais, avatar pelo schema oficial |
| `examples`, `suggestedResources` | Exemplos passivos e sugestão editorial; não executar nem anunciar prontidão |

Cada `links[]` possui `catalogKey`, `generation`, `revision`, `botId`,
`profileVersion` inicial e `state`: `available`, `active` ou `removed`.
Ausência é `available`, geração/revisão 0 e botId/versão null. Hidden continua
`active`; só a exclusão normal marca `removed`. A prévia não consulta a Alma
editada, memória, contas ou inventário do bot, nem afirma existência em outro
Space. Não procura bots homônimos. Sem vínculo comprovado, nenhum bot manual é
associado por semelhança.

## Inclusão e readição

`POST /api/donnaNativeCatalogAdd`, corpo fechado após o envelope:

```json
{
  "hubId":"id-do-hub-conectado",
  "spaceId":"id-real-do-space",
  "catalogKey":"pesquisador",
  "profileVersion":2,
  "clientRequestKey":"uuid-do-gesto",
  "intent":"add",
  "expectedGeneration":0,
  "expectedRevision":0,
  "backend":"codex"
}
```

`clientRequestKey` tem 1–100 caracteres ASCII alfanuméricos ou `-_:`, e é
preservada para retry do mesmo gesto. Expectativas vêm juntas da prévia do mesmo
Hub/Space. Não aceitar configuração, IDs de bot, permission, grant, conta,
runner, workspace, skills, conectores ou autoria pelo corpo. Versão diferente
da atual exige reler antes de novo gesto; recibo conhecido é consultado antes
da versão/slot e da validade dinâmica do backend atuais. Se o backend sair do
registro depois da inclusão, retry com chave/corpo idênticos ainda resolve o
recibo histórico sob a Authority e audiência atuais. Novos gestos continuam
validando o backend antes de resolver/criar; essa distinção não aprova execução. `backend` é opcional em Pessoal, seguindo o default normal
de `new_bot`; fora dele é obrigatório e explícito. Backend não é conta nem
aprovação de gasto/execução. Sem nome conhecido de motor, recusa.

O Hub monta somente a allowlist do catálogo: Agent, nome, Alma, função e avatar
iniciais; `Permission::Ask` explícito mesmo com TeamSettings Auto;
`permissionChosenByUser=false` e `createdBy=NULL`. Computador, gestão, conectores,
logins, skills, conta de motor, comando e runner começam ausentes/desligados.
Workspace próprio novo, memória/histórico sem cópia e sem memória compartilhada
herdada. Não copia configuração da principal ou de outro bot. Criar não envia
mensagem, inicia trabalho, liga rotina ou aceita custo; o ator só sobe após commit.
O usuário pode editar depois pelo editor normal; atualizar catálogo nunca
reaplica Alma, nome, aparência ou configuração a um bot do dono.

A resposta concluída traz `hubId`, `spaceId`, `catalogKey`, `clientRequestKey`,
`generation`, `botId` e `state` (`active`/`removed`). Não devolve perfil privado
nem escolhe outro bot. Para obter revisão atual, releia a prévia.

O slot UNIQUE `(Space,catalogKey)` guarda geração monotônica, revisão, botId,
versão inicial, estado e base da ativação. O recibo imutável UNIQUE
`(Space,catalogKey,clientRequestKey)` guarda digest, operação, geração e botId.
Não contém conversa, Alma privada, memória ou credencial. Consulta recibo antes
do slot: chave igual/digest diferente conflita. Duas intenções da mesma prévia
resolvem o mesmo bot ativo e registram recibos distintos, sem salvar o perfil.
Pedido desconhecido só resolve ativo se espera exatamente o ativo atual ou a
base geração/revisão/operação que o ativou. Assim, intenção velha desconhecida
não se vincula ao sucessor após remover/readicionar.

Bot, slot e recibo são gravados na mesma Connection/transação da Authority
original, ordem proprietário Cloud → DB, com revalidação antes do commit.
Preparação externa reutiliza `new_bot::with_defaults` e `Hub::prepare_new_bot`;
persistência usa `store::bots::save_bot`, sessão principal pelo produtor MAIN.
Callback síncrono só usa a Connection recebida, sem await/I/O/Store/Cloud
recursivos. Falha de recibo ou autoridade reverte todo o cadastro. A preparação
rastreia diretórios exclusivos da nova identidade: erro de preparação/rollback
remove apenas os criados pela operação e ainda vazios, incluindo o pai do
workspace. A limpeza usa `remove_dir` e preserva conteúdo e diretórios
pré-existentes.
Commit confirmado desarma a limpeza antes de qualquer falha posterior.

Remover usa a exclusão normal. O gancho `fork::on_bot_deleted` marca o slot
removido com CAS em botId/geração/revisão **dentro** da transação de exclusão;
não depende de observer/evento posterior. Recibos e tombstone permanecem.
Readição exige `intent:"readd"`, **chave nova** e geração/revisão removidas
capturadas da prévia; começa outro bot/ID/geração, sem restaurar memória/histórico.

Sequência: A→bot1, remover, B→bot2. Replay A retorna geração 1/bot1 `removed`,
nunca bot2; retry B retorna bot2. Repetir exclusão de bot1 não remove bot2.
Readicionar com A/digest alterado ou expectativa velha conflita. Resultado
incerto conserva chave/corpo e consulta o mesmo Hub/Space; não inventar outra
intenção nem interpretar timeout como ausência de efeito.

## No app

Abre pelo **+** da barra lateral no Pessoal (“Biblioteca de especialistas”) e pelo
⋯ do space aberto (“Biblioteca de especialistas…”), sempre para o space de onde
abriu, nesse computador.

- A folha lê `donnaNativeCatalog` ao abrir e mostra os cinco perfis do Hub, com
  o estado do vínculo deste space: Disponível, **Já neste space** ou **Removido**.
  Não diz se o perfil existe em outro space. A leitura fica só na folha aberta.
- A prévia mostra a Alma integral, os exemplos, “Pode precisar de…” como
  sugestão (sem concessão; o inventário efetivo ainda não existe no app), o space
  atual e o que acontece ao adicionar.
- Motor: no Pessoal segue a fonte canônica (sem `backend`). Fora dele, a escolha
  pela lista normal de motores do Hub é obrigatória antes de Adicionar. Nenhuma
  conta, modelo, custo ou permissão é escolhido aqui.
- Adicionar só fica disponível quando o Hub informa a capacidade de gestão
  (`donnaEngineAccess`); o Hub continua decidindo a autoridade.
- Só a resposta `active` confirma. No Pessoal, o app abre o **Bot por dentro**
  normal quando o bot chega. Fora do Pessoal, oferece a **conversa exata** do bot
  no space; se ela ainda não aparece, o space mostra **Atualizar**, sem abrir a
  Berga ou outra conversa. Editar fora do Pessoal: “Editar este bot fora do
  Pessoal ainda não está disponível.”
- Recibo histórico Removido (retry de um pedido cujo bot foi removido depois) não
  oferece abrir nada, nem o bot que a releitura mostrar ativo; **Ver estado atual** é a
  ação separada que volta ao vínculo de agora. Abrir a conversa de outro bot no space já
  aberto não mostra nada da conversa anterior.
- Já neste space abre o mesmo bot (Pessoal: Bot por dentro e Conversa); atualizar
  o catálogo não muda o bot do dono. Removido exige **Adicionar de novo**, com
  confirmação de que começa outro bot sem a conversa e a memória do anterior.
- Sem resposta, a mesma chave e o mesmo corpo ficam guardados em memória do app
  aberto; **Conferir resultado** reenvia igual. Nenhuma chave nova automática;
  **Descartar pedido** só esquece no app. Resposta que chega depois de trocar de
  space, de cair ou voltar a conexão é ignorada, e a intenção fica para conferir.

## Limites e validação

“Pode precisar de…” não afirma inventário efetivo, selo Pronto, conta, modelo,
velocidade, ferramenta disponível ou custo. Instalar recurso ou escolher conta
continua ação separada do dono, com sua autoridade. Sem inclusão no boot/refresh,
sem reset/sincronização de perfis, importação, nova memória ou agendador.
Nenhuma dependência nova ou plano incorporado ao binário. A UI consome só esse contrato.
