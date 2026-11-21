# Bot workspaces

New agent bots get a personal, persistent workspace automatically. Creating a bot does not require choosing a project. An omitted or empty `cwd` in `createBot` allocates `<DONNABOT_HOME>/bots/<bot-id>/workspace` (normally `~/.donnabot/bots/<bot-id>/workspace`). Groups do not allocate folders. An omitted or empty `backend` prefers Codex on the Hub; only when unavailable or confirmed signed out does it fall back to Claude or another runnable curated agent. Unknown sign-in state does not trigger a switch; with no ready alternative, Codex remains selected for setup. The desktop refreshes the Hub engines before enabling Create and sends the engine it shows; the terminal, the iPhone and Berga send it empty and the Hub resolves the engine (rechecking a Codex marked signed out). Explicit user engine choices are preserved. Omitted `connectors`/`logins` start empty; only explicit user selections grant access. Skills still default to every installed one. Existing bot engines and access are unchanged.

The host persists the resolved absolute directory in `cwd` and exposes `managedWorkspace` in bot responses. Empty `cwd` in `updateBot` selects the same bot's personal workspace again. Templates for personal bots send an empty cwd so a new bot gets its own directory.

## Validação da pasta no Hub

Ao criar ou mudar a pasta, toda pasta explícita passa pela mesma validação do Hub
(`fork::pasta`). O caminho precisa ser absoluto, existir e ser uma pasta. O texto
não pode estar vazio, ter controles ou ultrapassar 1024 bytes. `~` usa a pasta
pessoal do usuário do Hub; links simbólicos são resolvidos antes da validação e
o caminho canônico é salvo.

São recusados `/`, a própria pasta pessoal, qualquer pasta oculta no primeiro
nível dela (incluindo `.ssh`, `.config`, `.codex`, `.claude`, `.local`, `.cache`,
`.gnupg`), os dados internos do Hub e os diretórios `/etc`, `/proc`, `/sys`,
`/dev`, `/boot`, `/root`, `/var/lib`, incluindo suas subpastas e links para eles.
Também são recusados os ancestrais da pasta pessoal, dos dados do Hub e desses
diretórios de sistema, além das pastas de outros usuários em `/home` e `/Users`.
Dentro dos dados do Hub, só são aceitos o workspace próprio do bot
(`bots/<id>/workspace`) e as pastas gerenciadas de canal (`grupos/<id>/pasta`),
incluindo suas subpastas. A exceção também é conferida no caminho real.

Bots antigos com uma pasta agora recusada continuam carregando com o mesmo
caminho e arquivos. O Hub publica **Revise a pasta** no chat, sem repetir o mesmo
aviso após reiniciar, e envia `workspaceIssue` no sync para o editor exibir o
motivo. Alterar outros campos preserva a pasta salva e o aviso, inclusive em
canais. Para mudar a pasta, escolha uma válida ou volte ao espaço pessoal do bot.
A pasta inalterada mantém sua grafia, sem reiniciar o agente por normalização.
Nenhum arquivo é apagado ou movido automaticamente.

A validação cobre a configuração e preserva a compatibilidade dos bots antigos.
A revalidação por turno e a trava de pasta dos canais ficam para uma etapa
futura de pasta compartilhada. A pasta inicial não isola o shell nem limita por si só o acesso
do processo a outros arquivos.

## Pastas escolhidas pelo Berga

A ferramenta `create_bot` tem uma política mais restrita que a seleção feita pelo
usuário: somente subpastas de `workspaceRoot`, configurada pelo usuário em
`setTeamSettings`, com padrão `~/Donnabot/workspaces`. Sem `folder`, cria uma
subpasta nova nessa raiz. Links simbólicos são resolvidos; a pasta da principal,
os dados da Mexerica não podem ser usados como a pasta do novo bot. Subpastas
da raiz são permitidas mesmo se a principal trabalhar em HOME ou em ~/Donnabot.
A raiz passa pela validação comum antes de pedir aprovação, mesmo se ainda não
existir. Se a criação falhar, a pasta vazia recém-criada é removida; pastas que
já existiam e arquivos são preservados.
Bots existentes e pastas escolhidas diretamente pelo usuário mantêm o comportamento descrito acima.
Esta validação da ferramenta não é isolamento do shell.

## Task execution

The configured directory is the agent process and ACP session's starting directory. The bot's instructions explain that it can use explicit paths or change directory for an individual command when a task concerns another folder. This does not modify bot configuration or reset the conversation. Access remains subject to the harness's permissions; a personal folder is not a sandbox and does not grant additional access.

Routines and group contributions use their owner's configured starting directory. Task-specific paths should be explicit in routine instructions. The host does not infer a task directory from the last chat message. Harness-specific restrictions can still require user approval for other paths.

Changing the default project in settings deliberately reconfigures the agent session, as before. This is separate from executing a task in another directory.

## Lifecycle

- Renaming a bot or changing its model does not move or erase workspace files.
- Restarting the host or starting a new conversation context preserves the directory.
- Switching to a project and back reuses the personal workspace.
- Deleting a bot leaves its workspace files on disk. There is no automatic cleanup.
- Existing bots are not silently moved out of their configured project directories.

Swift settings offer Personal or Choose project folder; desktop quick creation and the terminal's new-bot flow default to Personal. Directory selection is optional. Separate workspaces do not prevent conflicts when multiple bots edit the same external project; automatic Git worktrees are not implemented here.

### Pastas privadas

`~/.mexerica*`, `~/.donnabot*` e `~/.codync*`, incluindo Dev, aliases e backups
com segredos, não são pastas de trabalho. O override atual continua protegido;
as exceções gerenciadas são decididas pelo Hub. Não contorne uma recusa por shell,
links, outro bot ou outro usuário. O novo default e `MEXERICA_HOME` ficam para uma etapa futura.
Veja os [limites de UID/permissões](seguranca-dos-motores.md).
