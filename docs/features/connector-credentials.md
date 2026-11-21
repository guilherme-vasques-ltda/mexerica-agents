# Conexões e credenciais — Mexerica

Codync uses one host-owned connection workflow on iOS and the desktop app (macOS, Linux). A
Installing from Extensions and a bot's connection request use the same installation,
OAuth and verification endpoints. The clients never implement a separate password
store. A phone submits credentials through its authenticated encrypted channel to
its selected host.

## Storage

`market/vault.rs` encrypts sensitive records with XChaCha20-Poly1305, a random
24-byte nonce and the record key as authenticated associated data. A random
256-bit key belongs to this host database's random vault ID. It is held by the OS:

- macOS: Keychain, via the native keyring backend.
- Linux: freedesktop Secret Service over the user's session D-Bus, with encrypted
  transport and a vendored D-Bus client. GNOME Keyring or another compatible
  unlocked Secret Service is required. Run the host in that user's session.
- Headless Linux: provide a session D-Bus and unlocked Secret Service for the
  service user. An unavailable keyring gives an actionable error. There is no
  plaintext, kernel-keyring-only or automatic ephemeral fallback.

O desbloqueio roda num worker bloqueante, com uma única leitura de chaveiro por
vez. Chamadas que chegam durante essa leitura compartilham seu resultado. No
Linux, o desbloqueio da coleção padrão apresenta um único pedido de senha e o
dispensa após 1 minuto; cada chamada espera até 65 segundos, incluindo a margem
para chamadas D-Bus. Em servidor sem tela, esse pedido pode não estar visível.
Se o cofre continuar indisponível, somente pedidos que dependem dele recebem um
erro claro de armazenamento seguro travado ou indisponível; o restante do Hub
continua respondendo. Falha de transporte, prompt dispensado ou timeout não
criam outra chave. Sem a chave de um cofre já inicializado, os registros cifrados
são preservados e a operação falha.

Limite aceito: uma leitura do sistema que ultrapasse esse prazo (Keychain no
macOS ou chave fora da coleção padrão no Linux) conserva a trava até terminar
ou até o processo encerrar. Enquanto ela estiver presa e não houver chave no
cache, cada pedido dependente do cofre espera até 65 segundos e falha; o timeout
da chamada não cancela a leitura nem libera uma segunda leitura/criação.

A chave fica em memória com zeragem ao liberar até o encerramento do Host.
Trancar o chaveiro do desktop depois do desbloqueio não remove esse cache.
Isso não isola segredos de código arbitrário executado como o mesmo usuário do
sistema. Não há fallback em texto aberto nem mudança de autoridade.

Encrypted records include connector environment, headers, command arguments and
OAuth tokens, agent API-key environment, and the optional
1Password service-account token. Existing records migrate on their first read
with an available key. Missing keys and corrupt ciphertext are errors, never an
empty replacement configuration. SQLite secure deletion is enabled; historical
backups and preexisting WAL/filesystem snapshots can still contain older data.
Back up the OS credential store together with the database. A database copied to
another machine without its key cannot decrypt credentials. There is no automatic
cross-host or cloud-account credential sharing.

## Execution boundary

Pedidos nativos de conexão, credencial e login conservam a execução de origem
capturada pelo Hub antes de desbloquear o cofre. A gravação do cartão confere o
vínculo na mesma transação e usa a sessão original; uma chamada revogada ou de A
durante B não publica em B nem recua para a principal. Listagens também revalidam
antes de responder. A conclusão segura pelo app mantém seu fluxo autenticado
próprio; não recebe uma execução MCP fabricada. A limitação A₁→A₂ na mesma sessão
continua valendo.

Handshake e descoberta não autorizam uma sessão antiga a receber credenciais.
No gateway local ou remoto, o Hub exige a execução original ativa ou a preparação
canônica vigente de `session/new`/`session/load`, inclusive no reuso da principal.
Essa preparação conserva o token já emitido pelo Hub, sem fabricar ExecutionRef
ou ACP, e é invalidada na troca/parada do turno. Só admite handshake/descoberta;
setup OAuth mantém sua concessão restrita e prazo próprios.

A preparação termina também no mesmo commit que ativa a ACP, inclusive quando
o binding é o mesmo. Uma captura anterior não é promovida para execução ativa;
depois do sucesso é necessária uma nova intenção no contexto ativo legítimo.

Recibo e exposição de um retorno exigem a guarda na mesma transação, antes da
gravação. A exceção de notificação externa tardia só marca a sessão de origem,
sem marcar a outra sessão ou o estado global do bot, liberar credenciais ou
entregar conteúdo ao motor. Um retorno com ID não vira notificação por mudar o
rótulo. A classificação é compartilhada entre parser, HTTP, stdio e guarda:
exige JSON-RPC 2.0 e método/forma de notificação MCP aceitos. Uma chamada
`tools/call` sem ID, versão ausente/incorreta, `id:null`, result/error ou parâmetros
inválidos não recebem a exceção passiva. A marca temporária de atividade externa
dos pedidos nativos só é publicada após resultado/commit válido; uma recusa não
apaga marcas anteriores.

O motor recebe comandos da ponte local com ambiente vazio. O gateway stdio do
Guard obtém a configuração por `donnaGuardRuntime`, autenticado e restrito ao
loopback, com contexto emitido pelo Hub e vínculo entre bot, conector e execução.
As credenciais são injetadas apenas no processo do conector. O antigo getter
genérico `connectorRuntime` foi removido. O ID opaco do contexto vai no argv da
ponte e pode ser visto por processos do mesmo usuário; isso não cria
isolamento contra shell nem comprova identidade humana.
Remote connectors use the existing host OAuth proxy. General listings omit secret
values and command arguments. The local API token and same-user host access are
trusted capabilities; this is not a sandbox against the agent's own shell tools.

A readiness check starts the connector, performs MCP initialize and tools/list,
and stops the check process. It never invokes a business tool. Failures and a
90-second timeout remain visible and retryable. Package acquisition is performed
by the configured runtime (npx, uvx, Docker, etc.), so that runtime must be present.

## Novos bots, conectores e contas

Bot novo prefere Codex disponível no Hub; só usa outro motor se o Codex estiver
indisponível ou com logout confirmado. O usuário pode escolher outro motor.
Conectores MCP e contas de ferramenta novos começam desligados em todos os bots.
Instalar ou importar um conector não o liga automaticamente. O usuário escolhe
cada conector ou conta por bot; escolhas explícitas na criação são preservadas.
O Berga copia só skills ao criar bots, sem conectores, contas de ferramenta
(inclusive `ferramenta-*` e origem `ferramenta`) ou logins.
Motores e acessos de bots existentes não são alterados por migração.

