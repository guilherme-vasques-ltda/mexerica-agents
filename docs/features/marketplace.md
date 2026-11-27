# Extensions and agent setup

The Extensions screen configures capabilities installed on the selected computer. Account membership does not install the same capability on every computer.

**Na Mexerica há um Hub só:** tudo o que se configura em Extensões vale para todos os bots e aparelhos. No desktop, o subtítulo da tela é **Hub** (não o nome da máquina) e o seletor **Trocar de computador** só aparece quando o app conhece mais de um computador (contando os offline; dá para trocar entre os online); a lógica de vários computadores do host não mudou.

## Ligar uma extensão em todos os bots

Além da escolha por bot (que continua sendo a base), o Hub oferece o lote "ligar em todos" para cada skill e cada conector instalados, em dois passos:

1. **Prévia** (`extensaoEmTodosPrevia`): devolve os bots que receberiam a extensão (`recebedores`, com `botId` e `nome`), os que ficam de fora com o motivo (`excluidos`: `apagado`, `grupo`, `oculto`, `ja_tem`, `fora_do_pessoal` para skills, `principal_de_space` para conectores) e um `versao` opaco que fixa o estado observado — extensão instalada, lista de bots, regras vigentes e, para conectores, um digest dos registros privados do cofre (argumentos, env, headers e autenticação, sem expor valores): trocar um segredo mantém a projeção pública igual, mas muda a `versao`. A prévia roda sob a autoridade original do aparelho do dono — revogação durante a espera da fila falha fechado.
2. **Aplicação** (`extensaoEmTodosAplicar`): recebe `botIds` (exatamente os confirmados na prévia) e a `versao`. Numa única transação, sob a mesma autoridade reconferida, o Hub refaz a leitura do estado e compara com o da prévia — inclusive os cofres dos conectores — e só então grava. Se qualquer coisa mudou desde a prévia — bot criado, apagado, ocultado, extensão desinstalada ou substituída, argumentos ou credenciais do conector trocados, lista de bots alterada, seleção de skills inválida — a resposta é `extensoes_em_todos_conflito` e **nada** é gravado; a tela refaz a prévia. A seleção segue a mesma régua da célula única: lista malformada ou versão de seleção ausente/inválida recusa o estado em vez de descartar entradas.

Só o dono aplica: aparelho sem Control e os transportos Ponte/Pairing/Bootstrap são recusados, e a escrita passa pela mesma marca de caminho do dono (`como_dono`) e pelos mesmos gatilhos do `updateBot`/seleção de célula — inclusive a regra da principal de Space, que nunca recebe conector (persona e poderes fixos). Extensão que não está instalada ou não tem conteúdo verificado é recusada com `extensoes_em_todos_indisponivel` na prévia; se sumir entre a prévia e a aplicação, vira `extensoes_em_todos_conflito`. Nenhum caminho tem efeito parcial.

### Novos bots já vêm com… (só skills)

O padrão para bots novos existe **só para skills**:

3. **Padrão** (`extensaoNovosBotsDefinir {protocol:1, tipo:"skill", id, ligado}`): grava no Hub, por id de skill, se os bots criados depois já vêm com ela. `tipo:"conector"` é recusado com `extensoes_novos_bots_recusado` — conectores, contas e logins nascem desligados e só ligam por "Ligar em todos os bots" ou bot a bot. O padrão exige a skill instalada e com conteúdo verificado; a lista de skills (`skills`) expõe `novosBots: true` nos itens ligados para a tela.

Na criação (`createBot` do dono), o padrão entra só quando o corpo não trouxe `skills` (ausente ou nulo — uma lista explícita, mesmo vazia, é escolha do dono e dispensa o padrão), o bot é agente do Space pessoal e não é grupo. Cada skill do padrão é reconferida no catálogo na hora: desinstalada ou sem conteúdo verificado é ignorada, sem erro (fail-closed). A gravação usa a mesma `CreationSelection` da seleção explícita, com a autoridade original já capturada — nenhum poder novo entra. Bots criados por ferramenta de outro bot/Times não passam por esse caminho: herdam só a seleção explícita do criador, como antes.

## Páginas de Extensões (desktop)

Extensões é uma **tela**, no mesmo molde de Ajustes: Voltar e Esc voltam ao que estava antes, e a lista à esquerda tem **Conectores** e **Skills** (e Plugins e Credenciais). **Aplicativos** deixou de existir como página (era a mesma coisa que Conectores; só tinha um aviso sem catálogo e o atalho "N instalados", que repetia Minhas contas). A faixa (ícone de grade) e a engrenagem abrem **Conectores**; **Skills** abre na engrenagem. Em Conectores, o botão **Adicionar conector personalizado** fica no canto superior direito do cabeçalho e abre a folha de Adicionar MCP, que começa com o aviso "Seu conector, não verificado pela Mexerica". O texto abaixo descreve o conteúdo de cada página; as "abas" de antes são as páginas da tela.

Os agentes (harnesses do registro ACP) se gerenciam em **Ajustes → Contas de IA**, na seção **Motores no Hub** de cada Hub (instalar, Node.js e entrar, com a mesma folha e o mesmo terminal assistido de antes); a aba Agentes de Extensões, que repetia esse catálogo, saiu. **Meus MCP** são as configurações nativas adicionadas por URL, comando ou config MCP. O fluxo de adicionar MCP é o mesmo de antes. Antes da tela (folha), havia duas abas, Aplicativos (com Donna Apps e Meus MCP dentro) e Skills, e a faixa e a engrenagem caíam ambas em Meus MCP; depois, Aplicativos e Conectores viraram só Conectores.

O botão **Credenciais** e o atalho **N instalados · Conectores e skills** ficam no cabeçalho de cada página. A busca vale para a página aberta: Meus MCP filtra localmente as configurações salvas e Skills filtra a lista na hora. Na tela, a página aberta é a do atalho, e cada página começa do zero ao ser aberta (a busca não fica de uma página para outra). O app ainda grava a última página em `localStorage.marketplaceTab` (IDs anteriores preservados; `agents` vira Aplicativos), mas a tela não a usa para escolher a página.

## Composio (integração dedicada removida)

A integração dedicada do Composio foi apagada: não há mais chave de projeto, usuário, catálogo de apps, contas conectadas, apelidos, `select_account`, servidor MCP embutido `composio` nem os métodos `composio*`/`setComposioKey` na API. O Composio passa a ser uma ficha comum do catálogo de conectores, conectada pelo MCP oficial dele, como qualquer outro MCP.

**Migração única na partida do Hub** (`host/src/fork/composio_retirada.rs`, marcada em `migration.composio-retirado.v1`): numa única transação, apaga só o registro `composio` do cofre (chave, configuração, cache de contas e apelidos), sem tocar no resto do cofre e sem ler ou imprimir segredo; tira das listas de conectores dos bots (inclusive o principal, pelo caminho do dono e com os gatilhos de proteção ligados) os ids de conta `composio-<app>-ca_<conta>`, mexendo só em `connectors`; e grava um aviso único na conversa principal de um bot do dono: "O Composio agora é um conector comum; conecte de novo pelo catálogo se quiser." Um MCP comum salvo pelo dono, mesmo com id `composio-…` sem o trecho `-ca_`, continua ligado. Instalações sem dados do Composio não recebem aviso. A marca e o aviso entram na mesma transação: se a migração falhar ou o processo cair no meio, nada fica gravado e a próxima partida repete tudo, inclusive o aviso; a falha é só registrada no log e não impede o Hub de iniciar. Depois da marca, a migração nunca roda de novo.

The desktop and iPhone installed count is labeled **Connectors & skills** (no desktop da Mexerica, **Conectores e skills**), and opens a library with that same title. It includes connected apps (stored as connectors), but excludes agents. The desktop shows harness setup in Settings → AI accounts, under Motores no Hub; the terminal (TUI) retains its general harness catalogue. The terminal already separates these categories and does not show a combined installed count.

| Capability | Implementation | Behavior |
| --- | --- | --- |
| Agents | `host/src/agent/backends.rs`, `host/src/agent/registry.rs`, `host/src/agent/node.rs` | Detect installed harnesses, resolve ACP registry adapters, provide Node.js for npm agents |
| Connectors | `host/src/market/mod.rs`, `registry.rs`, `install.rs` | Configure MCP servers manually by URL, command or pasted config; preserve existing connections; starts off for every bot, enabled by the user bot by bot |
| Skills | `host/src/market/skills.rs` | Texto passivo em pastas com `SKILL.md` no diretório de dados do Hub; associação explícita por bot, sem ativação automática ao instalar |

Install runs the CLI's own native installer where it has one, on Windows too (Claude Code, Codex, Grok, Qwen, Droid, Qoder, Cursor…, `Install` in `backends.rs`); CLIs published only on npm (Gemini, Cline, Auggie, Kilo…) install with `codync-host npm install -g <package>`, npm on the computer's own Node or else on Codync's. A CLI with its own ACP mode then runs as installed, without Node.js. Refreshing the agents re-reads the PATH a fresh login would get (the login shell's; on Windows the user's and machine's PATH from the registry), so a CLI found by an installer that just ran shows up without restarting the host. Many agents' ACP adapters run through `npx` (Claude, Codex, Gemini, Qwen…). When Node.js is absent, the Hub reports `node: "missing"` for adapters that need it. The terminal (TUI) and the existing setup service can add the pinned Node.js version (`agentSetup {step: "node"}` / `codync-host node`) under `~/.codync/node/<version>`, with global npm packages under `~/.codync/node/global`. Extensions no longer shows an agent list or setup sheet. Settings → AI accounts provides the Motores no Hub section, its Node.js notice and the existing assisted setup sheet for the supported harnesses. Codex and Claude Code sign in through the accounts above that section; other motors use their existing authentication methods. Both folders come last on the agent search path, so the computer's own Node and CLIs always win, and nothing outside `~/.codync` changes. Linux builds of Node need glibc: on musl distros (Alpine) install Node.js yourself.

**Extensões → Conectores** (Meus MCP) lista as configurações MCP nativas do usuário. A
listagem `connectors` traz só configurações MCP; Meus MCP usa todos os itens, sem filtrar `registryName`. Esse campo é procedência histórica
passiva. Conectores antes adicionados pelo catálogo continuam com os mesmos IDs,
credenciais, logins e seleções por bot/Hub/Space; não há migração ou novo login.

MCP Registry foi retirado: não há descoberta, busca pública, destaques, páginas,
cache de catálogo, aquecimento ou atualização de pacotes pelo Registry, inclusive
antes de iniciar conectores. Os RPCs legados `marketConnectors`, `connectorInfo`
e `installConnector` por `registryName` respondem `mcp_registry_unavailable:`.
Não existe opção para religar essas consultas. ACP, Skills e atualização
do produto são capacidades separadas.

