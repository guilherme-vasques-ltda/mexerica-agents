# Development and validation

O site fica em repositório separado. A demo do desktop é gerada com `cd apps/desktop && npm run build:demo` em `apps/desktop/dist/demo`, ignorada pelo Git; pode ser consumida pelo repositório do site depois, sem publicação neste fluxo.

Run commands from the repository root unless a block changes directory. See [file structure](../architecture/file-structure.md) for ownership.

## Desktop app

The macOS, Linux and Windows app is the Electron app in `apps/desktop/`; setup, UI-check switches and packaging are in [desktop app](../architecture/desktop-app.md#development):

```sh
cd apps/desktop
npm ci
npm run icons      # macOS only
npm run dev
```

Before launching a new build, quit only that variant. Production and Dev may coexist; see [Dev app isolation](dev-app.md). For a development rebuild:

```sh
osascript -e 'tell application id "app.donnabot.mac.dev" to quit'
pkill -x "Mexerica Dev"
```

`pkill` can return nonzero when no process exists.

The desktop app (`apps/desktop/src/main/host-controller.ts`) compares the running
host's executable path and SHA-256 fingerprint with the host it would install (the
bundled one in a packaged Mac app, else `DONNABOT_HOST_BIN` or an installed
`donnabot-host`), so reopening after a rebuild replaces the service even when the
version number has not changed. **Restart host** runs `donnabot-host install` again,
which waits for the old host's data lock to be released. If a manually started host
still owns that data directory, installation fails instead of reporting a
successful restart; stop that host in its terminal and retry. Hosts attached using
`CODYNC_PORT` are managed manually.

On SIGTERM the host stops its bots without waiting for open SSE or WebSocket
connections. ACP adapters run in separate process groups. Stopping an adapter
closes its stdin and gives its group 10 seconds to exit by itself, so a Claude
Code CLI can finish saving a refreshed login. Tools and MCP servers still left
in the group after that are killed, and the host waits for this before it exits.
Processes that detach into their own sessions and unrelated hosts using other
data directories are outside this cleanup.

After rebuilding the host, restart the service (macOS launch agent shown; Linux
uses the environment-specific systemd user unit). Stop only test hosts you started; do not kill another environment's host. For Dev:

```sh
launchctl kickstart -k gui/$(id -u)/app.donnabot.dev.host
pgrep -fl donnabot-host
```

## Component checks

| Component | Commands | Requirements |
| --- | --- | --- |
| Host | `cd host && cargo build` | rustup; `rust-toolchain.toml` pins the version CI uses (a Homebrew `rust` ahead of rustup on PATH ignores it) |
| Host checks | `cd host && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` | Local agent credentials are unnecessary for unit tests |
| Desktop | `cd apps/desktop && npm ci && npm run typecheck && npm test` | Node and npm; CI uses Node 24 |
| Idiomas | dentro do `npm test` do desktop; à parte: `cd apps/desktop && node --experimental-strip-types tools/textos-gate.ts` | Catálogos pt-BR/en e texto visível novo fora deles; ver `docs/guides/idiomas.md` |

Platform validation covers the Linux screen helper and the macOS `Screen` target with WebRTC.

### Entradas externas (Mexerica)

Um arquivo de documentação embutido no binário é entrada de código. Os consumidores abaixo leem arquivos de fora de sua própria pasta:

| Alvo | Entradas fora de sua pasta |
| --- | --- |
| Host | `docs/features/manual-do-harness.md`; `docs/reference/fixtures/remote-relay-vectors.json`; `apps/desktop/src/renderer/lib/donnabot-avatars/schema/avatar-config.schema.json`; `apps/desktop/src/renderer/lib/donnabot-avatars/presets/roles.json`; `apps/desktop/src/shared/fork-avatar-data.json`; `apps/desktop/src/shared/fork-mexeriquinha-data.json`; `apps/desktop/src/shared/fork-mexeriquinha.schema.json`; `apps/desktop/src/shared/fork-mexeriquinha-wire-fixture.json`; `rust-toolchain.toml` |
| Desktop | `apps/desktop/config/`; a fixture `remote-relay-vectors.json`; `apps/desktop/design/tokens.json` e `tokens.css`; `apps/desktop/design/assets/Icones/`; `host/src/fork/skill_catalog/registry.json` (importado pelo demo incluído no typecheck) |
| screen_linux | `rust-toolchain.toml` |

Os cinco primeiros inputs externos do Host vêm de `include_str!`/`include_bytes!`.
Não há `build.rs` no Host ou no helper Linux; as fixtures lidas pelos testes Rust
ficam dentro de `host/`. Consulte `AGENTS.md` antes de commitar; não use
filtros de teste para liberar um gate obrigatório.

