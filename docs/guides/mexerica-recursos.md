# Recursos próprios da Mexerica

A Mexerica é um fork do Codync. Para rodar no mesmo computador que um Codync ou Codync Dev
instalados, ele usa pastas, porta, IDs, labels e cofre próprios. Os dois não compartilham
dados nem serviços, e a Mexerica não migra nada do Codync.

O nome que aparece na tela e o que chega ao modelo é "Mexerica"; a bot principal se chama
**Berga**. Identificadores internos
continuam com o nome do upstream para facilitar ports (veja [O que não muda](#o-que-não-muda)).

A bot principal de cada Hub tem um cartão fixo no topo da barra lateral, com
estado do atendimento e não lidas, também no trilho compacto. Busca e seções não
o escondem. Veja [A bot principal na barra lateral](../features/donna-central.md).

## Criar Space com seu próprio principal

No novo fluxo do desktop com Hub compatível, **Criar space** pede nome e aparência. Em
**Continuar**, escolha o nome do principal, o motor e uma conta de IA conectada
naquele Hub. Confira a conta exibida e marque a confirmação antes de
**Criar Space e principal**. A conta padrão do Hub é sugerida visivelmente;
nenhuma credencial é copiada. Nesse fluxo, cada Space criado recebe um bot e memória próprios.

Nesse novo fluxo, uma falha não deixa metade da criação. Se a resposta se perder, **Tentar o mesmo
pedido** consulta a mesma intenção, sem duplicar o bot. Para mudar os dados,
feche e abra novamente. Hub antigo informa que precisa de atualização.
O endpoint legado `donnaSpaceCreate` continua podendo criar somente metadados; Spaces existentes não recebem principal automaticamente. Isso não é garantia de principal em todo Space. O seletor que trocará
todo o app, Equipes, Projetos, Rotinas e conectores isolados têm desenvolvimento próprio.

## Abas no topo e painel direito

O topo da conversa é um cartão de abas: a primeira é o bot (ou o canal), a
conversa principal; depois vêm as sessões abertas, com um ponto de estado
(rodando pulsa, pede você toca em âmbar, livre fica verde). As que não cabem vão
para **···**, que abre o painel em Sessões. **‹‹/››**, ou dois cliques na aba do
bot, recolhem as sessões até a pílula do bot. ⌘1 é a conversa principal,
⌘2…⌘9 as abas, e Esc volta para a principal. O "×" fecha só a aba; a sessão
continua no painel. O botão da barra lateral (⌃⌘S) fica no cartão, à esquerda.

Botão direito ou Shift+F10 nas abas e na lista de Sessões abre as ações de
cópia e arquivamento. Escape devolve o foco. Excluir uma extra pede confirmação
e depende do anúncio do Host; durante trabalho pendente, é preciso encerrar antes.
A exclusão preserva memória e outras sessões e rotinas. Veja também
[Excluir uma sessão](../features/excluir-sessao.md). Veja [Menus de sessões](../features/menu-sessoes.md).

O painel direito abre ao passar o mouse na borda direita da janela e fecha ao
sair; o alfinete, ⌘I, dois cliques numa aba de sessão ou um clique na alça o
fixam ao lado da conversa (a escolha fica guardada). No bot: cabeçalho com a
engrenagem (Configurações do bot, com Memória e Compartilhar), **Ligar** quando
há voz, **Agora** (o que roda e o que pede você, com Aprovar segurando e Ver;
recolhível) e as abas **Sessões** (árvore com a conversa principal e as sessões,
Arquivadas e Atividade recente) e **Rotinas**. No canal: **Sessões**, **Membros**
(você e os bots, estado, pílula Líder, tirar do canal, adicionar) e
**Configurações** (nome, descrição e líder do canal).

Enquanto o Hub não recebe mensagens dentro de uma sessão, a aba de sessão mostra
o histórico dela e, no lugar da caixa de mensagem, um aviso com **Voltar à
principal**. Nova sessão, "+" e Arquivar aparecem quando o Hub tiver essas
funções. Com a janela estreita, o painel fixo abre por cima da conversa.

## Tabela de recursos

| Recurso | Mexerica | Mexerica Dev (nome técnico `Donnabot Dev`) | Codync (nunca usar) |
| --- | --- | --- | --- |
| Nome visível | `Donnabot` | `Donnabot Dev` | `Codync`, `Codync Dev` |
| Binário do host | `donnabot-host` (Linux: `~/.local/bin/donnabot-host`) | `donnabot-host` (Linux: `~/.local/bin/donnabot-dev-host`) | `codync-host`, `codync-dev-host` |
| Pasta de dados do host | `~/.mexerica` | `~/.mexerica-dev` | `~/.codync`, `~/.codync-dev` |
| Sandboxes do `npm run dev` | — | `~/.mexerica-dev-<worktree>` | `~/.codync-dev-<worktree>` |
| Variáveis de pasta, porta e binário | `MEXERICA_HOME`, `DONNABOT_PORT`, `DONNABOT_HOST_BIN` | as mesmas | `CODYNC_HOME`, `CODYNC_PORT`, `CODYNC_HOST_BIN` |
| Porta local do host | `19422` | `19423` | `19222`, `19223` |
| Portas das sandboxes | — | `19430`–`19499` | `19300`–`19399` |
| launchd do host (macOS) | `app.donnabot.host` | `app.donnabot.dev.host` | `com.pokai.codync.host`, `com.pokai.codync.dev.host` |
| Job do atualizador (desligado) | `app.donnabot.update` | `app.donnabot.dev.update` | `com.pokai.codync.update` |
| Serviço systemd (Linux) | `donnabot-host.service` | `donnabot-dev-host.service` | `codync-host.service`, `codync-dev-host.service` |
| Unidade do atualizador (Linux) | `donnabot-host-update` | `donnabot-dev-host-update` | `codync-host-update` |
| Run key e supervisor do Windows | `DonnabotHost`, `donnabot-hostw.exe` | `DonnabotDevHost`, `donnabot-dev-hostw.exe` | `CodyncHost`, `codync-hostw.exe` |
| appId do Electron | `app.donnabot.mac` | `app.donnabot.mac.dev` | `com.pokai.Codync`, `com.pokai.Codync.dev` |
| `productName` e pasta `userData` | `Donnabot` | `Donnabot Dev` | `Codync`, `Codync Dev` |
| Trava de instância única e chave do `safeStorage` | pela pasta `userData` e pelo nome `Donnabot` | idem, `Donnabot Dev` | as do Codync |
| Esquemas de URL | `donnabot://`, `app.donnabot.mac://` | `donnabot-dev://`, `app.donnabot.mac.dev://` | `codync://`, `codync-dev://` |
| Helper de tela (macOS) | `DonnabotScreen.app`, bundle e label `app.donnabot.screen` | `DonnabotDevScreen.app`, `app.donnabot.dev.screen` | `com.pokai.Codync.screen` |
| Socket do helper de tela | `~/.mexerica/screen.sock` | `~/.mexerica-dev/screen.sock` | `~/.codync/screen.sock` |
| Socket do cua-driver | `$TMPDIR/donnabot-cua/cua.sock` (Linux: `$XDG_RUNTIME_DIR/donnabot-cua`) | `donnabot-dev-cua` | `codync-cua`, `codync-dev-cua` |
| Serviço do cofre (Keychain / Secret Service) | `app.donnabot.credentials` | `app.donnabot.dev.credentials` | `dev.codync.credentials`, `dev.codync.development.credentials` |
| Job do atualizador do Electron (desligado) | `app.donnabot.mac.ShipIt` | `app.donnabot.mac.dev.ShipIt` | `com.pokai.Codync.ShipIt` |

Dentro da pasta de dados ficam o token local, o banco (o arquivo mantém o nome `codync.db`
do upstream), `screen.sock`, o registro e os agentes baixados, `bots/`, `skills/`, `node/`,
`host.log` e `host.lock`.

As definições ficam em `host/src/environment.rs` (host), `apps/desktop/src/shared/environment.ts`
e `apps/desktop/electron-builder*.yml` (app) e `apps/project.yml` (helper de tela).

## Travas de convivência

- O host recusa `serve`, `install` e a abertura do banco quando a pasta de dados está em
  `~/.codync*` ou a porta é do Codync (19222, 19223, 19300–19399). A pasta é resolvida antes
  (symlinks, `..`, caminho relativo) e comparada sem diferenciar maiúsculas. No `install`, essas
  travas rodam antes de qualquer efeito (banco, status line, serviço).
- O app recusa porta ou pasta de dados do Codync, também ao rodar com `DONNABOT_PORT` e
  `MEXERICA_HOME`.
- O app só usa um host chamado `donnabot-host`: o que vem dentro dele, `DONNABOT_HOST_BIN`, os
  builds da própria worktree (dev) ou `~/.local/bin/donnabot(-dev)-host`. Nunca o PATH, o
  Homebrew ou um `codync-host`.
- Pelo SSH, o computador remoto só é consultado por caminho absoluto (`~/.local/bin/donnabot-host`
  e o binário dentro do pacote da Mexerica ou, numa instalação antiga, do pacote Donnabot); o PATH remoto nunca
  escolhe o host.
- A status line do Claude Code envolvida por um `codync-host` (o do Codync, exista ou não o
  binário) não é tomada pelo `install` nem desfeita pelo `uninstall`. Um `donnabot-host` em
  qualquer caminho é da Mexerica.
- `MEXERICA_HOME` é o único override de dados. A presença de `DONNABOT_HOME` ou `CODYNC_HOME` recusa a seleção, mesmo vazias ou iguais ao override; remova a configuração antiga explicitamente. Não há fallback nem importação de dados antigos.
- Os configs de conta (`apps/desktop/config/*.plist`) não trazem a nuvem nem a chave do Clerk do
  upstream: o login fica "não configurado" até a nuvem própria.

## Extensões e Composio

Na tela de **Extensões**, a página **Conectores** é o **Meus MCP**, que lista conexões MCP nativas e permite
adicionar por URL, comando ou config; a página **Aplicativos** não tem mais catálogo próprio e aponta para
Conectores, e **Skills** é a terceira página. MCP Registry e suas consultas/atualizações foram retirados; conexões
antigas são preservadas. Os agentes ficam em Ajustes → Contas de IA.

A integração dedicada do Composio (chave de projeto, usuário, catálogo e contas por bot) foi removida em 16/11/2026.
O Composio passa a ser um conector comum, conectado pelo MCP oficial dele em **Extensões → Conectores → Adicionar MCP**.
Os dados da integração antiga são apagados por uma migração única na partida do Hub
(ver [marketplace](../features/marketplace.md#composio-integração-dedicada-removida)).

## Integrações desligadas

- Host local no Mac: os bots rodam só no Hub. O app não instala nem inicia o
  `donnabot-host` (o serviço `app.donnabot.host` não é criado) e se conecta ao perfil SSH do Hub
  configurado pelo operador. Veja [App da Mexerica no Mac](app-mac-local.md).
- "Computer access" e o interruptor "Use the computer" dos bots: escondidos no app enquanto
  `forkFeatures.localHost` for `false`. O ajudante do Mac (tela Computadores) vai precisar
  de uma chave própria em `showsComputerAccess` para trazê-los sem host local.

## Como rodar em desenvolvimento

- No Mac, use uma worktree para o código e gere o cliente com `dist:mac:local`, conforme
  [App da Mexerica no Mac](app-mac-local.md). O app instalado conversa com o Hub remoto.
- Compilação, formatação, Clippy e testes do Hub rodam no servidor pelo roteiro do operador.
  `npm run dev` também compila o Hub localmente e não deve ser usado no Mac neste fluxo.
- O modo Dev herdado mantém seus próprios dados e portas quando usado em um ambiente de
  desenvolvimento apropriado. Ele não é uma instalação nem uma validação do Hub real.
- O cliente local não instala serviço de Hub no Mac. Para parar o Hub remoto, siga o
  procedimento do operador no servidor. Não encerre processos de outros aplicativos.

## O que não muda

- O pacote Rust `codync-host` (só os binários mudaram de nome), os nomes de crates e arquivos,
  `CODYNC_ENV` (escolhido na compilação) e outras variáveis `CODYNC_*` de ferramentas de
  desenvolvimento.
- Os nomes internos dos servidores MCP (`codync-chat`, `codync-team`…), o cabeçalho de
  protocolo `Codync-Sig` e as fontes CodyncBot/CodyncOrb.
- O app do iPhone mantém a identidade do upstream até o trabalho de mobile; o pareamento pelo
  iPhone vem depois.

## Pendências

- As listas de recursos recusados do Codync estão duplicadas em Rust (`host/src/environment.rs`),
  TypeScript (`apps/desktop/src/shared/environment.ts`) e Swift (helper de tela); unificar depois.
- Windows não está no plano: além do rename dos binários, nada foi adaptado nem testado.

- Assinatura do macOS: o app do Mac sai de
  `npm run dist:mac:local`, com assinatura ad-hoc (ou um certificado local explícito), sem
  notarização e sem a identidade do upstream ([App da Mexerica no Mac](app-mac-local.md)).
  Com assinatura ad-hoc, o macOS liga as permissões (microfone, Keychain e, no futuro, as do
  ajudante de tela) à assinatura de cada build, então elas são pedidas de novo a cada
  recompilação. `dist:mac:main`/`dist:mac:dev` ainda empacotam o host e o helper de tela.
- O helper de tela só roda empacotado (`DonnabotScreen.app` dentro do app); `npm run dev` não
  o registra.


## Troca de pasta sem movimento automático

O padrão é `~/.mexerica` em Main e `~/.mexerica-dev` em Dev. Override relativo
usa o CWD da invocação; use caminho absoluto quando o destino importa. Componentes com `..`
são recusados pela abertura privada do Hub. Diretórios próprios novos usam 0700;
lock, token, identidade e banco novos usam 0600 antes de conteúdo. Symlinks e
dono impróprio são recusados na criação Unix, sem alterar UID/GID existentes.

As famílias antigas, novas e backups permanecem negadas enquanto houver bytes,
inclusive grafias em maiúsculas e aliases. Nomes de cofre, banco `codync.db`,
recibos, usuários e serviços ficam iguais. O movimento de dados é manual.
Não iniciar versão nova em pasta
vazia e chamar isso de migração.

No desktop, Node usa pai próprio 0700, O_NOFOLLOW, fstat/fchmod e fsync pelo fd.
A troca concorrente do **pai** entre conferência e open permanece possível;
essa implementação não equivale a openat. No Hub, a criação usa descritores de
pai e openat/mkdirat, sem seguir links. Isso não é uma promessa de isolamento
quando motor e Hub compartilham UID, nem prova de instalação/cofre/SSH reais.