## Conversation requests

The built-in `connectors` MCP offers list_connectors, request_connection by an
existing connectorId, and request_secret. MCP Registry
search and installation have been removed. Legacy search_connectors and requests
by registryName alone fail with mcp_registry_unavailable before discovery or a
new card. The user adds native MCP connections by URL, command or pasted config.
Existing catalog-origin connectors retain their IDs, materialized targets,
credentials and bot selections; registryName is passive provenance. An incomplete
record remains listed as configuration: manualRequired, without remote lookup.
Saved configuration/authentication is not proof that tools are currently usable.

A integração dedicada do Composio (chave de projeto, usuário, contas conectadas e o servidor MCP embutido) foi removida; o Composio é só um conector MCP comum, conectado pelo MCP oficial dele. Requests become
notice entries with structured connectionRequest data. They identify the requesting
bot, service, destination field, purpose and status; they contain no credentials.

Legacy catalog-only cards remain pending and unavailable; the host does not
install or select a substitute to complete them. Explicit cancellation remains
available. Cards already bound to a connectorId retain the existing workflow.
On the desktop such a card shows no Connect button: it says the catalog was
retired and points to Extensões → Meus MCP for the manual connection.

An add or import that gets no clear answer (the link drops or times out) may have
been saved. The desktop treats it as uncertain: it reads the same Hub's list again
and allows another add only after a successful read of that Hub's list requested
after the write ended, even if the form is closed and reopened. When the re-read
fails, the form shows the uncertain result with the read error and **Reler a
lista**, which only reads; nothing is repeated, assumed empty or moved to another
Hub. After a switch to another Hub only that button, with the notice on screen,
frees another add.