Desenvolvimento do Hub: `cargo run --manifest-path host/Cargo.toml -- serve`. Avoid competing with an installed host on port 19422; isolated tests should use a temporary `MEXERICA_HOME` and another port. Stop test hosts when finished.

## Visual checks

Follow [UI conventions](../reference/ui/ui-conventions.md) for desktop behavior.

Versions are defined in `apps/project.yml` (`MARKETING_VERSION`), `host/Cargo.toml` and `apps/desktop/package.json`; keep them equal (the desktop release workflow fails when the app and host differ), then regenerate the Xcode project.

## Releases e espelhamento

Os fluxos de release e espelhamento herdados do upstream não estão autorizados como procedimentos da Mexerica. Não publicar, criar tags, espelhar o crate ou enviar notificações por inferência; distribuição depende de autorização e procedimento próprios.

## Checagens de segurança no CI

PRs que alterem `.github/workflows/`, `.github/security/`, `.gitleaks.toml` ou
`tools/ci-gitleaks*` passam por revisão de segurança antes do merge; o executor
inclui head e escopo para revisão. O Gitleaks usa executor, configuração e
baseline aprovados da main; o intervalo de commits analisado continua sendo o
do PR/push. Testes do PR executam somente depois dessa varredura. O Semgrep usa
lock completo com versões e hashes de wheels para CPython 3.12 /
Ubuntu 24.04 x86_64; mudanças no lock também precisam dessa revisão.

## Checagem de vazamento operacional

O job obrigatório `vazamento` procura dados operacionais nas
linhas adicionadas e mensagens de commit, com executor e regras da main confiável.
Não instala dependências; os testes da cópia do PR rodam sem o secret do scanner.
O workflow semanal separado verifica árvore e histórico, com notificação somente
em destino privado confirmado. Regras, marcador, secret obrigatório, exceção para
fork e inventário estão no job `vazamento` do CI.

As exceções de URL são prefixos exatos aprovados; termos pessoais
também são comparados com separadores equivalentes. Varreduras públicas geram
só contagens, arquivos sensíveis geram achado sem leitura e texto com UTF-8
inválido continua na análise (só byte NUL indica binário).


### Projeção legada das Mexeriquinhas v2

A projeção de `mexeriquinha` v2 para `avatarConfig` legado é contrato persistido: suas 6 cores, olhos, padrões e 22 conjuntos não podem mudar em silêncio. Rust e TypeScript comparam a coluna pública com essa projeção para reconhecer uma edição legada posterior. Uma mudança visual que altere a projeção exige versão nova, preservando o leitor/projetor da v2. Os testes com valores literais congelados protegem essa compatibilidade.

A desserialização do campo opcional em `BotConfig` tolera metadados de aparência inválidos/futuros como ausentes. Isso vale para leituras de configuração e snapshots internos, não autoriza gravar payload inválido: ingressos externos precisam validar o JSON bruto com `validate_patch` ou recusar avatar pelo caminho `user_only` antes da conversão em `BotConfig`. Depois dela, não há como distinguir omissão de metadado rejeitado. O tipo `MexeriquinhaConfig` permanece estrito.

O matcher v2 não é um validador JSON Schema geral: só interpreta const, enums e objetos fechados. Os testes Rust/TS travam as palavras-chave e os discriminantes das variantes; ampliação de schema exige primeiro implementar e testar a nova restrição nos dois leitores. Versão numérica `2.0` é canonicalizada para `2`, de acordo com o número JSON recebido pelo JavaScript.

A migração guarda a coluna original textual e as chaves originais de aparência de `config` em `kv` (`mexeriquinhas.legacy.original.<id>`), antes de projetar a coluna pública. A cópia é atômica e não sobrescrita, permanece fora do fio e sobrevive a saves de perfil antigos. O marcador global evita reconverter bots ou fabricar originais retrospectivos; bots legados criados após ele recebem equivalência visual no app até uma nova escolha do usuário.

### Memória

O perfil derivado e o envelhecimento vivem em `host/crates/memory/src/profile.rs`
e `lifecycle.rs`. A migração 5 do fork acrescenta cache e metadados sem alterar
fatos existentes. Invalidações do cache são gatilhos da própria transação.
O Host injeta o perfil em `agent/bot/turn.rs` e usa o dreamer existente para conferir
idade uma vez por hora, sem modelo. Testes usam SQLite em memória e identidades
temporárias; não use o banco instalado do dono. O gate integral do Host inclui
as provas de cache, origem/natureza, segredos no legado, teto Unicode,
reforço/replay, arquivo, rollback e restauração. A ideia vem do Mercury Agent;
não houve cópia ou adaptação de código de terceiros.