O campo `configuration` informa `configured` quando existe alvo materializado
(comando ou URL) e `manualRequired` quando falta. Não prova funcionamento,
autenticação ou ferramentas disponíveis. Configurações incompletas continuam
visíveis; não são completadas por consulta remota. `auth` mantém o estado de
credenciais salvo, sem garantir validade atual. `connectorVerify` confere
handshake e ferramentas naquele momento. Falhas de cofre/listagem são erros,
nunca uma lista vazia. Timeout depois de uma escrita exige reler o mesmo Hub
antes de tentar novamente, sem repetição automática: só uma leitura **bem-sucedida**
da lista, pedida depois que a escrita terminou, conta como reconciliação; uma
releitura que falha mantém o resultado incerto.

**Na tela (desktop):** Meus MCP mostra as suas configurações com o alvo salvo
(comando ou URL) e selos honestos: **Configuração manual necessária** quando falta
o alvo (num Hub antigo, sem o campo, o app infere só pelo alvo público) e
**Precisa entrar** quando o servidor espera login. Nenhum selo diz "pronto": ligar
por bot e ferramentas disponíveis continuam separados. Cada linha tem **Entrar**
(quando precisa, pelo login nativo) e **Remover**; não há edição do alvo, e um
incompleto orienta a adicionar de novo por URL, comando ou config, sem apagar o
antigo. A busca filtra só a sua lista. Sem link com o Hub a lista some ("Sem
conexão com o Hub"); erro de leitura mostra **Tentar de novo**, nunca "nenhum
MCP"; troca de Hub, reconexão ou resposta atrasada não mostram dados da origem
anterior (Hub, link e geração conferidos antes de publicar). **Adicionar MCP**
é o único botão que abre o fluxo nativo (Comando, URL, Config), e fechar o formulário
devolve o foco a ele; a lista vazia só orienta, sem um segundo botão que sumiria ao
salvar o primeiro MCP; um resultado incerto (sem resposta do
Hub) avisa que não deu para confirmar e relê a lista do mesmo Hub. Um novo
Adicionar só volta depois de uma releitura **bem-sucedida** desse Hub, mesmo fechando e
abrindo o formulário; se a releitura falha, o formulário mostra o resultado incerto
com o erro e **Reler a lista**, que só lê e nunca repete o que foi enviado. Mudança
de conexão durante o gesto e MCP salvo com login falho continuam bloqueando o
formulário; depois de trocar de Hub, só **Reler a lista**, com o aviso na tela, libera
um novo Adicionar. Conectores e skills mostra a mesma lista.

A custom connector is a command line (split like a shell), a URL, or a pasted MCP config (`importConnectors {config}`: the `mcpServers` / `servers` JSON from READMEs, Claude, Cursor or VS Code, or one server's entry); every server in a config is added.

MCP connectors and tool accounts start off for every bot, including existing bots. Installing or importing does not grant access. A new bot gets only the connectors/accounts the user explicitly selects; Berga copies none. The user enables each account in the bot settings. Existing selections are preserved without migration. An enabled connector is passed to the agent as an MCP server when its session starts or resumes. Local connectors are spawned by the agent; remote ones always go through the host's stdio proxy (`codync-host mcp remote`, `host/src/mcp.rs`), so they work with agents that only speak stdio and always carry a fresh token. The proxy speaks streamable HTTP and falls back to the older HTTP+SSE transport when the first POST fails with 400/404/405, as the MCP spec suggests.

O catálogo/registro de Skills atual pertence a **Pessoal**. Em Space adicional, os
RPCs de lista, catálogo, detalhes, instalar/manual e remover retornam indisponibilidade
(`skills_personal_only`) antes de dados/contagens/metadados pessoais ou efeitos; não
representam uma lista vazia. O seletor explícito Pessoal só é adaptado após o guard
comum validar Hub/Space/bot/conversa e Caller; extras permanecem recusados pelos
contratos estritos. Skills por domínio continuam planejadas, sem ativação por esta tela.

Skills usam um catálogo passivo: instalar importa somente os arquivos verificados e não ativa a skill em bots nem concede ferramentas, execução, rede, contas ou segredos. O usuário associa os IDs instalados explicitamente nas configurações de cada bot, uma skill por vez: `updateBot {id, skillSelection:{protocol:1, expectedVersion, skillId, enabled, expectedInstallationId}}` grava só aquela célula em `bots.config.skills` se a versão opaca do bot (`Bot.skillSelection.version`) e a instalação ainda forem as vistas no gesto; senão recusa com `skill_selection_conflict` ou `skill_installation_stale`, sem retentativa. Perfil completo antigo que traga outra lista recebe `skill_selection_unsupported`; escritores internos de perfil nunca restauram a seleção de um snapshot. Bots criados pelo usuário começam sem skills; bots criados pelo Berga herdam somente a seleção explícita do criador. Seleções antigas são preservadas, sem nova migração de ativação global. Nome, identificador de origem, descrição e instruções mantêm o idioma original, inclusive quando o texto original é português; controles e avisos próprios da Mexerica continuam em pt-BR. O catálogo atual tem 29 entradas: 12 pacotes com SKILL.md original integral, duas adaptações textuais, 14 entradas indisponíveis com causa concreta (3 condições anteriores e 11 incompatibilidades) e a skill autoral criador-de-skills, sem fingir ferramenta pronta ou reescrever YAML para instalar.

Abrir uma skill mostra sua descrição, instruções passivas e, quando há recibo, a versão exata instalada. `skillDetails {id}` lê o registro e arquivo existentes; `{source}` consulta somente o bundle incorporado, sem download ou cache de branch mutável. O retorno contém `markdown`, `catalog`, `receipt` e `installation` (registro completo com `installationId` e `contentStatus` da mesma leitura; null na consulta do catálogo); detalhes da instalação antiga são vinculados ao catalogId+digest capturados, não à embalagem atual. Nova importação exige `{catalogId, expectedDigest}` do catálogo; URL/source remoto e sobrescrita por nome não são aceitos. Remoção exige ID instalado, `expectedInstallationId` capturado e expectativa explícita `expectedReceipt` (`none` ou `catalog` com catalogId/digest exatos). Só ID/digest antigo é recusado com `skill_installation_unsupported`; encarnação/recibo divergentes recusam com `skill_installation_stale`. Mesmo pacote ou manual idêntica reinstalada recebe outra identidade. Corpo divergente/ilegível recusa detalhes sem reparo automático; remover segue pela encarnação vista e a curada pode ser reinstalada pela versão do recibo. O consumidor de UI está implementado; composição, QA e instalação aplicáveis ainda são pendentes. Restore/invalidação de intenções antigas pertence ao fluxo BKP/MEM. A indisponibilidade futura do catálogo não apaga, traduz, renomeia ou desliga pacotes/seleções anteriores. A pré-instalação única de find-skills reconhece recibo legado e não reimporta automaticamente. O motor recebe somente nome/descrição e caminho das skills selecionadas cujo texto confere com a instalação no momento da montagem (alterada fora do app ou ilegível fica de fora e o `/id` avisa sem caminho); `/id` solicita sua leitura como orientação externa, sem mudar autoridade. Na seção Skills das configurações de cada bot, o app mostra o tamanho desse índice ("8 ligadas · 6 no índice · 1,2 KB no índice"), medido pelo Hub na mesma montagem do bloco: só informa, não corta nem limita, e não mede o texto completo das skills, a conversa nem se o bot leu ou usou uma skill; sem conferência fica "Tamanho do índice indisponível", nunca zero ([contrato](../reference/catalogo-skills.md#medida-do-índice)). Consulte [Catálogo passivo de habilidades](../reference/catalogo-skills.md) para limites, licenças e disponibilidade.

Package connectors added earlier through the catalog keep the command and arguments saved for them; the host no longer looks up newer versions in the MCP Registry. Changing a version is a manual configuration by URL, command or config. Network failures, missing packages and attempted downgrades leave the installed reference unchanged. Custom command connectors are never updated automatically.
### Skills de marketing passivas

Copywriting (adaptada com guia curto autoral no lugar do auxiliar de licença incerta), stop-slop e
copy-review-panel são opções do catálogo curado. Instalar e selecionar para
um bot exigem gestos separados do dono; nenhum pacote já instalado é atualizado
ou sobrescrito automaticamente. Marketing-os completo continua piloto futuro.
O painel usa cinco perspectivas numa resposta, cada uma 0–20: total <70
refazer/descartar, 70–84 revisar a principal objeção e conferir novamente, >=85
candidato para apresentar ao dono; no máximo três rodadas. É julgamento
editorial heurístico, sem prova de conversão, qualidade, segurança ou aprovação.
Sem cinco bots reais, serviços, ferramentas ou publicação automática; fatos,
métricas, depoimentos e promessas precisam de evidência real.


## Formato das skills novas

A fundação manual emite revisão inicial 1 e digest dos bytes canônicos no
salvamento humano existente. Manual v2 já conferível continua disponível sem
revisão nova fabricada; migração/escrita legada não adota o arquivo corrente como
aprovado. Sem prova preservada, fica indisponível, mas Remover e salvar de novo
continuam gestos disponíveis. Não entrega editor de revisões, diff/recibo/aceite
global, snapshot por turno ou revogação nativa. Contrato completo:
[catálogo de skills](../reference/catalogo-skills.md#fundação-manual).

O Hub valida o frontmatter com um parser YAML na criação manual e antes de
cadastrar a instalação pelo catálogo atual. Nome e descrição são obrigatórios:
nome técnico de 1 a 64 caracteres, minúsculas, letras/números Unicode ou hífens
(sem hífen nas pontas nem consecutivos), igual à pasta após normalização NFKC;
descrição de texto não vazio, até 1024 caracteres. Os limites contam caracteres,
não bytes. Na criação, o nome digitado gera o nome técnico; colisões recebem
sufixo e o truncamento reserva espaço para ele. Arquivo, pasta e nome anunciado
usam esse identificador final. A descrição conserva quebras, aspas, dois-pontos,
`#` e Unicode com serialização YAML segura.

Campos opcionais continuam opcionais: `license` e `allowed-tools` são texto;
`compatibility`, se presente, é texto não vazio de até 500 caracteres;
`metadata` é mapa de textos para textos. Propriedades adicionais ficam em
`metadata`. O arquivo importado é preservado, inclusive opcionais; um erro de
formato impede seu cadastro e sua ativação. Não há descrição fabricada nem
renomeação silenciosa da importação. Entradas inválidas do catálogo são omitidas
com diagnóstico no log do Hub. YAML malformado, tipos incorretos, campos duplicados
e frontmatter não fechado são recusados.

Skills já cadastradas permanecem no disco e no cadastro. Ao abrir seus detalhes,
o Hub registra aviso de formato inválido no log e conserva a leitura do conteúdo,
sem migração, renomeação ou regravação automática. O diagnóstico não é uma auditoria
retroativa de todo o acervo. Configurações pessoais de agentes não são importadas.
Seleção por bot, comandos e leitura progressiva continuam iguais. Uma skill,
inclusive `allowed-tools`, **nunca concede ferramentas ou autorização**.

Esta etapa cobre os primeiros caminhos novos. O instalador ainda baixa diretamente
no destino: uma falha pode deixar arquivos sem cadastro. Transporte completo,
atomicidade, referências imutáveis, confinamento, liberação de versões,
aprendizado, novas telas e exportação não fazem parte desta correção. Não há
promessa de conformidade completa nem de isolamento pelo formato.
Referências: [especificação Agent Skills](https://agentskills.io/specification),
[validador oficial](https://github.com/agentskills/agentskills/blob/69ef37e9424c0a7ea9dd2293b559e43ec8176379/skills-ref/src/skills_ref/validator.py).

## Comparar texto manual sem salvar

A comparação de uma skill manual com revisão comprovada abre uma cópia editável,
mostra o documento anterior e a proposta integralmente e os bots vinculados em
`cfg.skills`. **Comparar não salva, aprova ou substitui a skill**: a revisão
seguinte é só candidata e o digest comprova a comparação, sem reservar versão ou
alcance. Editar o rascunho exige comparar novamente para atualizar a apresentação.
Curadas ficam fora; manual antiga sem revisão comprovada conserva sua leitura
normal, mas não inicia exame fabricando uma aprovação. Falha ou outro Hub não
viram resultado vazio. Publicação compartilhada aguarda o produtor aceito de
leitura nativa/snapshot e revogação; esse fluxo não entrega essa capacidade.

Queda ou novo hello no mesmo Hub preservam só o texto/alvo em memória da
abertura, sem base, resultado ou pedido automático. Reler a instalada exige
novo gesto na mesma instalação; outro Hub ou instalação não adotam o rascunho.
Fechar, Voltar, abrir outra linha ou sair da página Skills descarta o texto.

## Commands

Typing `/` at the start of a message to a bot (desktop, iPhone, Android, terminal) lists that bot's commands, filtered by what follows the `/` (a command's name or title): the built-in ones first, then the skills the bot has on (on the desktop, unless Ajustes → Conversa → "Skills no menu /" is off: then only the built-in ones are listed, and a typed `/<skill id> ` still works; see [composer](../guides/composer.md)). A group chat lists its members' commands the same way: Fast mode for every member that offers it (on once all of them are; picking it turns each member's own Fast mode on or back off, so it also applies in their own chats), then every skill one of them has on. A `/skill …` message in a group reaches each member that has the skill on as "read this skill first"; the others read it as typed. Each row shows an icon, the title and what it does. The list floats above the box on the desktop (↑/↓ move the highlight, Tab or Return picks it, Escape hides the list until the draft changes, hover and click work too), sits inline above the box on the iPhone (a tap picks; it isn't a menu, so the system-menu rule doesn't apply), and is the highlighted list over the transcript in the terminal (↑/↓, Tab or ↵).

| Command | Title | Does |
| --- | --- | --- |
| `/fast` | Fast mode | Switches [Fast mode](#fast-mode) with `updateBot`; listed only while the bot offers it. Its detail says what picking it does, or why the agent refused it (warning color). |
| `/<skill id>` | the skill's name | Fills in `/<skill id> ` for the rest of the message. |

Picking a built-in command, or sending a message that is just one (`/fast`), runs it in the client and clears the box; nothing reaches the agent. Return on a skill whose name is already typed out sends the message. The host sees a message starting with `/<id of a skill the bot has on>` and hands the agent `[Codync: the user picked the "<name>" skill for this message. Read <path> first and follow it.]` followed by the rest of the message; the chat keeps the text as typed. Any other `/command`, including `/fast` for a bot without Fast mode on offer, reaches the agent unchanged, so its own commands keep working.

## Sign-in for remote connectors (`host/src/market/oauth.rs`)

Installing a remote connector probes it: a 401 to `initialize`, or (with no headers given) protected-resource metadata naming an authorization server, for servers like Call2Me that only refuse tool calls, marks it `signedOut`, and bots don't get it until someone signs in. Sign-in follows the MCP authorization spec: protected-resource metadata → authorization-server metadata → dynamic client registration (both redirect URIs at once) → PKCE. Tokens stay on the computer and are refreshed on use (or after a 401).

| Where the user signs in | Redirect URI | Who finishes |
| --- | --- | --- |
| On the computer (TUI, or any loopback caller that sends no `loopbackPort`) | `http://127.0.0.1:<port>/oauth/callback` | the host's callback page (em pt-BR) |
| On the desktop app, Hub on **another** computer (SSH tunnel on a random local port) | `http://127.0.0.1:19424/oauth/callback` (dev: `19425`), the app's own listener | the app sends `connectorSignInFinish` over its authenticated channel |
| On the phone (E2E channel caller) | `<cloud>/v1/oauth/callback` → 302 `codync://oauth?…` | the app's system sign-in sheet sends `connectorSignInFinish` |

**Hub remoto (túnel).** O redirect `127.0.0.1:<porta do Hub>` só existe no computador do Hub; com o Hub atrás de um túnel SSH (porta local aleatória no Mac) o navegador recebia `ERR_CONNECTION_REFUSED`. O app desktop agora abre, só enquanto o login está em andamento, um ouvinte em `127.0.0.1` numa porta fixa (`19424`; `19425` no app dev; `apps/desktop/src/main/fork-oauth-loopback.ts`) e manda `connectorSignIn { id, loopbackPort }`. O Hub (`host/src/fork/conectores/oauth_desktop.rs`) só aceita essas duas portas, só do chamador local (aparelho com `loopbackPort` é recusado), usa `http://127.0.0.1:<porta>/oauth/callback` no `redirect_uri` e no cadastro dinâmico e responde `callback: "desktop"`. O ouvinte só atende `GET /oauth/callback` com o `state` exato do pedido e `Host` `127.0.0.1:<porta>` (qualquer outra coisa recebe 4xx e o login segue esperando), uma tentativa por vez, e repassa a URL de retorno ao renderer, que chama `connectorSignInFinish`; o navegador só vê "conectado" depois de o Hub concluir. `state`, PKCE e `iss` valem como em todo retorno, o `state` do app só conclui com o mesmo chamador que o iniciou (a página `GET /oauth/callback` do Hub o recusa), nada escuta fora do loopback, e sem porta livre (ou com Hub antigo) o login segue pelo caminho de antes.

The cloud page is stateless and the code is useless without the PKCE verifier held by the host. Servers without dynamic client registration (e.g. GitHub) need a token header instead (custom connector, URL + headers). `connectorTarget` (loopback only) hands the proxy the URL and headers.

**Endereços e redirecionamentos (`host/src/fork/conectores/oauth_urls.rs`).** A URL do conector precisa ser `https` com host, ou `http` só em `localhost` exato, `127.x.x.x` ou `[::1]`; usuário/senha na URL e `#` são recusados. A instalação e o início do login conferem isso antes de qualquer pedido; no login, a recusa não limpa tokens nem muda geração ou revisão. Os endereços que o servidor informa (dica de metadados, servidor de autorização, `authorization_endpoint`, `token_endpoint`, `registration_endpoint`) seguem a mesma regra, e `http` só vale quando o conector e o endereço são locais. Servidor de autorização em outra origem continua permitido. Os pedidos do login e da renovação não seguem redirecionamento (`Location`): metadado redirecionado passa ao próximo endereço padrão já previsto; cadastro redirecionado é recusado; token redirecionado recusa o login e, na renovação, conserva o login para tentar depois, sem sign-out. Serviços que dependem de redirecionamento automático no login deixam de entrar. Os erros são fixos, sem URL, corpo ou segredo. Limites: `localhost` é validado pelo nome, não pelo DNS; não há filtro de IP nem bloqueio de rede interna, e um `https` que resolve para endereço interno continua alcançável. O `token_endpoint` já salvo não é revalidado na renovação; o próximo login o confere.

**Identidade do servidor de login (RFC 8414 §3.1/§3.3).** O emissor escolhido em `authorization_servers` (ou, sem ele, a origem do conector, como antes) é guardado exatamente como veio, sem aparar barra final nem normalizar. A barra final sai só para montar o endereço `.well-known` da consulta. Cada resposta de metadados precisa trazer `issuer` como texto idêntico a esse emissor; ausente, nulo, de outro tipo, diferente ou só com outra barra final é descartada, e a descoberta tenta apenas os outros endereços padrão do mesmo emissor. Se nenhum servir, o login para com erro fixo ("não confirmou a própria identidade"), sem URL, emissor ou corpo remoto e sem cadastro do app nem pedido de token; não se troca de servidor de autorização. Só metadados aceitos alimentam endpoints, cadastro e link de autorização, e o emissor original vai para o vínculo privado da concessão, na mesma gravação protegida. Servidores que omitem ou publicam outro `issuer` deixam de entrar. Limites: o início do login já criou a nova tentativa antes da recusa e não restaura concessão anterior; `iss` do retorno e `resource` do PRM são conferidos nas seções abaixo; escopos não são conferidos aqui (PKCE anunciado abaixo); contas já salvas não são deslogadas nem migradas.

**PKCE anunciado (exigência da especificação MCP vigente).** Depois de aceitar os metadados do mesmo emissor e reconferir o vínculo, e antes do cadastro do app (DCR), da gravação da concessão, da tentativa pendente e do link, o login exige que esses metadados declarem `code_challenge_methods_supported` como lista só de textos contendo `S256` exatamente assim. Ausente, nulo, texto solto, objeto, lista vazia, item que não é texto, só `plain` ou grafia diferente (`s256`, com espaço) recusam; vale também quando o app já tem `client_id` cadastrado. Lista com `S256` e `plain` é aceita, e o link continua com `code_challenge_method=S256`; nunca se usa `plain`, nem se omite o challenge, nem se busca outro emissor ou outro `.well-known` para contornar a ausência. O app mostra a frase fixa "Este serviço não declara a proteção PKCE exigida (S256); login recusado.", sem URL nem corpo remoto. Recuperação: só o provedor do serviço pode publicar a declaração; não há opção para relaxar. Fonte: especificação MCP 2026-07-28, Security Considerations, [Authorization Code Protection](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations#authorization-code-protection), consultada em 05/11/2026 (metadados sem o campo, em OAuth AS ou OIDC: o cliente MUST recusar). A RFC 9700 §2.1.1, sozinha, só recomenda publicar o campo; a recusa aqui segue a especificação MCP. Limites: o início do login já invalidou a concessão anterior e limpou tokens antes da recusa, que não restaura o login anterior; a versão MCP negociada não muda e os demais requisitos de 2026-07-28 não foram implementados por esta etapa; escopos seguem pendentes (ver seções abaixo).

**Emissor do retorno (especificação MCP vigente e RFC 9207 §2.4).** Ao iniciar o login, depois de aceitar os metadados do mesmo emissor e o PKCE, e antes do cadastro do app, da concessão, da tentativa pendente e do link, a tentativa guarda o emissor original e o valor de `authorization_response_iss_parameter_supported` desses metadados. Campo presente com tipo diferente de booleano (inclusive `null` ou `"true"`) recusa o início com frase fixa, sem presumir `false`. No retorno, depois de consumir o `state` e conferir Callback, Origin, vínculo e autoridade originais, e antes de usar o código, pedir token ou mostrar `error`/`error_description` do serviço:

| Anúncio guardado | `iss` no retorno | Resultado |
| --- | --- | --- |
| `true` | igual ao emissor original | segue como antes |
| `true` | ausente | recusa |
| `false` ou ausente | presente | compara exato; diferente recusa |
| `false` ou ausente | ausente | segue como antes |

A comparação é de texto exato, também em retorno de erro: sem aparar espaços, mudar maiúsculas, porta, barra ou codificação. A query é decodificada uma vez (loopback do Hub ou `URLSearchParams` no app); o JSON só transporta. Campo repetido no retorno (`state`, `code`, `error`, `error_description`, `iss`) recusa em vez de escolher o primeiro valor; no `connectorSignInFinish`, `iss` ausente ou `null` é ausência, texto segue intacto e outro tipo recusa. A recusa mostra só "O retorno do login não confirmou o serviço esperado; login recusado. Inicie o login novamente." (ou "veio malformado" para retorno ambíguo), sem emissor, URL nem texto do serviço; o `state` já consumido não volta e nada é repetido sozinho. Metadados trocados depois do início não mudam a tentativa; o retorno nunca consulta metadados de novo. A nuvem (`/v1/oauth/callback`) já repassa a query inteira ao app. Fontes: [MCP 2026-07-28, Authorization Response Validation](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization#authorization-response-validation) e [RFC 9207 §2.4](https://www.rfc-editor.org/rfc/rfc9207.html#section-2.4). Limites: iOS descarta iss; cliente antigo será recusado quando o servidor exigir — adaptar o repasse no app iOS numa etapa própria. Sem exceção por plataforma. A versão MCP negociada não muda; escopos e cliente por emissor seguem pendentes (seção abaixo); tokens e logins já salvos não mudam.

**Identidade do recurso.** A descoberta PRM leva a URL de consulta e a
identidade esperada de cada candidato: primeiro o hint do ping, depois well-known
por caminho/query e raiz. O hint exige `resource` igual à URL usada na chamada ao
servidor; o candidato por caminho preserva query e barra semântica final de `/mcp/`.
A barra imediatamente após o host, sem caminho, sai somente da montagem do lookup.
O fallback raiz espera a origem sem barra que gerou a consulta, não o endpoint `/mcp`.
`resource` ausente, nulo, não textual, vazio ou diferente descarta o documento inteiro,
inclusive AS e escopos. A comparação é exata, sem trim, normalização Unicode ou
equivalência de URLs. O próximo candidato legítimo pode ser consultado.

Sem PRM válido, o login recusa antes de cadastro do app, concessão, tentativa pendente
ou link com a frase fixa "O serviço não confirmou a identidade do recurso; login
recusado.", sem URL nem dados remotos. O começo da reconexão já pode ter invalidado
a concessão e limpado tokens; a recusa não restaura o login anterior. Só o provedor
corrige os metadados; não há opção para relaxar a checagem ou repetir por outro endpoint.
O resource aceito segue intacto na mesma tentativa/concessão, autorização e troca do
código; callback não redescobre nem recaptura pelo ID. Refresh conserva o resource do
mesmo Grant e legado sem Grant continua omitindo o parâmetro, sem migração.

Para não acrescentar outra audiência ao recurso escolhido pelo Hub, o login novo
recusa `resource` já presente na query de `authorization_endpoint` ou `token_endpoint`,
inclusive chave percent-encoded ou repetida. A recusa ocorre antes de DCR/Grant/Pending/link,
com "O servidor de login informou um recurso adicional; login recusado.". Outras queries
legítimas permanecem; só o provedor corrige os endpoints. Não altera token_post nem refresh
legado, inclusive concessões antigas sem Grant.

A sonda com initialize 200 só reconhece OAuth por PRM validado com AS; 401 continua
indicando login necessário e fallback SSE continua só em 404/405. AS em outra origem
permanece legítimo pelas guardas existentes. O fallback de AS pela origem só permanece
após PRM válido sem AS selecionável; é compatibilidade herdada, não prova de conformidade
MCP completa. Login e sonda não executam ferramentas. Não muda escopos, cliente por
emissor, iOS, versão MCP ou autoridade. Fontes: [RFC 9728 §§3/3.1/3.3/6](https://www.rfc-editor.org/rfc/rfc9728.html)
e [MCP 2025-06-18, Resource Parameter Implementation](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization#resource-parameter-implementation), conferidas em 06/11/2026.

Native local MCP uses the exact command and arguments the user saved; the host no longer looks up or replaces package versions. An enabled skill tells the bot where its instruction folder lives. These are distinct from the built-in `team` and `computer` MCP services.

Imported configuration and repository metadata are untrusted: the host validates names and paths before installing. Secret values stay on the computer; listings report whether required values are configured. Adapter downloads and external authorization depend on the selected provider.

To check a change, use a disposable bot on the intended computer, verify native installation/configuration, start a new session as needed, and confirm the agent actually receives the capability. Listing an item alone does not verify its authentication or runtime behavior.

See [file structure](../architecture/file-structure.md), [bot collaboration](bot-collaboration.md), and [remote screen](remote-screen.md).

## Model selection

The shared bot editor requests `agentModels {backend}` from the selected computer. The host starts an isolated ACP session under its probe directory with saved agent credentials, no MCP servers and no prompt, then closes the process. The response is `{models: [{id, name, description, options}], currentModelId, fastMode}`.

Discovery prefers ACP `configOptions` with category `model` (including grouped options), with conventional `model`/`models` IDs as a compatibility fallback. Older agents may expose `models.availableModels`. Model names and IDs come from the agent; Codync does not bundle a provider catalog. See the [ACP config option contract](https://agentclientprotocol.com/protocol/v1/session-config-options).

A model's `options` are its own settings, such as reasoning effort or fast mode: `[{id, name, values: [{id, name}], default}]`. Every `initialize` sends `clientCapabilities._meta.parameterizedModelPicker: true`; Cursor then lists plain models and, on `cursor/list_available_models` (asked only of the Cursor backend), each model's settings as ACP select config options. Without the flag Cursor offers one fixed variant per model (`grok-4.7[reasoning_effort=high,fast=true]`) and rejects every other. The bot stores the picks as `modelOptions` (option id → value; an absent id means the agent's default). After choosing the model with `session/set_config_option`, the host sets each option the same way, on new sessions and again after `session/load`, because Cursor resumes a session on its last-used model. A bot saved with a variant ID keeps working: when the agent lists the plain model instead, the bracket's `key=value` pairs become options (Auto's `default[]` becomes `default`). Some values change Cursor's billing: a larger context (Grok 4.7 at 500K) turns on its Max Mode. Changing the model or agent clears the options; the host also clears them when an app that predates them changes the model.

The editor keeps Default, shows available models in a dropdown, and resets the selected model when changing agents. Under the model it shows one picker per option (iPhone and desktop: a row each, named by the agent, with Default first; terminal UI: an Options row, ←→ to move, space to change). Errors are visible and retryable. Custom commands and agents without model discovery retain manual ID entry; an existing unlisted ID is preserved. The probe uses an isolated directory, so project-specific agent configuration can differ from the real bot session. Changing the model restarts the bot's agent sessions with fresh model context and preserves the stored chat history. The agent remains responsible for validating availability when a session starts.

## Fast mode

Claude Code's and Codex's ACP adapters offer Fast mode (faster replies for more usage) as a session config option: Claude's `fast`, Codex's `fast-mode`, each a `select` of `on`/`off` (a boolean for clients that opt into boolean options), listed only while the session's model supports it. Only such an `on`/`off` or boolean option counts: Cursor's `fast` (`true`/`false`) is one of a model's options (see Model selection), so Fast mode leaves it to the bot's `modelOptions`. `fastMode` in `agentModels` says whether the probe session offered it, so it reflects the agent's default model.

The host keeps each live session's Fast mode option as the agent last listed it: from `session/new` (after the model switch, whose answer can change the options), `session/load`, `session/fork`, `session/set_config_option` answers and `config_option_update` notifications. Before every turn it switches the session to the bot's `fastMode` with `session/set_config_option` when the value differs, so changing the setting needs no agent restart and keeps the context. The adapters start each session or process at their own default, which is why a resumed session is switched again.

The bot's runtime carries what the agent says (`fastModeSupport`, `fastModeIssue` in the bot payload):

| `fastModeSupport` | Meaning |
|---|---|
| `unknown` | No session has listed its options yet. A session that lists no config options at all settles it as `unsupported`. |
| `offered` | The agent offers it for the session's model. |
| `unsupported` | The session's model doesn't (the chat gets an info notice while the bot asks for it). |
| `refused` | The agent turned it off right after the bot asked, or despite the switch: Claude does this for an account without extra usage and appends the reason to the option's description, which becomes `fastModeIssue`. The host stops asking until the user changes the setting or the host restarts. |

`offered` and `unsupported` are kept across host restarts (`fast-mode.<bot>` in the kv store); a refusal isn't, so a new host asks again.

Clients show Fast mode for an agent bot unless its runtime says `unsupported` (an `unknown` bot may offer it; its first session settles it), and always while it's on (so it can be turned off). A group has no Fast mode of its own: its `/fast` sets its members' (see [commands](#commands)). It shows in:

- Composer: the `/fast` [command](#commands), not a button. The terminal UI also shows `⚡ fast` or `⚡ refused` in the chat header.
- Bot settings: a Fast mode switch row under Model when the agent's model list offers it (never for a custom command), when the bot's own sessions do, or while it's on. While refused, its detail gives the reason. The terminal UI form always lists the field and notes when the agent doesn't offer it.

## Secure connection setup

See [Connector setup and credentials](connector-credentials.md) for the shared
macOS/Linux vault, optional 1Password references, in-chat connection requests and
MCP readiness checks. Native setup accepts a URL, command or pasted config;
saving is not proof of readiness. Setup failures remain visible for correction.
Legacy catalog-only requests are unavailable; existing connector IDs retain the
authorization workflow. See the manual connection contract above.

### Fonte RPC e recuperação de skills

O consumidor forte usa `skillProtocol:2` em consultas/mutações e exige essa versão no
envelope, mesmo em listas vazias. Parser antigo recusa o campo antes de mutar; não há
fallback ao corpo V1. Reply perdida/irregular depois do envio é resultado incerto a
reconciliar no Hub de origem, sem promessa de ausência de efeito ou retry manual automático.

O v1 persistido mantém seu shape antigo; v2 guarda só metadado forte do mesmo domínio/Store.
Guards SQL observam escritores antigos e o dono reatesta IDs/contador atomicamente antes de
reabrir clientes. Bytes/recibos/bindings são preservados; leituras/restart limpos mantêm
identidades. Restore físico continua no fluxo de backup/memória, sem ledger novo. O consumidor
desta fonte está implementado; QA e instalação aplicáveis permanecem pendentes.

## Mediação de retorno MCP remoto

Conexões remotas existentes usam a ponte Guard sem opção oculta: HTTP JSON,
Streamable SSE e SSE legado passam pela mesma inspeção de conteúdo/segredos do
stdio. Initialize, descoberta e resultado de ferramenta só são entregues depois
de persistir o recibo e a exposição externa da sessão. Descrições e dados
do servidor continuam externos; não são ordens do dono. Erros são projetados sem
credenciais; notificações, pedidos reversos e respostas sem correlação são retidos. Após observar,
ping reverso recebe `{}` e outros pedidos recebem erro fixo -32601, sem chegar
ao motor; até oito respostas de protocolo por pedido, sem diálogo recursivo.
O recibo distingue transporte `http` de `stdio`; o campo HTTP inclui os dois SSE.

Falha de contexto, cofre, cache, observação ou persistência retém o conteúdo; não
há fallback bruto. A ponte conserva tokens antigos/atuais em leases transitórias
para redigir resultados em voo. Trocar URL/headers, remover o conector, desligá-lo
no bot ou encerrar sua lane/run impede o retorno antigo. Isso não é geração de
conta nem catálogo curado. Verify usa contexto temporário de setup, separado de
bot, só para handshake/ping/descoberta; não chama ferramentas nem liga bots.

Listagens preservam `nextCursor` técnico validado, byte a byte, nas quatro
famílias: ferramentas, recursos, templates e prompts. Cursor presente deve ser
texto de até 4 KiB, sem controles/markup e sem alteração pela inspeção de segredos.
Cursor inseguro produz erro explícito de paginação retida e descoberta incompleta;
não vira fim silencioso da lista, cursor redigido ou página truncada com sucesso.
Verify só confere handshake e primeira descoberta; toolCount não é total paginado.

### Versões MCP por ponta

Data de protocolo MCP não é versão do app. Os servidores internos do Hub
implementam só **2025-06-18**: pedido nessa versão recebe a mesma, outra string
recebe 2025-06-18 como contraproposta e campo ausente ou não textual recebe
`-32602`. As pontes Guard (stdio local, remota e curada) e o verify aceitam
**2025-06-18**, **2025-03-26** e **2024-11-05**, somente no perfil de mensagens já
mediado, sem traduzir respostas entre revisões. A versão pedida suportada segue ao
serviço; texto desconhecido vira a preferência 2025-06-18; pedido sem versão
textual recebe `-32602` sem rede.

A versão devolvida é lida do resultado original correlacionado antes da projeção
e só é aceita depois do Guard, do recibo, da marca de exposição e da
revalidação; chega intacta ao motor. Versão ausente, não textual ou desconhecida
nunca vira 2025-06-18: o recibo é gravado e o motor recebe `-32603` fixo
("Resposta initialize MCP inválida" ou "Versão MCP não suportada nesta conexão"),
sem o valor remoto. A sessão remota, a ponte stdio ou o canal curado é encerrado;
nenhum `initialized`, descoberta ou chamada vai à rede depois. O verify para antes
de `initialized`. Durante o initialize da ponte stdio só ping atravessa.

No HTTP, cada Session guarda a própria versão aceita e o próprio `Mcp-Session-Id`;
dois canais para a mesma URL negociam separadamente. `MCP-Protocol-Version` e
`Mcp-Session-Id` configurados no conector são descartados, inclusive duplicados e
em outra caixa; só o estado da Session é enviado, uma vez. Initialize e GET inicial
saem sem versão; `initialized`, ping, descoberta, chamadas e respostas reversas
levam a versão aceita, inclusive um ping recebido no GET legado antes do ACK do POST,
sem liberar a descoberta antes desse ACK. Ping reverso anterior ao resultado aceito
sai sem versão. RPCs internos do Hub, metadados e token OAuth não recebem esses headers.

Fallback para o SSE legado só com HTTP 404 ou 405 no initialize inicial, uma vez e
no mesmo prazo. HTTP 400, erro JSON-RPC em 200 ou versão incompatível não abrem
fallback nem repetição, e `tools/call` nunca é repetido para descobrir transporte ou
versão. Limite: um servidor legado que só responde 400 deixa de ser detectado.
`2024-11-05` por POST é compatibilidade restrita do adaptador, não o Streamable HTTP
daquela revisão. Fora desta etapa: MCP 2026-07-28, SDK, novas capacidades e OAuth
de emissor/recurso.

O GET legado tem leitor contínuo único com canal de capacidade 1, inclusive entre
pedidos. O mesmo mediador observa os eventos e só depois responde ping/recusa;
funciona também quando o POST aguarda um ping no GET antes de devolver seu ACK.
POST legado 2xx com texto, como `202 Accepted`, é ACK retido/observado; a resposta
correlacionada continua vindo do GET. No Streamable, texto 2xx de notificação é
retido sem gerar erro. Respostas reversas enviam Accept explícito para JSON e SSE,
para compatibilidade com servidores estritos; isso não é uma reprodução de 406
no SDK Python atual, que admite wildcard com o reqwest utilizado.

Limites: entrada até 4 MiB; corpo JSON e cada frame SSE até 64 KiB; até 64 frames
de dados/256 KiB de wire por pedido/resposta, nunca pela idade da conexão. Um chunk
HTTP pode conter vários frames. A biblioteca entrega esse chunk antes da quota,
sem teto próprio imposto aqui; só a cópia admitida é limitada. Isso não limita
RSS ou alocação prévia da biblioteca. Comentários keepalive contam no wire e na taxa,
mas não como resposta de dados. Janela monotônica de 1 s: até 64 eventos/256 KiB e
oito pedidos reversos, incluindo atividade idle. A janela pode admitir duas
rajadas próximas à fronteira; os limites permanecem finitos. Há backpressure no
leitor, sem fila ilimitada. Pedidos do motor devem ser serializados: um ativo,
um único slot para o próximo pedido durante initialized ou mediação idle, e
pedidos paralelos adicionais recusados. Não há fila de execução de efeitos.

Prazo total de 90 s por pedido, incluindo retenção/registro da falha e sem
reiniciar por ping; se não houver tempo para registrar, encerra com erro fixo,
sem conteúdo bruto ou retry. POST até 60 s. Mediação idle
até 30 s e POST de protocolo até 10 s. UTF-8/CRLF divididos são preservados.
Erro HTTP/transporte de um pedido Streamable pode retornar erro fixo e conservar
a ponte para pedidos posteriores; nunca o repete automaticamente. Falha de
contexto/cofre/cache/observação/persistência, origem ou limite remoto/taxa, cancelamento, EOF,
prazo total e estado SSE legado incerto encerram a ponte. Interromper HTTP ou
conservar a ponte não comprova ausência de efeito: conferir antes de repetir.
Notificações do motor sem ID fora do fluxo admitido são ignoradas sem erro id:null.
Redirects são recusados e o endpoint legado precisa da mesma origem antes de
receber headers; refresh/fallback só negociam no initialize, sem retry de tools/call.

Esta entrega medeia **retorno**. Não entrega aprovação de efeitos, recibo de ação,
catálogo/Linear, isolamento de processos, ferramentas
nativas do motor e terminal. Bearer local não isola processos do mesmo usuário.
As restrições de Spaces e a seleção legada BotConfig permanecem aplicáveis.
A inspeção é parcial: componentes de Basic/path podem não ser reconhecidos, e
headers/query comuns podem causar redação excessiva; veja connector-credentials.

## Catálogo preparatório Linear no Hub

O Hub informa dois perfis preparatórios de Linear pela consulta administrativa
Local `donnaConectorCapability {}`: **Leitura** (`/mcp/readonly`, escopo previsto
`read`) e **Leitura e criação** (`/mcp`, escopos previstos `read write`). O catálogo
está disponível por esse contrato do Hub. O catálogo de conexão do Marketplace é outro (veja a seção seguinte). A resposta mantém `decisionProtocol:0`, `available:false`,
`conectarDisponivel:false` e a lista de acessos vazia.

Consultar não cria conta, login, seleção de bot ou cartão. Os escopos são uma
previsão do perfil, não permissões concedidas nem seleção aplicada ao OAuth
legado. `read write` é mais amplo no fornecedor; a allowlist futura terá de
restringir as operações. Sem identidade e schema real autorizado, nenhum acesso
curado fica disponível. Adicionar uma URL em Meus MCP não promove a conexão ao
catálogo. Google e Meus MCP conservam seus fluxos próprios.

Fontes públicas conferidas por GET em 06/11/2026: [guia Linear](https://linear.app/docs/mcp),
[metadados de leitura](https://mcp.linear.app/.well-known/oauth-protected-resource/mcp/readonly)
e [metadados padrão](https://mcp.linear.app/.well-known/oauth-protected-resource/mcp).
Essas fontes não comprovam conta, workspace autenticado ou schema de ferramentas.

## Marketplace de conectores no Hub

Esta etapa entrega **só o Hub**; a tela vem em seguida. O catálogo antigo de `donnaConectorCapability` (acima) segue como estava: é do caminho curado de efeitos e não conecta nada.

### Catálogo como dado

`host/src/fork/conectores/catalogo_servicos.rs` (tipos), `catalogo_fichas.rs` (fichas em Rust) e `catalogo_json.rs` (fichas em arquivo JSON, pasta `fichas/`) são dado versionado no repositório, **sem segredo**: nem chave, nem token, nem `client_secret`, nem URL privada. Cada serviço tem `id`, `nome`, `categoria`, `descricao`, `exemplos`, `links` (privacidade e termos do fornecedor), `conferidaEm`, `fontes`, `aviso`, `tipos[]`, `skills[]` e `emBreve`. Cada tipo de conexão (`oficial_oauth`, `oficial_chave`, `oficial_app`, `local`) traz a URL oficial fixa, o login (`oauth` por DCR ou `chave` em cabeçalho), os **grupos de liberação** (`id`, `rotulo`, `escopos`, `url`, `ferramentas`, `padrao`, `disponivel`) e as `perigosas`. A pessoa nunca informa URL, escopo nem ferramenta: o Hub pega tudo da ficha.

**Skills das fichas** (`skills[]`: `id`, `titulo`, `resumo`): cada `id` é uma skill do catálogo curado (categoria **Conectores**, pasta `host/catalogo/skills/<serviço>/<serviço>-<skill>/`, nome da pasta igual ao `name`), com o `LICENSE` junto; o teste reprova `id` que não existe no catálogo ou repetido. A descrição de cada uma diz quando usar e quando não usar e as skills nomeiam só ferramentas de leitura (a do Exa nunca usa `agent_run`; a do Supabase só `SELECT`). Skill é texto: não concede conta, ferramenta nem permissão e nunca liga sozinha.

Cada ficha é um arquivo em `fichas/` (lista `FICHAS` em `catalogo_json.rs`; o teste reprova arquivo fora da lista). Só as três do Google ficam em Rust, até a frente do Google as refazer. Campos acrescentados em 18/11/2026: `site` e `docs` (links do fornecedor), `skillsOficiais` (`url`, `origem`, `licenca`, `aproveitavel`: só link e licença, nada é instalado), `imposicao` em cada grupo (`url`, `escopo` ou `natureza`: quem garante que o grupo só lê) e `prefixo` na chave (`Bearer `, que o app junta ao que a pessoa cola). **Regras que o carregamento confere** (ficha que falha fica de fora e vai para o log): categoria na lista fechada (E-commerce, Projetos, Código, Dados, Comunicação, Vendas e CRM, Marketing, Finanças e pagamentos, Design e mídia, Produtividade, Pesquisa e web, IA e automação, Infraestrutura, Agregadores; o Hub manda a lista em `categorias` e o app mostra nessa ordem), descrição de até 140 caracteres, endereços em https, id do jeito começando pelo id da ficha e **nenhum grupo disponível sem `imposicao`**. Um teste confere ainda que grupo com `imposicao: url` tem URL de leitura e que nenhum escopo de grupo disponível é de escrita. Entrar na lista `CONECTAVEIS` do teste é decisão consciente.

| Serviço | Categoria | Estado (conferido em 18/11/2026) |
| --- | --- | --- |
| Ficha do dono A | E-commerce | Primeira da lista. OAuth com DCR, grupo **Ler** pelos escopos `*:read`; **Criar e alterar** indisponível até o filtro de ações perigosas. Só esse arquivo traz o nome do produto |
| Linear | Projetos | Conecta por OAuth (DCR). **Ler** (padrão): `/mcp/readonly`, escopo `read` (imposição por URL) |
| Context7 | Pesquisa e web | Conecta por OAuth (DCR). Só duas ferramentas de consulta (imposição por natureza); conferido ao vivo que `tools/list` responde sem login |
| Neon | Dados | Conecta por OAuth (DCR) pelo endereço `?readonly=true` (imposição por URL), escopo `read`; a identidade do recurso com a query foi conferida no metadado |
| Wispr Flow | Produtividade | Conecta por OAuth (DCR). A documentação do fornecedor diz que o acesso é só de leitura (natureza) |
| GitHub | Código | Conecta por **chave** (token pessoal, `Authorization: Bearer`) no endereço `/readonly` (imposição por URL). O token deve ser de leitura: é a segunda trava |
| Exa, Supabase, Todoist, ClickUp, Metricool, Lovable, Firecrawl, Render, Short.io, Composio | vários | **Em breve**, cada um com o motivo na ficha: execução cobrada liberada junto com a busca (Exa), cadastro com segredo (Supabase), escopo de leitura não anunciado (Todoist), leitura ainda não provada num login real (ClickUp, Metricool), plano Business e login a confirmar (Lovable), ferramenta que age em sites (Firecrawl), chave sem modo leitura (Render, Short.io, Composio) |
| Airtable, Atlassian, Attio, Brevo, Browser Use, Cal.com, Calendly, Camtasia, Cloudflare, Coda, Descript, Expo, Fal, Granola, Higgsfield, Jungle Scout, mem0, monday.com, Notion, OpenRouter, PayPal, Pipedrive, Plaud, Railway, Resend, Semrush, Sentry, Stripe, Tally, TikTok Ads, TinyFish, vidIQ, Wix | vários | **Em breve** (segunda leva): MCP oficial com cadastro automático (DCR) conferido ao vivo, mas sem leitura imposta pelo fornecedor; entram depois do filtro de ações perigosas. Coda e mem0 pedem um segredo por instalação e ficam fora das conexões. Os que gastam créditos (Semrush, vidIQ, Higgsfield, Fal, OpenRouter, Browser Use, Camtasia) e os que mexem com dinheiro (Stripe, PayPal) trazem aviso na ficha. A lista `perigosas` de cada serviço não fica na ficha |
| Gmail, Google Drive, Google Agenda, Outlook, Slack, Instagram | Comunicação, Produtividade, Marketing | **Em breve**, sem caminho hoje e sem data |

Fora do catálogo por enquanto: Mailchimp Transactional (o kit de marca só traz um logotipo largo e a chave não tem modo leitura) e Produto B do dono (falta o endereço do MCP e a logo, que o dono precisa passar).

**Reconferência a cada 90 dias**: o teste `nenhuma_ficha_esta_com_a_conferencia_vencida` falha no CI se alguma ficha tiver `conferidaEm` há mais de 90 dias, e o Hub recusa conectar por uma ficha vencida (`ficha_vencida`). Data ilegível ou futura conta como vencida.

### Contas nomeadas e Space

O `Connector` (uma conta = um conector) ganhou `servico`, `tipo`, `liberacoes`, `space` (obrigatório) e `identidade` (opcional, só para a tela; o recibo usa um identificador opaco). Contas antigas (registro sem `space`) ganham o Space pela migração abaixo, sem perder nada.

- **Nomes**: a 1ª conta do serviço no Space não pede nome e usa o do serviço ("Linear"). Ao conectar a 2ª, o nome é **obrigatório para a nova e para a 1ª** (`nomePrimeira`) enquanto a 1ª ainda se chamar só pelo nome do serviço; da 3ª em diante o nome é sempre obrigatório. Nomes são **únicos por serviço e Space** (sem diferenciar caixa, acento ou pontuação); a regra antiga, global, saiu. Recusas: `nome_obrigatorio`, `nome_primeira_obrigatorio`, `nome_repetido`, `nome_invalido`. A regra roda de novo dentro do `connector_lock`, então duas conexões simultâneas não a furam.
- **Renomear** qualquer conta nunca muda o `id` (bots, recibos e ferramentas seguem apontando para ela) nem o vínculo OAuth.
- **Desconectar** apaga só o registro daquela conta no cofre; as outras seguem idênticas. O resultado lista os bots que a usavam. `revogadoNoFornecedor` é sempre `false`: nada diz que o acesso foi revogado no fornecedor.
- **Recibo** de conectar, renomear e desconectar (tabela `mexerica_conta_recibos`), gravado na **mesma transação** que sela o cofre (a ação e o recibo nascem ou falham juntos): ação, identificador opaco da conta, serviço, Space, resultado e horário. Nunca nome, token, cabeçalho nem endereço. A leitura pela tela vem depois.
- **Space**: a conta nasce no Space do pedido (`spaceId`, validado pelo `guard` do dispatch; sem ele, o Pessoal), sem seletor de Space na tela. O Hub recusa `conta_de_outro_space` ao ligar a conta em bot de outro Space (`updateBot`, criação de bot, pedido de conexão e `extensaoEmTodosAplicar`, que deixa esses bots de fora com o motivo `space_diferente`), ao renomear ou desconectar de outro Space, e na execução: o bot não monta o servidor MCP de conta de outro Space, `canonical` (entrega de credencial) recusa e `list_connectors` não a lista. Contas antigas migram uma vez, com marca (`migration.contas-space.v1`), na partida com o cofre aberto ou logo depois de abri-lo (`market/contas_migracao.rs`): **(a)** se todos os bots que usam a conta estão no mesmo Space, a conta vai para esse Space; **(b)** se nenhum bot a usa, vai para o Pessoal, e o mesmo aviso diz que nenhuma conta sem uso fez algum bot perder acesso; **(c)** se é usada em Spaces diferentes, vai para o Pessoal e o aviso lista os bots que perderam a conta. O dono recebe **um** aviso por migração (`add_entry_once`, id `contas-space-v1`), com os nomes dos bots que perderam a conta (nos casos b e c), sem nome de conta, endereço nem identidade. Só o registro de contas no cofre muda; nenhum bot é regravado (nem o Berga, nem principal de Space), e a lista `connectors` do bot que perdeu a conta fica como estava, porque a execução já recusa conta de outro Space. Rodar de novo não move nada, não regrava o cofre e não repete o aviso. **Banco ocupado:** quem lê o cofre já dentro de uma transação não espera o banco e **não adivinha**: o Space da conta antiga fica vazio (incerto), a execução e o vínculo a recusam (fail-closed) e nenhuma escrita fora do `run` sela destino chutado; só o `run`, que espera o banco, sela o Space calculado e grava o aviso.

### Conectar pela ficha

`marketConectar` cria a conta com URL, tipo e liberações **da ficha**, no Space do pedido, e devolve `proximoPasso`: `entrar` (OAuth) ou `chave`. Depois o app usa o caminho que já existia: `connectorSignIn` → `connectorSignInFinish` → `connectorVerify` (OAuth), ou `credentialUpdateConnector` com o valor da chave e `connectorVerify` (chave em cabeçalho; a conta nasce com o cabeçalho vazio e só vira servidor MCP depois do valor). O login **pede só os escopos dos grupos ligados** (`escopos_do_login`): conta de ficha ignora o desafio do servidor e a lista inteira dele e recusa o login se o servidor não oferecer um escopo da ficha; conta sem ficha (MCP do próprio usuário) mantém o comportamento antigo. Conta de ficha que sumiu do catálogo ou com URL diferente da ficha falha fechada. `connectorVerify` conta e expõe só as ferramentas dos grupos ligados quando a ficha traz lista; sem lista (Linear, onde o `/readonly` impõe o limite no fornecedor) o limite é do fornecedor. **O filtro por ferramenta durante o uso**, para grupos com lista, fica para o filtro de ações perigosas; por isso nenhum grupo disponível hoje depende dele.

### Vínculo bot × conta

A lista `connectors` do bot continua sendo a lista de contas **ligadas** (padrão: nenhuma; contas existentes ligadas continuam ligadas, sem migração de dados), porque guarda, execução e `extensoes_em_todos` já a leem. Dois campos novos, só do dono (classe `DO_DONO` dos gatilhos do Berga e campos fixos do principal de Space): `contaPadrao` (`servico` → id da conta) e `perguntarConta` (`servico` → `true`). A tela lê a projeção `{conta, ligada, padraoDoServico}` por `marketBotContas`. O padrão precisa ser conta do serviço, do Space do bot e ligada; desligar a conta apaga o padrão dela. A escrita passa por `update_bot_by_user_authorized` com a autoridade original do dono: os gatilhos do Berga seguem ligados e o principal de Space continua sem conector.

### Escolha da conta em cada pedido (pronta, sem encaixe no turno)

`host/src/market/contas_escolha.rs`: `escolher_conta` e `contas_do_turno` decidem, nesta ordem: (1) conta citada **pelo nome na mensagem da pessoa** (nome inteiro, limite de palavra, sem caixa e acento; o nome mais longo vence o que ele contém, mas só se o nome curto não aparece também sozinho: "Casa e Casa Nova" autoriza as duas). Nome citado de conta que existe mas está desligada ou é de outro Space vira `Perguntar`, nunca o padrão em silêncio; (2) conta padrão do bot; (3) única conta ligada; (4) senão `Perguntar`. Com `perguntarConta` e mais de uma conta, o padrão não vale. Só entram contas ligadas e do Space do bot. A entrada é o tipo `MensagemDaPessoa`: texto de ferramenta, e-mail, ticket ou página não tem como entrar na escolha. `ContasDoTurno::exigir` recusa conta não autorizada no pedido. **Falta** ligar isto ao turno do bot: montar a lista de ferramentas do turno só com `ContasDoTurno` e conferir de novo em cada `tool_request` (depende do `set_tools` do Motor Mexerica) e o cartão "Qual conta?".

### API do Marketplace (contrato para a tela)

Todos os métodos aceitam `spaceId` (o Space atual; sem ele, Pessoal); erros são `400` com texto que começa com um código estável (`nome_repetido: …`). Leitura: `marketCatalogo`, `marketServico`, `marketContas`, `marketBotContas`. Escrita, só do dono (Local ou aparelho com Control): `marketConectar`, `marketContaRenomear`, `marketContaDesconectar`, `marketBotContasDefinir`.

| Método | Corpo | Resposta |
| --- | --- | --- |
| `marketCatalogo` | `{spaceId?}` | `{versao, prazoConferenciaDias, categorias[], hoje, spaceId, servicos[]}`; cada serviço traz os campos da ficha mais `conectavel`, `conferenciaVigente`, `diasDesdeConferencia` e `contas` (`[{id, name, identidade}]` do Space, ou `null` com o cofre trancado) |
| `marketServico` | `{id, spaceId?}` | `{servico, spaceId}` |
| `marketContas` | `{spaceId?}` | `{spaceId, items[]}`: `Connector` público (`id`, `name`, `servico`, `tipo`, `liberacoes`, `space`, `identidade`, `auth`…) mais `bots[{id, name, ligada, padraoDoServico}]` |
| `marketConectar` | `{servico, tipo, nome?, nomePrimeira?, liberacoes?[], spaceId?}` | `{connector, proximoPasso: "entrar" \| "chave"}` |
| `marketPersonalizado` | `{name, url \| command (+ args?, env?, headers?), description?, spaceId?}` | `{connector, proximoPasso: "verificar" \| "entrar"}` |
| `marketContaRenomear` | `{id, nome, spaceId?}` | `{connector}` |
| `marketContaDesconectar` | `{id, spaceId?}` | `{removed, bots[{id, name}], revogadoNoFornecedor: false}` |
| `marketBotContas` | `{botId, spaceId?}` | `{botId, spaceId, contas[{conta, nome, servico, ligada, padraoDoServico}], perguntarConta{}}` |
| `marketBotContasDefinir` | `{botId, ligar?[], desligar?[], padrao?{servico: id\|null}, perguntar?{servico: bool}, spaceId?}` | o mesmo de `marketBotContas` |

**MCP do próprio usuário**: `marketPersonalizado` adiciona um MCP "personalizado", por link (`url`) ou por comando (`command`), pelo mesmo caminho de `installConnector` (`add_custom_connector`). A conta nasce no Space do pedido, com `servico: null`, `tipo: null` e as marcas que o app mostra como **"seu, não verificado"**: `origem: "usuario"` e `naoVerificado: true` (contas de ficha trazem `origem: "catalogo"` e `naoVerificado: false`; as duas marcas são calculadas de `servico` no `Connector` público e valem também para MCPs que o usuário já tinha). Segue as mesmas regras de qualquer conta: nome obrigatório e único por Space (conta sem serviço forma um grupo à parte das fichas), Space validado pelo Hub e **nasce desligada em todos os bots** (a pessoa liga bot a bot, por `marketBotContasDefinir` com `ligar` ou por `updateBot`, e o Hub recusa ligar em bot de outro Space; ela não aparece em `marketBotContas`, que lista só contas de ficha). Não entra no catálogo e `registryName` é recusado. `proximoPasso` é `entrar` quando o servidor pede login (então o app chama `connectorSignIn`) e `verificar` caso contrário (`connectorVerify`). **O bloqueio das ações perigosas dele fica para o filtro de ações perigosas**: até lá a conta não tem lista de ferramentas nem filtro do Hub, e o aviso "seu, não verificado" é o único freio.

Códigos de recusa: `nome_obrigatorio`, `nome_primeira_obrigatorio`, `nome_repetido`, `nome_invalido`, `ficha_vencida`, `servico_desconhecido`, `tipo_desconhecido`, `tipo_indisponivel`, `liberacao_desconhecida`, `liberacao_indisponivel`, `liberacao_obrigatoria`, `conta_de_outro_space`, `conta_padrao_invalida`, `ficha_indisponivel`, `ligar_e_desligar`, `principal_sem_contas`, `app_proprio_ausente`, `app_google_invalido`, `cofre_trancado`. O bot ganha `contaPadrao` e `perguntarConta` (`MIRROR_SCHEMA` 49).

### Google direto com o app OAuth da própria pessoa

Gmail, Agenda, Drive, Docs e Planilhas conectam **com o app OAuth que a própria pessoa cria no Google Cloud**. Nenhum ID de
cliente nem chave secreta existe no código, no catálogo ou no CI: o Google só deixa entrar apps cadastrados nele, e o app é
dela.

- **Catálogo**: o app agrupa as fichas de `catalogo_google.rs` pelo provedor Google. O guia pede criar um projeto,
  ativar as APIs dos serviços e os MCPs escolhidos, configurar a tela de consentimento, adicionar as permissões de
  leitura, criar o cliente OAuth para um agente de IA e cadastrar o ID e a chave secreta no cartão seguro.
- **Endpoint do MCP**: novas contas de Gmail, Agenda, Drive, Docs e Planilhas usam `/mcp/v1self`, o perfil oficial
  para clientes sem aprovação interativa do Google. Nesse perfil, algumas consultas podem retornar menos itens.
  Contas no perfil anterior mantêm `/mcp/v1` e o vínculo original; para usar o novo perfil, é preciso criar
  outra conta.
- **Contatos, Slides, Tarefas e Analytics**: as quatro fichas também usam o app próprio e restringem as ferramentas
  à leitura. No projeto Google Cloud, habilite só os serviços que for conectar:
  - **Contatos**: `people.googleapis.com` e o escopo `contacts.readonly`. A ficha busca contatos pessoais, sem
    pedir acesso ao perfil ou ao diretório da organização.
  - **Slides**: `slides.googleapis.com` e `slidesmcp.googleapis.com`, com `presentations.readonly`.
  - **Tarefas**: `tasksmcp.googleapis.com`, com `tasks.readonly`. O MCP não pede outra API de produto.
  - **Analytics**: `analyticsdata.googleapis.com`, com `analytics.readonly` e acesso de Leitor à propriedade
    GA4. A ficha usa o MCP remoto da [Data API](https://developers.google.com/analytics/devguides/reporting/data/v1/rest);
    não instala o servidor local nem precisa da API Admin. Os quatro escopos usam o prefixo `https://www.googleapis.com/auth/`.
- **O app da pessoa (`market/google_app.rs`)**: ID e chave secreta num registro do cofre **por Space** (`google_app.<space>`,
  selado com o slot como dado associado: copiado para outro Space, não abre). A leitura é **estrita** (`kv_read` +
  `vault::open`): texto claro ou sem selo é recusado (`app_google_ilegivel`) e nunca migrado. `marketGoogleApp` (dono, como os outros dois) devolve só
  `{configurado, clientId}`; `marketGoogleAppSalvar` (dono) recebe os dois valores uma vez, pelo cartão seguro do app, e a
  resposta traz só o ID; `marketGoogleAppRemover` (dono). O `marketCatalogo` traz `appsProprios.google` (`null` com o cofre
  trancado). Trocar ou remover o app encerra o login das contas Google do Space (outro cliente emitiu os tokens). Recibos
  (`google_app_salvar`/`google_app_remover`) nascem na mesma transação do cofre e **não guardam ID nem chave**.
- **Login (`oauth::start`)**: o servidor de login é o **fixado pela ficha** (`https://accounts.google.com`); o anúncio do
  recurso pode trazer uma barra final a mais, e só isso se releva. Sem cadastro automático: o ID e a chave do app do Space
  vão para a conta a cada login. O retorno do Google só vale pelo loopback (do app ou do Hub): a página da nuvem é
  recusada (`google_so_neste_computador`). O `Debug` do OAuth da conta oculta a chave e os tokens. Continuam valendo PKCE S256, `state`, `iss` e a conferência dos escopos devolvidos. Pede
  `access_type=offline` e `prompt=select_account consent` (permite escolher a conta e pedir o refresh token) e **não envia `resource`**
  (nem no login, nem no refresh). Só os escopos dos grupos ligados; um login por serviço.
- **Sem app cadastrado** no Space, `marketConectar` recusa com `app_proprio_ausente` e o app abre o guia.
- **Escopos mínimos**: Gmail ler = `gmail.readonly`, rascunhos = `gmail.compose` (nunca `gmail.modify`/`gmail.send`); Agenda
  ler = `calendar.calendarlist.readonly` + `calendar.events.freebusy` + `calendar.events.readonly`; Drive ler =
  `drive.readonly` (o `drive.file` sozinho só enxerga o que o próprio app criou), criar = `drive.file`; Docs ler =
  `documents.readonly`; Planilhas ler = `spreadsheets.readonly`. Os grupos de escrita ficam **indisponíveis**:
  o Hub não pede seus escopos. Ferramentas sem curadoria precisam de liberação por conta e bot, inclusive as de
  leitura. Autorizar uma ferramenta no Hub não amplia o escopo do token.
- **Conta real**: em teste externo, adicione as contas em Usuários de teste; o Google pede login de novo após sete
  dias. Para disponibilizar a terceiros, o Google pode exigir verificação dos escopos sensíveis ou restritos.
  O [guia oficial](https://developers.google.com/workspace/guides/configure-mcp-servers) também documenta App para
  computador e o modo pessoal para uma conta. Login, leitura e renovação dos tokens ainda precisam de teste com
  uma conta real; use duas contas para confirmar que cada login retorna os dados da conta selecionada.
- `MIRROR_SCHEMA` sobe para 51 (`appsProprios` e `auth.modo: "app_proprio"`).
- `MIRROR_SCHEMA` sobe para 53 (`skills[]` das fichas deixa de vir vazia: `{id, titulo, resumo}`; o catálogo é lido sob demanda e não entra no mirror, e o app novo não mostra a seção quando a lista não vem).

### Marketplace na tela

O app desenha as telas aprovadas **só com o que o Hub já entrega** (`marketCatalogo`, `marketContas`, `marketConectar`, `marketContaRenomear`, `marketContaDesconectar`, `marketBotContas`, `marketBotContasDefinir` e o fluxo `connectorSignIn`/`connectorSignInFinish`/`connectorVerify` ou `credentialUpdateConnector`). Nenhum campo novo do Hub na época; `MIRROR_SCHEMA` estava em 50. O código novo mora em `apps/desktop/src/renderer/views/fork/mercado/`; os ganchos no código herdado são `MarketplaceView.tsx` (a página Conectores passa a ser a `PaginaConectores`) e `FerramentasEmAbas.tsx` (a tabela "Contas que o bot usa" acima das abas).

- **Extensões → Conectores**: "Catálogo | Minhas contas N". Busca, categorias (cada uma com o número de serviços, contando os "em breve"; escolher uma mostra os conectáveis e os "em breve" dela), a lista "Todos os conectores" agrupada por categoria, na ordem que o Hub manda (todos com o mesmo cartão, sem serviço em destaque), selos discretos ("Oficial · 2 jeitos de conectar"), "N contas" ou Reconectar por serviço e o bloco "Em breve", sem data. O Meus MCP antigo continua, atrás do link "Adicionar um conector seu (não verificado)".
- **Ficha**: exemplos, jeitos oficiais (o recomendado já marcado), o que dá para liberar, contas do Space, links do fornecedor (site, documentação, privacidade e termos), as skills do catálogo curado que a ficha sugere (só nome e resumo; texto passivo, sem acesso, nunca ligadas sozinhas; o botão leva a Extensões → Skills), as skills que o fornecedor publica (só o link e a licença) e "Conferida pela Mexerica em". Na ficha "em breve" aparece **Por que ainda não conecta**: o motivo da ficha (por exemplo, no Exa, a conta também libera execuções cobradas). Ficha vencida (mais de 90 dias) não conecta; o Hub também recusa (`ficha_vencida`).
- **Conectar**: sem seletor de Space (vale o Space atual). Nomes: a 1ª conta não pede nome; da 2ª em diante o nome é obrigatório para a nova e, enquanto a 1ª ainda se chama só pelo nome do serviço, para ela também. O app só decide quais campos mostrar; quem confere é o Hub, e as recusas aparecem pelos códigos estáveis (`nome_repetido` cai no campo do nome). Liberações em grupos com o padrão seguro marcado; **Personalizar** só aparece com mais de 3 grupos na ficha. Depois do login (ou da chave guardada no cofre do Hub) o app pede `connectorVerify` antes de dizer "conectada". Desistir no meio desconecta a conta pela metade. Em seguida, "Usar em quais bots?" com todos desmarcados; as skills sugeridas da ficha aparecem também aqui (só nome e o que fazem; não ligam sozinhas). Cada bot aparece com o rosto, o nome e a função (`Bot.title`), agrupados pela equipe de Times do Space (título com o caminho, ex. "Marketing › Social"; "Sem equipe" no fim; sem dados de equipes, uma lista só) e com **Marcar todos** por equipe; a busca (a partir de 6 bots) filtra por nome, função e equipe. Tudo vem do que o app já recebe (lista de bots, grafo de Times e `principal.protectedId`): nenhum campo novo do Hub, `MIRROR_SCHEMA` fica em 50. O **principal do Space nunca é marcável**: aparece no fim, desabilitado, com o motivo na linha. O Hub é quem recusa: `validar_vinculo` (usado por `updateBot`, `marketBotContasDefinir` e pelo "ligar depois de conectar" do pedido de conexão) devolve `principal_sem_contas` para conta de serviço (de ficha) nova ou padrão novo em qualquer principal (o Berga do Pessoal ou o de um Space, pelo id gravado, nunca pelo nome); tirar conta continua permitido, os MCPs próprios que o dono liga no Berga seguem pela `donna_fixa` e os gatilhos dela continuam ligados. O lote **ligar em todos** (`extensaoEmTodosPrevia`/`Aplicar`, que grava por SQL fora do `validar_vinculo`) também deixa o Berga do Pessoal de fora para conta de serviço (motivo `principal_sem_contas`, na prévia e de novo dentro da transação), como já fazia com o principal de um Space; pedir o principal à mão no `Aplicar` recusa com o mesmo código.
- **Contas no bot**: tabela de leitura única, uma linha por serviço (Serviço · Contas · Se o pedido não diz a conta · Ações perigosas), com os detalhes ao abrir a linha (contas ligadas, conta padrão ou "Perguntar qual conta").
- **Logos**: cada serviço do catálogo mostra a logo do fornecedor, em arquivo local do app (`apps/desktop/src/renderer/assets/marcas/`, mapeado por `views/marketplace/marcas.ts`; a chave é o campo `marca` que o Hub manda em cada serviço e em cada conta); nada vem da rede e, sem arquivo, o globo. Origem, data e uso em `THIRD_PARTY.md` e `TRADEMARK.md`. O teste `marcas.test.ts` reprova ficha sem arquivo de logo, logo fora de `marcas.ts` e arquivo sem linha em `THIRD_PARTY.md` com o SHA-256 igual ao do arquivo.
- **Adicionar outra conta**: botão na ficha (quando já há conta) e em cada serviço de Minhas contas; o nome da nova já vem sugerido ("Linear 2"), editável, e o Hub segue conferindo nome repetido.
- **Minhas contas**: renomear na própria linha, reconectar a que expirou e desconectar só uma. O diálogo lista os bots que a usavam e diz que **só se apaga o acesso neste computador**: o Hub devolve `revogadoNoFornecedor: false` e a tela nunca afirma o contrário.
- **Ações perigosas (única diferença das telas aprovadas)**: nenhuma ficha tem ação perigosa disponível hoje, e as chaves só funcionam com o filtro de ações perigosas. Por isso o bloco (tela 03) e a coluna (tela 05) só **listam as ações da ficha como "Desligadas"**, somem quando a ficha não tem nenhuma e **não têm chave de ligar**. Elas entram junto do campo novo do Hub (`MIRROR_SCHEMA` 50).

## Infraestrutura de chamadas curadas

O Hub possui um caminho separado de tentativa, cartão nativo e recibo por chamada
curada. **Ainda indisponível em produção:** faltam a fonte canônica de instância,
perfil e acessos, a apresentação integral e a integração da prova
humana comum. `decisionProtocol:0` recusa antes de cofre,
tentativa, cartão pendente ou rede. Versão/source enviados
no pedido e annotations MCP não habilitam o caminho. Nenhum catálogo, conta,
Linear ou novo login é entregue por esta infraestrutura.

A prova de integração é exclusivamente sintética, em testes: Pessoal, turno
humano e operador `Caller::Local` (inclusive o SSH oficial). Device, Control,
`owner.devices`, `engine.accounts` e voz não leem detalhes privados nem decidem.
Essas fixtures provam o fluxo técnico de efeitos; Local, `source=app` e scope
não comprovam gesto humano. HumanReviewProof terá produtor único,
ainda não integrado e sem API positiva estável aceita.
Esta infraestrutura mantém somente o ledger específico de efeitos de chamadas curadas de
Conector, sem produzir prova ou oferecer aprovação genérica a outros domínios.
O cartão nativo não aceita `respondPermission` e nunca vai ao motor. Editar
somente os campos declarados substitui snapshot/cartão, sem renovar os 90 segundos.
A apresentação do protocolo de decisão 2 traz a ação curada, o rótulo curado de cada
campo e a conta de saída da fonte canônica, vinculada à concessão e revalidada até a
admissão; troca de conta ou repareamento invalida o cartão. App do protocolo 1 não
mostra botões e o Hub recusa sua decisão. Nome da ferramenta e argumentos seguem
exatos no pedido e no digest; rótulos não concedem acesso nem mudam a edição.
O recibo distingue sucesso reportado, falha sem efeito comprovada e efeito incerto;
incerto exige conferência antes de novo pedido, sem retry automático.

Este caminho não amplia a guarda de efeitos para Meus MCP, terminal ou
ferramentas nativas. Não comprova isolamento ou presença humana diante de outros
processos com acesso ao mesmo bearer Local.

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
par OAuth está descrita na seção abaixo. Esta sonda não faz GET legado;
401 disponível somente nesse GET não orienta o pedido nesta etapa. `required`,
ping e fallback SSE M1 existentes continuam; nenhum `tools/call`, step-up
automático, seleção de bots ou mudança da versão MCP foi adicionado.

Fonte: [MCP vigente, Scope Selection Strategy](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization#scope-selection-strategy),
consultada em 06/11/2026; sintaxe de scope conforme RFC 6749 §3.3.

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
fora desta entrega. Não há campo público novo nem mudança de interface.

Fontes verificadas em 06/11/2026: [RFC 6749 §§3.3/5.1/6](https://www.rfc-editor.org/rfc/rfc6749.html),
[RFC 6750](https://www.rfc-editor.org/rfc/rfc6750.html) e
[MCP vigente, autorização](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization).


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
step-up, perfis mínimos e a fonte canônica de perfis continuam
posteriores. Fonte: [MCP, vínculo ao servidor de autorização](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration#authorization-server-binding).

Enquanto Aplicativos cobre a barra/conversa, os
rostos dessa base pausam a vida automática e suas animações pelo mecanismo já
existente. A base mantém layout, estado e posição de rolagem; ao voltar, os rostos
retomam. Rostos visíveis no rail ou em folhas não entram nessa pausa. O botão Carregar mais mantém o movimento de pressão, mas seu estado carregando
muda a opacidade imediatamente, sem prolongar um fade entre páginas. Nenhuma
mudança no motor/autoridade do Hub.

A primeira lista já usa as colunas da largura real, medida enquanto aparece o
indicador de carregamento. Se a área ainda está oculta ou sem largura, o indicador
permanece; ao aparecer, a lista e a paginação automática continuam. Redimensionar,
aumentar a fonte, buscar e voltar de Conectar mantêm os comportamentos existentes.

### Na tela

Em Extensões, cada skill instalada (Skills → Instaladas) e cada conector MCP instalado (Conectores) tem o botão **Ligar em todos os bots**: a prévia abre no mesmo lugar, embaixo da linha, com a lista de quem recebe e, à parte, quem fica de fora com o motivo em linguagem simples, e pede **um** OK. Conflito do Hub (`extensoes_em_todos_conflito`) refaz a prévia sozinho com "A lista mudou. Confira de novo."; 401/403 mostra a frase de permissão (só o dono). Só as skills têm o interruptor **Novos bots já vêm com esta skill** (lê `novosBots` da lista de skills); conectores não têm. A lógica fica em `views/fork/extensoes/em-todos.ts` (testada sem DOM); o desenho segue o componente `ExtensoesEmTodos`. Fora desta entrega: contas de Aplicativos (Composio), que têm uma ou mais contas por app.

### Informação da instalação de uma skill

Na **Base central** ou em **Instaladas**, abra uma skill para ver o resumo da
instalação no Pessoal deste Hub. Ele usa a mesma lista e o recibo da versão
instalada; não troca para a versão mais nova do catálogo. O endereço exibido é
lógico, não um caminho do sistema nem prova de que só um bot possa ler os arquivos.

A folha tem uma só **Conferência**, com o botão **Verificar agora**. **Igual ao
recibo** vale para o SKILL.md na leitura apresentada, enquanto a conexão e a
instalação continuarem as mesmas. Uma queda, nova conexão ou outra instalação troca
o estado para **Leitura anterior · não confirmada nesta conexão**. **Diferente do
registrado** e **Não conferido** não atribuem uma causa que o Hub não informou. A
conferência não atesta os arquivos auxiliares nem a segurança das instruções. O
manifesto mostra arquivos declarados no recibo, sem varredura do disco ou
conferência atual dos auxiliares; metadados incompatíveis ficam indisponíveis.

Datas de atualização, anúncio aos bots e entrega do texto continuam desconhecidos
ou sem medição. O uso aparece na seção **Uso** só quando o Hub o informa na leitura
atual; sem isso, aparece "Sem medição" (ou "Desconhecido" numa leitura anterior).
Aparecer na lista não comprova uso. O resumo não acrescenta ações
de instalação, remoção ou seleção. **Comparar rascunho** continua no fluxo existente,
por gesto, sem salvar uma revisão nem publicar alterações para os bots.