Clients submit values through connectorRequestFinish. The host validates the
request, destination, requesting bot and 24-hour expiry, stores credentials,
verifies availability, enables the connector for that bot, and queues a non-secret
acknowledgement in the original conversation. Repeated completed submissions
return the completed entry. Verification failures remain pending so the user can
correct credentials and retry. Current clients offer no way to cancel a request
(it stays until it's done); `cancel: true` remains for older clients, and a
cancelled request rejects submissions.

The host posts one more kind itself, `agent`: when a bot's own agent (Claude Code…)
refuses a turn because it needs signing in, the chat gets a card naming the agent
(`backend`) and the user messages it couldn't answer (`entryIds`) instead of an
error. A bot keeps one such card; messages that fail later join it. The card's
Sign in opens the agent's sign-in sheet from Extensions. As soon as a sign-in check
(`agentAuth`, `agentAuthenticate`, `setAgentEnv`, or connectorRequestFinish on the
card) finds the agent signed in, every pending card for it completes and its
messages go out again; a card older than 24 hours only completes.

MCP connection requests and runtime secret retrieval are loopback-only. Authorized
Control devices may submit or cancel requests. No endpoint exposes a generic
credential getter to a device. Existing local-process trust boundaries still apply.

## Cartão seguro: senha e chave fora da conversa

Pedidos `request_login` (login de site) e `request_secret` (chave de conector) aparecem na conversa como o **pedido
seguro**, desenhado pelo app: selo "Esperando você", quem pediu, **Para** (site ou conector), **Fica guardada** ("No cofre
da Mexerica, só para o bot"), **Digitar a senha com segurança** e **Agora não** (só recolhe o pedido; não cancela nem
bloqueia a conversa). O cartão **não tem campo de valor**. Os logins já salvos para o mesmo site continuam com **Usar**,
que manda ao Hub só o id.

O botão abre o **cartão seguro**, uma janela própria do app (filha modal da janela principal), com usuário (só login), senha
ou chave (`<input type="password">`), rótulo (só login) e o olho **Mostrar**: o valor fica visível por no máximo 10 s,
esconde sozinho, ao sair do campo e ao fechar. Ao salvar, o cartão da conversa vira **Guardado
no cofre** (nome da senha ou da chave, nunca o valor, nunca Copiar; **Ver no cofre** leva a Extensões → Credenciais).
Uma janela por pedido: abrir de novo foca a que existe. O código de login (2º fator) usa o mesmo pedido seguro e a
mesma janela (mais abaixo); o aviso de senha colada no chat abre esta janela vazia (mais abaixo).

Caminho do valor: campo da janela segura → IPC estreito do preload dessa janela (`window.mxCofre`, 3 ações) → processo
principal → `connectorRequestFinish` no Hub → cofre. Ele **não** passa pelo renderer do chat, pelo store, por entradas,
histórico ou espelho, por logs, nem por IPC de difusão. O chat só manda a **referência** do pedido (computador e
`entryId`, mais `spaceId`/`hubId`/`conversationId` de roteamento conferidos pelo Hub): o main resolve e captura ele
mesmo a conexão com o Hub — o local em loopback ou o túnel SSH com as mesmas guardas de `captureLocal` (identidade,
validade e posse do listener, reconferidas após cada espera e imediatamente antes do envio; conexão por relay/nuvem
falha fechada) — e busca a entrada por `connectorRequestGet`, a menor leitura do Hub: o GET devolve uma projeção sem
valor de cartões login/secret ainda não vencidos (com o status deles) e nunca valor nem senha; o main exige status
`pending` na abertura e antes do envio. Todos os textos da janela (site/conector, bot, campo) são derivados dessa entrada;
antes de enviar, o main reconfere a mesma entrada e a mesma captura. O chat recebe de volta só "aberta / fechada /
salva" e erros traduzíveis; "salvo" chega ao cartão pela atualização normal da entrada no Hub (`status: ready`).

A janela segura usa `sandbox`, `contextIsolation`, sem `nodeIntegration` nem DevTools, `setContentProtection`, é marcada
como contexto privado (fora do host-proxy e das difusões) antes de carregar, nega `window.open`, bloqueia navegação e fecha
se o renderer cair. O processo principal confere remetente (a instância viva), quadro principal e um nonce por abertura.
Não é a caixa protegida (`fork-secret-box`): outro contrato, que continua como está. A interface promete só o que é verdade
(não aparece na conversa, vai para o cofre, o bot recebe só o nome), nunca que o bot não consiga ver a tela.

**Quais conexões funcionam.** O processo principal só alcança Hubs que ele mesmo conhece, e escolhe pelo id do computador
(o chat nunca envia endereço nem token): o Hub deste computador e computadores por túnel SSH em 127.0.0.1, inclusive
dentro de um space (com `spaceId`/`hubId` conferidos pelo Hub). **Conexão pela nuvem (relay) falha fechada**: o envio por lá
é assinado e de resultado incerto, e o valor não pode seguir por canal que o app não controla de ponta a ponta; o cartão
mostra que não dá para digitar daqui, sem cair para um campo na conversa.

**Login novo, sem pedido do bot.** O aviso de senha colada no chat ("Isto parece uma senha. Mover para o
cofre?") abre a **mesma janela segura**, mas **vazia**, para guardar um login novo (site, usuário, senha, nome). O trecho
escondido **não** vai do chat para a janela, nem por IPC: o chat o descarta ao clicar e a pessoa **cola a senha de novo**,
direto na janela ("Cole aqui a senha que você colou na conversa"). O canal novo (`fork-cofre-seguro:abrir-novo`) leva só o
id do computador e **recusa qualquer outro campo**; o `entryId` reservado `login-novo` não abre pelo canal do pedido do
bot. Como não há pedido no Hub, o main salva pelo caminho que o Hub já tem, `credentialSaveLogin` (o mesmo da tela
Extensões → Credenciais; nenhuma rota nova), chamado **pelo main** com a captura da abertura e as mesmas guardas da janela
do pedido: remetente geral no quadro principal, nonce por abertura, Hub local ou túnel SSH com posse reconferida antes e
depois do envio, `redirect: 'error'`, só o status volta, uma janela por abertura. **Relay (nuvem) é recusado** (a janela
nem abre e o cartão explica). Na abertura o main confere a captura com uma leitura sem valor e sem cofre (`hello`); o
valor só existe no corpo da chamada ao Hub.

**Código de login na janela segura.** O pedido `request_login_code` (`connectionRequest.kind = "code"`)
vira o mesmo "pedido seguro" da conversa: título do login e site (do Hub, nunca texto do bot), **Para**, **Vale** ("Uma vez só,
por 5 minutos. Não fica guardado.") e o botão **Digitar o código com segurança**; não há campo de valor no cartão. Estados:
pendente, **Código entregue** (sem "Ver no cofre": o código não é guardado) e cancelado. A janela abre em modo código:
**caixinhas de número** (6; crescem até 10 se a pessoa colar um código maior), **um só `<input>` numérico** por baixo delas
(`inputmode="numeric"`, `autocomplete="one-time-code"`), que aceita **só dígitos** (colar "123 456" ou "Seu código: 482913"
preenche as caixas com os dígitos), **4 a 10 dígitos** como o Hub, e o botão **Usar código**. Textos "Vale uma vez só, por 5
minutos." e "Não fica guardado."; a janela **não promete que o bot nunca vê o código** (depois de digitado ele pode aparecer
na tela). **Sem olho**: é um número curto que a pessoa confere enquanto digita. O main usa o
mesmo caminho: o Hub entrega a projeção do pedido por `connectorRequestGet` (agora também para `code`; a projeção é a
mesma, sem `loginId`, sem a sessão de origem e nunca com o código) e o código segue pelo `connectorRequestFinish`, com as
mesmas guardas (captura local/SSH com posse reconferida, `redirect: 'error'`, remetente, quadro e nonce, pedido reconferido
antes do envio, relay recusado, só o status volta). O código só existe no campo da janela e no corpo do `finish`; o Hub o
guarda só na memória (uso único, 5 minutos). `finish` de código não toca o cofre.

**Login novo nunca substitui.** O main chama `credentialSaveLogin` com `createOnly: true`. Nesse modo o
Hub confere e grava **sob a mesma exclusão** (um `Mutex` do slot `logins`, que `save`, `create`, `relabel` e `remove` tomam,
sem corrida entre conferir e salvar): se já existe login com o **mesmo site normalizado** (`https://www.GitHub.com/x` →
`github.com`) **e o mesmo usuário**, ele **recusa com 409 `login_exists` sem modificar nada** — senha, id, nome e os bots
em que o login está ligado ficam como estavam. A janela mostra "Já existe um login para este site e usuário. Para trocar
a senha, use Extensões → Credenciais." (6 idiomas) e continua aberta. O caminho normal (sem a flag, ou com `false`: Extensões
→ Credenciais, o cartão do pedido do bot) **não mudou**: continua trocando a senha no mesmo id. `createOnly` que não é
booleano é recusado. Um login realmente novo nasce sem nenhum bot ligado. Outras recusas da gravação (cofre bloqueado,
nome repetido) têm texto próprio do login novo, sem "pedir de novo ao bot", e nada do corpo do erro do Hub volta.

O chat recebe só "aberta / fechada / salva"; o cartão do aviso mostra
**Guardado no cofre** (estado sem valor) quando a janela salva, e some se a janela for cancelada (cancelar não guarda nada).

Hub: `finish_as` e o cofre não logam nem devolvem o valor (a resposta leva só a entrada, sem segredo). A leitura nova
`connectorRequestGet` (`requests::inspect`) é projeção explícita — ids, nome do bot, tipo/status, título/site/campo/local
e contexto — restrita a login/secret, recusa pedido vencido pela mesma regra de idade do finish e nunca devolve valor;
pedidos de outros tipos e contextos divergentes são recusados na entrada da API. A rota passa pelo mesmo filtro de
origem do Hub (Caller, autoridade e `spaces::guard`, como toda chamada); a única diferença é que ela **não destranca o
cofre** — é leitura de projeção, sem segredo — enquanto `connectorRequestFinish` continua exigindo o cofre aberto.

## Several accounts per service

Each account is an item of its own, turned on bot by bot in the bot's settings.
For MCP connectors, add the same server twice (by URL, command or pasted config)
under different names, e.g. "GitHub pessoal" and "GitHub Casa": each gets its
own id (the name's slug, `github-pessoal`), its own OAuth sign-in and headers, its
own switch per bot, and reaches the bot as an MCP server named by that id. A
connector added by hand can't reuse a name already used by another connector of the same service in the same Space (ignoring case, accents and punctuation); the same name in another Space is fine. When
signing in the second account, pick that account in the browser's sign-in page.
A second account goes in by URL or command under its own name. No Registry
lookup or automatic package update occurs. `list_connectors` shows a bot only the
connectors on for it. New MCP connectors and tool accounts start off for every
bot. Enable them only for the intended bots. Removed
connector ids still selected in a bot are reserved so a later account cannot
inherit that access by reusing the same name.

## Website and app logins

Na Mexerica, referências de senha (`{{senha:...}}`) ainda são inertes. `request_login`
em `site`, `request_login_code` em `login` e `type_login` em `login` recusam a
presença literal de `{{senha:` em
qualquer posição, inclusive sem fechamento, antes do cofre, pedido ou driver.
A resposta fixa não ecoa a entrada: "Esta referência de senha exige um caminho
protegido ainda indisponível." Não há coleta privada ou concessão nova; não
remova a marca nem contorne a recusa. O fluxo legado abaixo continua para nomes,
IDs e sites sem essa marca. Veja o
[manual do harness](manual-do-harness.md).

`market/logins.rs` keeps sign-ins (`site` = domain or app name, username, password
or 1Password vault reference, optional `label`) as one vault record. A login's label
defaults to `site · username`; labels are unique. Each bot has the login ids it may
use in `BotConfig.logins` (bot settings → Sign-ins). Bots use `list_logins` (id,
label, site and username of the logins on for them) and `request_login {site,
reason}`, which becomes a login card; there the user saves a new login (with an
optional label) or picks one already saved for that site, and only the requesting
bot gets it turned on and is resumed with its id. Logins saved before per-bot sign-ins start on
for no bot. New ids carry a random suffix and never reuse a removed login's id.
The `computer` tool `type_login {login, field}` takes an id, a label or a site,
looks only among the logins on for the bot (any other answers like a missing
login), refuses a site that matches several and lists their labels and ids, then
types it into the focused field through the screen helper and returns only a
short "typed" note, never the value. Before typing, the helper's `focusedField` reports the front app, the
page URL (macOS AX `AXWebArea`/`AXURL`, Linux AT-SPI `DocURL`) and whether the field
is a password field. The host refuses unless the page host is the site or a
subdomain (or, without a page, the app name equals the site), and types a password
only into a password field. Clients manage logins with `credentialLogins`,
`credentialSaveLogin` (`label` optional), `credentialLabelLogin` and
`credentialRemoveLogin`; no endpoint returns a password.

Quando um site pede segundo fator, o bot chama `request_login_code {login,
reason}` com um login já ligado para ele. O cartão (`connectionRequest.kind` =
`code`) carrega o site e o título do próprio login vindos do Hub, nunca texto
digitado pelo bot. O usuário digita o código no cartão; o Hub aceita só 4–10
dígitos, guarda o valor somente na memória — nunca no cofre, no store, na
entrada, em logs ou traces — vinculado a bot + login + a sessão que pediu,
uso único e válido uma vez por cinco minutos (o varredor descarta mesmo sem
`take`), e marca o cartão como pronto sem o valor. O bot é retomado com uma
nota que só diz que o código está pronto para `type_login` `field: "code"` —
ele recusa quando não há código pronto e válido. `type_login` com `field:
"code"` faz a mesma conferência de foco de site/app dos outros campos, mas não
exige campo seguro (campos de código normalmente não são); em vez disso o
controle em foco precisa provar ser curto e numérico: `autocomplete=one-time-code`
ou `type`/`inputmode` numérico, com tamanho declarado curto quando exposto.
Papel ausente, papel/subpapel de busca, `inputmode=text`, maxlength/size
maior que 10, tipo claramente não numérico, multilinha/contenteditable e a
falta de metadados recusam — palavras de rótulo sozinhas não autorizam, e a
plataforma que não expõe a prova manda a pessoa digitar no site (uma caixa de
comentário ou chat no mesmo domínio falha). O foco é reconferido já sob a
exclusão do consumo, imediatamente antes da digitação: se mudou, recusa sem
gastar o código. Então o código e o Enter saem na mesma ação pelo caminho
secreto: só eventos de input, sem screenshot, sem árvore, resposta fixa — o
código nunca volta no resultado da ferramenta, mas depois de digitado pode
aparecer na tela como qualquer texto digitado, e erros do envio viram
mensagens fixas em pt-BR sem eco do valor. Toda recusa responde em pt-BR
curto e pede para a pessoa digitar o código no site. Cancelar o cartão,
remover o login, expirar ou uma conclusão que falha descartam o código sob a
mesma trava do consumo, e um Hub reiniciado não tem nenhum. A marca
`{{senha:` é recusada no `login` de `request_login_code` como nos outros
argumentos de login. Apps antigos mostram o cartão `code` como pedido
manual/desconhecido sem botão funcional até a tela de código dedicada
chegar; o terminal o lista como indisponível. `MIRROR_SCHEMA` é 40: o pedido
carrega a `origin` da sessão para que outra sessão não gaste o código, e o
valor nunca chega ao mirror; `connectionRequest` continua viajando como dado
de entrada opaco.

Terminal password prompts (sudo, ssh) are not covered: they aren't password fields
to the accessibility API. The same-user boundary above applies.

## Optional 1Password

Credentials settings accepts a service-account token and checks it using the
installed `op` CLI. Use a dedicated vault and grant the service account access only
to that vault. Codync does not create a 1Password account or vault automatically.

Use a 1Password vault reference (`vault/item/field`) as an environment value or complete header
value. The host resolves it when verifying/launching a local connector or preparing
a remote request. The token goes in the child environment, not command arguments;
output is consumed by the host and errors do not echo provider secret values.
Disconnecting removes the active token; connectors using references then require
reconnection. OAuth remains the preferred flow for services that support it.

## Validation

The host tests cover authenticated encryption, record substitution, plaintext migration,
locked storage, secure request completion, retry idempotency, cancellation and
failed verification. The ignored platform_keyring_roundtrip test is for an actual
unlocked OS credential store. Run it explicitly in a disposable Secret Service
session on Linux or the macOS login keychain.

## Contas e permissão por bot

Cada conta de serviço é um conector independente, ligado bot a bot pelos interruptores existentes. Ligar o conector X no bot A não permite usar o conector Y, mesmo que o bot informe o ID de Y.

Contas que a integração antiga do Composio guardava (`composio-<app>-ca_<conta>`) não existem mais: a migração única da partida do Hub (`host/src/fork/composio_retirada.rs`) apagou o registro `composio` do cofre, tirou esses ids das listas dos bots e avisou o dono uma vez. Conectores MCP por URL, incluindo `composio-mcp`, mantêm o caminho normal.

## Runtime MCP remoto mediado

A ponte remota e o getter legado connectorTarget exigem contexto emitido pelo Hub,
ID de ponte e método MCP. Getter sem contexto não entrega URL/headers secretos.
Caller::Local continua obrigatório; Device não é promovido a Local. Isso preserva
a fronteira de confiança local existente, sem prometer isolamento entre processos.
Verify emite setup temporário (120 s, até 16 verificações ativas), sem bot fictício
ou seleção automática; só handshake/ping/descoberta, nunca tools/call.
O handshake valida a versão MCP pelas regras de
[versões por ponta](marketplace.md).

A sonda de login usa a oferta 2025-06-18, remove `MCP-Protocol-Version` e
`Mcp-Session-Id` configurados e só tenta o GET legado após 404/405 (não após 400).
Quando o initialize responde 200 sem dica e os well-known não existem, o ping não
autenticado continua sendo o caminho até a URL de metadados indicada no 401. Um 200
da sonda não prova handshake, versão nem prontidão.

O runtime usa o mesmo cofre/cache do stdio. Headers privados e tokens OAuth
conhecidos são registrados para redação mesmo quando curtos ou sem dígitos. Leases
conservam valores antigos e atuais durante refresh; perda de cache não recria uma
lease incompleta para liberar raw. URL/headers têm impressão privada por ponte:
alteração durante resolução ou enquanto o retorno está em voo retém o conteúdo.
Remoção revoga contextos e leases. O vínculo privado OAuth descrito abaixo também
impede que reconexão de mesma URL herde uma lease da concessão anterior.

Antes de entregar descoberta/resultado, o Hub grava recibo e exposição;
falhas de cofre/cache/observação/persistência retêm o retorno. SSE e corpo de POST
legado também são mediados. Não seguir redirects e conferir a origem do endpoint
legado evita enviar Authorization a destino derivado não conferido. Diagnósticos
próprios não incluem URL/query, corpo de erro externo ou valores de headers.
O contrato é mediação de retorno, sem aprovação de efeitos ou cobertura de
terminal e ferramentas nativas do motor.

ACK textual 2xx fica retido/observado; GET legado segue vivo com
leitor único/canal 1 entre pedidos e atende ping/recusa só após observe. Orçamento
SSE é por pedido/resposta (64 frames de dados/256 KiB), com limite fixo por frame
64 KiB e proteção temporal de 64 eventos/256 KiB/oito reversos por janela de 1 s.
Comentários contam na taxa/wire, sem consumir frames de resposta. HTTP recuperável
no Streamable conserva a ponte sem retry; falhas de autoridade/segurança ou estado
legado incerto encerram. Cursores inseguros geram erro explícito, nunca fim de lista.

Limites conhecidos de redação: senha decodificada de Basic e segredos em segmentos
do path de URL não têm extrator genérico. O header Basic inteiro é
um valor conhecido, mas sua senha separada pode não ser reconhecida. Não foi
adicionada heurística de path que redija mcp/messages indiscriminadamente. A coleta
conservadora dos valores de headers/query pode causar redação excessiva para
literais curtos/comuns, como true; não foi relaxada por heurística que pudesse
liberar tokens alfabéticos. Isso é limite explícito, não cobertura integral.

## Dados privados de uma tentativa curada

Argumentos e apresentação integral pendentes usam o cofre canônico `vault:v1`,
com AAD do slot privado da tentativa. O selo é produzido fora do lock SQLite;
ciphertext, ledger, vínculo de cartão e projeção pública entram numa transação.
Abrir copia o ciphertext, libera SQLite, abre o cofre e reconfere contexto,
concessão, revisão da operação e ciphertext antes de devolver conteúdo privado.
Não há tokenstore paralelo nem fallback plaintext neste caminho.

Terminar apaga o KV privado; chave de ingresso consumida, digest e recibo seguro
permanecem no ledger, sem cascade de chat ou TTL Guard. Reiniciar expira espera,
cancela autorização não consumida e marca envio pendente como incerto; não reenvia.
Sucesso reportado já persistido sobrevive à falha de Guard, apresentação ou ACK.
Rotação de token não equivale a mudança da geração/perfil/revisões lógicas.
O snapshot copia `Binding.generation` como UUID opaco em String, sem normalizar,
hash, truncamento ou contador paralelo. `configRevision` corresponde exatamente
a `Binding.revision` u64; zero inicial é legítimo. Ausência, tipo inválido e
mudança da geração original continuam recusados; não se converte protótipo numérico
em uma identidade nova.

**Conta de saída.** A fonte canônica informa de qual conta
a ação sai: um vínculo opaco (ID canônico e encarnação, que muda a cada repareamento)
e a identidade legível (número do WhatsApp, e-mail ou login) com um rótulo. Só o
vínculo opaco vai para o ledger; a identidade legível fica em memória no Hub e no
privado cifrado do mesmo slot `vault:v1`, nunca em Entry pública, voz, logs ou recibo.
Não se guarda hash da identidade, que seria reversível por força bruta em números
de telefone. Sem identidade da fonte, a tentativa nem é criada. Identidade ou vínculo
diferentes da captura fecham leitura, decisão e admissão: o cartão da conta A nunca
autoriza a conta B. App, modelo, import, argumentos e rótulo do bot não declaram a conta.
O produtor do número do WhatsApp ainda não está integrado.

HumanReviewProof tem produtor único próprio, ainda não integrado
nem com API positiva estável aceita. Local, `source=app`, scope e posse da conta
não comprovam aprovação de pessoa. Na integração futura, decisão validará o
binding/nonce/revisões exatos; a prova será revalidada e consumida uma única vez
na mesma transação SQLite da admissão do efeito do Conector, com rollback conjunto.
O callback será síncrono, só na conexão recebida, sem Store/Cloud, rede ou await.
Isso não dá atomicidade ao POST HTTP. Contexto/Authority originais serão conservados
após awaits; o prazo original de 90 s não herda 120 s nem renova em edições. O compartilhamento de confirmação entre
aparelhos só poderá acontecer com vínculo exato demonstrado; sem ele, duas etapas.
Esta etapa não produz prova nem é backend genérico de aprovação para outros domínios.

Em produção, ausência da fonte, da apresentação ou da integração da prova comum
fecha o caminho antes de cofre/rede/cartão. Este caminho não lê binding OAuth novo nem ativa contas. O contrato de
timeout de um motor real ainda precisa ser comprovado por backend/versão antes
de sua admissão; o teste usa somente `fixture-mcp-300` e seu transporte sintético.

## Login MCP vinculado à instância

O Hub conserva identidade privada permanente por instância, geração do login e
revisão da configuração no mesmo registro cifrado dos tokens. Remover e recriar o
mesmo nome/URL gera outra instância. Reconectar encerra a concessão anterior;
refresh legítimo só rotaciona o par de tokens. Registros legados recebem vínculo
antes do primeiro uso, preservando login e seleções existentes.

Se remoção, configuração, reconexão ou autoridade mudarem durante discovery,
registro ou troca do código, a resposta antiga é recusada sem alterar o substituto.
Ao alterar um header de remoto que usa OAuth, o login fica `signedOut`: tokens e
cadastro anteriores são descartados, e a opção de entrar novamente permanece.
No início do login, se algum conector ou login mudar enquanto o cofre é desbloqueado, a tentativa
pendente é cancelada e pede uma nova intenção explícita. Isso inclui uma renovação
legítima durante essa espera; a tentativa antiga não altera a conta atual nem
obtém URL dela. Não há repetição automática nem recuperação silenciosa.
Essa proteção inicial é transitória: uma renovação legítima continua preservando
o vínculo e os contextos existentes. Pedido recusado por alvo/redirect inválido
preserva o login anterior; falha de commit também conserva o pending original,
sem gravar parte das credenciais ou invalidar o estado fora da transação.
O usuário inicia um novo login. A geração anterior perde seus contextos de proxy e
setup; um bridge novo também não os recupera. Após reconectar, iniciar explicitamente
uma nova sessão do bot e uma nova intenção faz o ator emitir outro contexto,
conservando apenas as seleções atuais. O login não faz essa recuperação automaticamente.
Callback expira e só pode ser usado uma vez.
O operador original continua obrigatório após filas/rede; uma autorização nova
não recupera o pedido antigo. Para Device account, tanto o prazo capturado quanto
o atual precisam continuar válidos; renovar lease não prolonga a intenção antiga.
Device QR continua Device no contrato Control legado. Nenhum desses fluxos concede
acesso curado ou permissão de contas de IA, nem liga conectores para bots.

O proxy HTTP também conserva o Context/Setup emitido originalmente pelo Hub.
Se esse vínculo for revogado durante desbloqueio, filas ou rede, a operação antiga
não renova nem encerra o login de uma conta substituta, mesmo com ID/URL iguais.
O alvo esperado permanece fixo; só o par rotativo atual da mesma instância,
concessão e configuração pode ser adotado. A seleção atual do bot, a conversa/run
e o prazo original de setup continuam obrigatórios. Isso não emite outro contexto,
não reautoriza a ponte antiga e não elimina os segredos anteriores ainda necessários
à redação de resultados em voo. A regra conservadora sobre qualquer gravação no
registro durante unlock aplica-se ao início do login, não vira identidade runtime.

Instâncias atuais continuam legadas sem perfil. A admissão curada antes de abrir o
cofre e os callbacks do catálogo futuro ainda não estão disponíveis.


Na composição com o caminho curado interno, que continua sem fonte/apresentação
produtivas disponíveis, o produtor próprio conserva também o ID original do
binding. Sua guarda não procura um ID novo depois dos awaits, mesmo se a ACP for
a mesma. Alvo, headers e efeitos exigem esse binding vivo; recibo e exposição
validam na mesma transação. O campo fica no contexto privado, sem aparecer nas
projeções públicas. Recibo terminal antigo permanece legível, mas ausência do
binding original não concede execução. A autoridade própria do curado, a fonte,
o grant e o produtor único de aprovação humana permanecem separados do legado.

## Renovação de login MCP

Se o serviço estiver temporariamente indisponível, o login e o par de credenciais
continuam salvos. Tente novamente em instantes. O Hub só usa o acesso anterior se
souber que ele ainda não venceu, ele não tiver sido recusado com 401 e a mesma
concessão, configuração e autoridade original continuarem válidas após filas e rede.
Token vencido, sem prazo conhecido nesse fallback ou já recusado não é enviado.
Um erro inconclusivo também conserva as credenciais, mas recusa o uso do token.
Não há repetição automática de ferramentas, novo login ou liberação de bots.

Só `invalid_grant` confirmado na resposta de protocolo encerra a concessão
original; entre novamente pelo fluxo seguro. `invalid_client` confirmado também
invalida o cadastro daquela instância para o próximo login explícito. Outras contas
não são alteradas. Timeout, rede, 408, 429 e 5xx conservam o login, mesmo com corpo
`invalid_grant`; mensagens desconhecidas não provam revogação. Diagnósticos não
incluem erros livres do provedor, corpos, URL/query, tokens nem credenciais.

A renovação usa exatamente o `resource` guardado da concessão. Legados sem esse
campo continuam omitindo o parâmetro como antes, sem deduzir pela URL, fazer nova
descoberta ou fabricar concessão; isso não promete que todo servidor aceite o
legado. Um resource presente vazio ou inválido causa erro sem fallback. A rotação
legítima mantém identidade, geração, revisão e contexto e grava access/refresh
juntos. Se o acesso novo já vier vencido (`expires_in=0`), o par novo é conservado
atomicamente para não perder o refresh rotativo, mas nenhum header desse acesso é
entregue. Um acesso novo sem `expires_in` conserva o contrato OAuth normal; sua
validade desconhecida não permite um fallback temporário futuro. Respostas antigas
não sobrescrevem nem apagam um par mais recente, inclusive após recriação de ID/URL.

O prazo é conferido novamente na última entrega do Hub, depois das guardas de
autoridade e dos locks. Essa conferência pertence ao acesso que de fato está no
header da resposta, mesmo se o registro já tiver outro par rotativo. Se o acesso
vencer nesse intervalo, o Hub devolve erro sem headers e conserva login, par atual
e segredos anteriores necessários à redação de resultados em voo. Cabeçalhos
manuais sem OAuth mantêm seu comportamento; nenhum testemunho de prazo sai ao app.

Se uma resposta de renovação com HTTP 200 trouxer refresh vazio ou malformado, ela é
inconclusiva: nenhum acesso é entregue e o par salvo fica intacto. A ausência do
campo refresh continua opcional e conserva o refresh anterior no sucesso normal.

## Identidade do recurso no login MCP

Antes de cadastro do app, nova concessão ou link, o Hub exige PRM cujo `resource`
seja exatamente a identidade esperada para aquele candidato. Hint espera a URL
chamada; well-known por caminho preserva query e `/mcp/`; raiz espera sua origem,
sem assumir `/mcp`. Documento divergente é descartado inteiro. Sem candidato válido,
o erro fixo é "O serviço não confirmou a identidade do recurso; login recusado.".
Só o provedor corrige os metadados; não peça segredos nem contorne por outro endpoint.
O início da reconexão pode já ter invalidado a concessão e limpado tokens; a recusa
não restaura o login anterior. O valor aceito acompanha a mesma tentativa, Grant,
link e troca do código. A renovação descrita acima conserva esse mesmo Grant;
legado sem Grant não ganha resource nem nova descoberta. A sonda 401 continua
pedindo login, mas metadata sem resource conferido não comprova OAuth na sonda 200.
Veja o contrato e os limites de compatibilidade em [marketplace](marketplace.md).

No login novo, `resource` preexistente na query de autorização ou token (inclusive
chave codificada/repetida) recusa antes de DCR/Grant/Pending/link com "O servidor de
login informou um recurso adicional; login recusado.". Só o provedor corrige a query;
outros parâmetros legítimos permanecem. Essa guarda não altera refresh legado.

## Permissões pedidas no login MCP

Ao iniciar um login, a Mexerica faz uma sonda `initialize` sem credenciais na
mesma tentativa. Só `scope` de um desafio Bearer HTTP 401 válido tem prioridade
sobre `scopes_supported` do PRM validado. Pode indicar menos, mais ou outras
permissões; não se exige relação entre os conjuntos. Sem esse desafio, conserva
o fallback pela metadata; ausentes ambos, omite `scope` (conjunto desconhecido,
não confirmação de permissões vazias). O pedido acompanha o mesmo Grant e o
snapshot pendente; não é recalculado no callback.

Permissões vazias, inválidas, repetidas como parâmetro ou conflitantes entre
desafios recusam antes de cadastro do app, Grant, pending ou link. Query `scope`
preexistente nos endpoints de autorização/token, inclusive chave codificada ou
repetida, também recusa. Outros parâmetros permanecem. Erro fixo: “O serviço
informou permissões inválidas ou conflitantes; login recusado.” Só o provedor
corrige esses dados; não peça segredos, amplie permissões ou repita sozinho.
O início da reconexão pode já ter invalidado o login anterior, sem restauração.

Limites: declaração remota não é perfil curado nem aprovação humana. A seleção
mínima por perfil continua pendente. A validação da declaração de escopos do
par OAuth está descrita abaixo. Esta sonda não faz GET legado;
401 disponível somente nesse GET não orienta o pedido. `required`,
ping e fallback SSE M1 existentes continuam; nenhum `tools/call`, step-up
automático, seleção de bots ou mudança da versão MCP foi adicionado.

Fonte: [MCP vigente, Scope Selection Strategy](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization#scope-selection-strategy),
sintaxe de scope conforme RFC 6749 §3.3.

## Escopos declarados no par OAuth

A Mexerica valida o `scope` declarado na resposta de login antes de salvar
access, refresh e expiração juntos. Em nova tentativa com pedido explícito não
vazio, exige o mesmo conjunto: ordem e repetição não mudam o resultado, mas a
caixa de cada nome é exata. Ausência de `scope` é aceita quando a RFC 6749 §5.1
permite conservar o pedido. Campo presente nulo, de outro tipo, vazio ou com
sintaxe inválida recusa. Extras, faltas e conjuntos alternativos também recusam:
essa igualdade estrita é política do piloto, não exigência genérica do OAuth.

Pedido omitido/[] e logins antigos sem validação continuam desconhecidos, sem
confirmar permissões vazias nem transformar `Grant.scopes` em prova. Uma resposta
inicial explícita válida sem pedido conhecido registra somente o conjunto declarado,
sem inventar intenção ou perfil. No refresh de uma fonte com conjunto validado ou
declarado, a ausência conserva o conjunto original e a presença exige igualdade.
Refresh legado não confirma retroativamente a concessão inicial. O pedido de
refresh continua sem scope; sem Grant continua sem resource, sem reconstruir pela URL.

Um candidato a novo par com escopos inválidos ou incompatíveis não salva tokens
nem usa o par anterior como fallback. O login fica indisponível localmente até nova
entrada explícita, inclusive depois de reabrir o Hub; par e evidência anteriores
são preservados. O app oferece Entrar novamente e a fonte fica fora do ACP enquanto
retida. Isso não declara logout nem revogação no serviço. A mensagem fixa é:
“O serviço informou permissões inválidas ou diferentes das esperadas. Entre
novamente no conector em Extensões.” Oriente o usuário a entrar pelo app, sem pedir
segredos, ampliar permissões ou repetir ferramentas/efeitos automaticamente.
Erro temporário de rede/serviço conserva a política anterior e não vira retenção
por conter scope inválido. Autoridade original e vínculo continuam obrigatórios.

Limites: esta validação confere a declaração OAuth ou sua omissão permitida;
não introspecta o token, não comprova poderes reais no servidor, aprovação humana,
perfil local ou privilégio mínimo por ferramenta. Perfis e step-up continuam
pendentes. Não há campo público novo nem mudança de interface.

Fontes: [RFC 6749 §§3.3/5.1/6](https://www.rfc-editor.org/rfc/rfc6749.html),
[RFC 6750](https://www.rfc-editor.org/rfc/rfc6750.html) e
[MCP vigente, autorização](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization).

## Conversas adicionais próprias

Conectores MCP já ligados ao bot também podem ser usados na sua conversa
adicional do mesmo Space, por stdio ou HTTP. `list_connectors`,
`request_connection` e `request_secret` conservam essa origem; não ligam outro
conector. Foreign continua sem ferramentas. Logins/computer e gestão
ficam para etapas futuras.

A execução de origem é a emitida pela sessão (`ExecutionRef` do vínculo de sessão);
o `ToolContext` só acrescenta, sobre ela, a cerca da conversa adicional. Não há
outra fonte de lane, sessão ou autoridade. A definição fixa a geração e cada pedido conserva a captura de conversa,
sessão, turno e permissão através do cofre, renovação e retorno. Uma captura
expirada não é refeita por repetir o ID. Handshake/descoberta anteriores à
ativação ACP não autorizam ferramentas nem leitura de conteúdo. Cartões de
setup exigem a mesma geração, sessão e turno antes de acessar o cofre, gravar,
confirmar e retomar; confirmação e avisos ficam na origem capturada. Novo turno
ou sessão exige nova intenção. A inspeção e referências de credenciais do gateway
continuam; isso não acrescenta isolamento ou aprovação de efeitos. Resultado
incerto exige conferência antes de tentar novamente.

## Cadastro OAuth vinculado ao emissor

O Hub só reutiliza um cadastro OAuth quando ele próprio vinculou sua emissão ao
mesmo `issuer` original e exato, e os redirects desejados já estão registrados.
ID igual em dois emissores não identifica o mesmo cadastro. Troca de emissor ou
cadastro legado sem esse vínculo exige novo cadastro automático (DCR) no próximo
login; sem suporte, a conexão é recusada com mensagem fixa em português, sem URL,
ID, segredo ou corpo remoto. Não tente outro servidor, não contorne a recusa nem
peça credenciais no chat. Quando o serviço não oferece DCR, use no app uma conexão
com cabeçalho de token pelo fluxo seguro existente. Falha de cadastro não autoriza
repetição automática. O início do login pode já ter limpado os tokens anteriores;
a recusa não restaura a conexão anterior.

O vínculo do cadastro é privado, separado da concessão e das evidências de escopo.
Não se deduz sua origem pelo Grant antigo, URL atual, nome ou metadata de hoje.
Logins ativos e refresh legados não são invalidados em massa nem ganham prova de
origem retroativa; a exigência vale para o novo login. Sair/reconectar conserva o
vínculo enquanto conserva o cadastro; `invalid_client` e mudanças que apagam as
credenciais também apagam esse vínculo. Isso vale para `invalid_client` tanto no
retorno do login quanto no refresh; só uma nova intenção explícita tenta novo cadastro,
sem repetir código ou efeito. Importação/criação não aceitam selo do JSON.
Refresh válido conserva vínculo e revisão. Endpoints legítimos em outra origem
continuam permitidos: a identidade é o issuer verificado, não a origem física do
endpoint. DCR não envia cadastro/segredo/headers antigos; 3xx é recusado sem seguir
Location. Cadastro e issuer novos são gravados juntos com a autoridade e o snapshot
originais revalidados após rede, antes de persistir a tentativa ou entregar o link.

Isso não comprova bloqueio completo de egress nem OAuth comercial pronto. CIMD,
step-up, perfis mínimos e a prova humana comum continuam
posteriores. Fonte: [MCP, vínculo ao servidor de autorização](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration#authorization-server-binding).

## Perfis preparatórios Linear

A capability Local do Hub agora inclui catálogo preparatório com **Leitura**
(`/mcp/readonly`, `read`) e **Leitura e criação** (`/mcp`, `read write`). A consulta
não abre o cofre, cria instância, inicia OAuth ou seleciona perfil numa conexão.
`scopesPrevistos` não são escopos concedidos; o fluxo OAuth legado não ganhou
mínimo por perfil. Catálogo continua com conexão indisponível e sem acessos.
`read write` permite mais escritas no serviço do que a criação mínima pretendida;
a futura curadoria de ferramentas/schema deverá restringir o uso.

URL, nome, client_id e workspace presumido não identificam a conta de saída.
Somente uma fonte confiável poderá produzir essa identidade. Nenhum token ou
Binding adicional é criado, nem uma conexão manual vira curada pela URL.
Veja [catálogo preparatório](marketplace.md).
## Contas do Marketplace: Space, escopos da ficha e desconectar

Cada conta do Marketplace é um conector com o **próprio registro no cofre** (tokens OAuth, cadastro DCR, chave em cabeçalho): nada é compartilhado entre contas do mesmo serviço nem entre Spaces. **Desconectar** uma conta (`marketContaDesconectar`) apaga só o registro dela; as outras contas ficam byte a byte como estavam. O desconectar não revoga nada no fornecedor e o Hub não diz que revogou (`revogadoNoFornecedor: false`).

Escopos: conta criada pela ficha pede no login **somente** os escopos dos grupos ligados (ex.: Linear Ler = `read`), ignora o desafio `WWW-Authenticate` e a lista `scopes_supported` inteira, e o login é recusado se o servidor não oferecer algum escopo da ficha. O que o servidor devolve é conferido contra o pedido pelas regras de escopos declarados. Conta sem ficha (MCP do próprio usuário) segue o caminho de permissões pedidas no login. A URL, o tipo e as liberações da conta vêm do catálogo; URL diferente da ficha ou ficha removida falha fechada.

Space: o campo `space` da conta é conferido ao ligar a conta num bot, ao renomear/desconectar e a cada entrega de credencial ao proxy (`oauth_runtime`): credencial de conta de outro Space não sai do Hub. Cada ação sobre a conta gera um recibo opaco (`mexerica_conta_recibos`), sem nome, token, cabeçalho nem endereço. Chave em cabeçalho: a conta de ficha nasce com o cabeçalho vazio, o valor entra pelo cartão seguro/`credentialUpdateConnector` e, sem valor, a conta não vira servidor MCP. Detalhes e API: [marketplace.md](marketplace.md).

## Rascunho nativo WhatsApp — preparo sem admissão

Esta etapa oferece `preparar_resposta_whatsapp` no roteador
nativo, com `ExecutionRef` original e mensagem canônica. Usa leitura existente,
conta de pareamento, telefone confirmado e snapshot das concessões. Não abre
a fonte HTTP/MCP global, `available()`, `decisionProtocol` ou decisões existentes.
Local, clique, bearer e rótulo de ferramenta não são prova humana.

O rascunho tem Entry de aviso com `conectorDraft`, recibo mínimo em KV e privado
no cofre existente. `donnaConectorRascunho` é somente leitura Local. A fonte viva
é retida em custódia opaca por até 90 s; leitura depois do turno é acesso a dado,
não autoridade. Após expirar ou perder conta/conexão/concessões, o privado é
apagado; startup fecha os sobreviventes sem reconstruir a fonte. Não há edição
ou decisão nesta etapa. O desktop mostra “Rascunho de resposta — aprovação
indisponível”, incluindo “Resposta a” fixa e marcada externa.

A escrita nativa abre uma única transação na Connection do Store e conserva a
trava WhatsApp DB → bots até terminar commit/rollback. Cifragem/abertura ficam
fora do DB. Não há transação aninhada ou chamada a fila/pace, autorização ou
transporte. A adaptação completa proxy/ledger para StoreTxB fica para uma etapa futura.
