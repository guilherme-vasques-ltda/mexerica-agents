# Skills e motores

Regra do dono: as skills de um bot vêm **só do harness da Mexerica** —
o catálogo do Hub, com a seleção explícita do dono para aquele bot e a versão fixa
emitida na instalação. O Hub nunca copia nem cria link para as pastas nativas dos
motores (`~/.claude/skills`, `~/.codex/skills` e afins). O catálogo e a seleção
estão em [skills e aplicativos](marketplace.md) e no
[contrato do catálogo](../reference/catalogo-skills.md).

O problema: cada motor também lê skills e regras das próprias pastas, na home e
na pasta do projeto. O que estiver lá entra no bot por fora do harness, sem
digest, sem versão e sem escolha do dono.

## Doutor de skills (só leitura)

O método `donnaSkillsDoutor` recebe `{ "botId": "…" }` e devolve:

- `harness`: o que a entrega anunciaria agora, medido pela **mesma conferência da
  entrega** (`skill_catalog::entrega::conferir`, que usa `checked_document`) e no
  mesmo snapshot do registro. `skills` é o resultado da **conferência local**
  (registro e texto): as selecionadas que passam, cada uma com a versão fixa
  (encarnação, digest do registro e digest do corpo). Estar em `skills` **não**
  prova entrega; só o `digest` não nulo confirma o conjunto.
  `problemas` lista cada skill selecionada que **não** entra, com o motivo:
  não instalada, texto alterado/removido/ilegível, ou registro sem versão fixa ou
  sem digest do corpo. `digest` (SHA-256 das `skills`) só aparece quando não há
  problema; com qualquer problema, ou se o registro não pôde ser conferido
  (`verificado: false`), vem `null` e o conjunto não está confirmado.
  `verificado: true` significa só que o registro foi conferido, não que houve entrega.
  `selecaoVersao` é a versão opaca da seleção gravada. O doutor não publica nada
  para o motor separado; só confere. Por isso, **com motor em usuário separado**
  (`motor_exec`) a entrega depende de uma etapa que ele não prova (a publicação
  da skill para o motor pode falhar por permissão ou pelo limite de 100 MiB, e
  então o perfil real omite a skill): o `digest` vem `null` e `problemas` ganha
  `{"id":"*"}` com o motivo. `skills` segue listando o que passa na conferência
  local (registro e texto).
- `nativas`: três estados, do mais ao menos afirmado.
  - `nativa_vazando`: a fonte nomeada de skills ou regras (por exemplo
    `~/.claude/skills`, `AGENTS.md`) **existe com o tipo esperado** (arquivo para
    `*.md`, pasta para as demais); a leitura efetiva depende da configuração do
    motor (por exemplo, do fallback estar ativado).
  - `nativa_possivel`: existe algo que **pode** conter regras ou skills, sem
    conteúdo confirmado, com `motivo`: pasta lida inteira por cautela
    (`~/.config/opencode`, `.opencode`, `.gemini`, que podem ter só configuração),
    link (não seguido), componente do caminho que é link, tipo incompatível
    (ex.: pasta chamada `AGENTS.md`) ou arquivo especial.
  - `nao_verificavel`: a conferência falhou (permissão, erro de leitura) ou a base
    não pôde ser aberta sem seguir links; não se sabe se existe. Sempre com `motivo`.

  `tipo`: `pasta`, `arquivo`, `symlink` ou `indeterminado`. Os três estados tiram o `limpo`.
- `limpo`: `true` só quando não há nenhuma `nativas` e nenhum item em
  `motivosNaoLimpo`. Motor fora do mapa (`motorMapeado: false`), fonte "a
  confirmar", home não verificável ou pasta fora da home entram em
  `motivosNaoLimpo`: **ausência de prova nunca vira limpo**. Como cada motor ainda
  tem fontes a confirmar (tabela abaixo), hoje `limpo` é sempre `false` e o valor
  útil é a lista `nativas`.

Caminhos: nunca absolutos. `~/…` é a home efetiva do motor, `./…` a pasta do bot
e `../…` seus ancestrais; nenhum nome de usuário sai do Hub.

Garantias: não lê conteúdo, não apaga nem muda nada e não segue symlink. Cada base
(home, pasta do bot e ancestrais, `CODEX_HOME`) é aberta a partir da raiz com
`openat` + `O_NOFOLLOW` em todo componente, e cada passo seguinte é relativo ao
diretório já aberto (`fstatat` sem seguir o último link), então trocar um nome por
link entre conferir e usar falha em vez de atravessar. Link em qualquer componente
da base, base que não é pasta comum ou erro de permissão vira `nao_verificavel`;
link dentro do mapa vira `symlink`; o alvo nunca é olhado. Fora do Unix nada é
afirmado (tudo `nao_verificavel`).

Escopo: só a home efetiva do motor e a pasta do bot. `CODEX_HOME` fora da home e
da pasta do bot, relativo ou com `..` é `nao_verificavel` e não é sondado. Pasta do
bot fora da home: só ela é sondada (os ancestrais não são da home nem do bot) e
vem o motivo. A pasta do bot é a **efetiva**, a mesma do lançamento
(`motor_exec::launch_cwd`): com motor separado, se o cadastro aponta para a home do
Hub ou a pasta de dados, o motor roda na raiz do motor, é ela que é sondada e vem o
motivo da troca. Home efetiva: a da conta isolada do bot (`HOME`/`CODEX_HOME` do
lançamento da conta) quando há; sem conta, a do Hub; com motor em usuário
separado (`motor_exec`) e sem conta isolada, a home do motor é desconhecida do
Hub e vira "home efetiva do motor não verificável".

Autoridade: a mesma da gestão de contas de IA (`engine.accounts`) — o dono no
computador do Hub ou o aparelho do dono com o vínculo vigente, reconferido antes
de responder. Device sem esse vínculo (Executor, Screen, Control sem vínculo) e
pareado são recusados; há teste pelo despacho do RPC e outro que revoga o vínculo depois da coleta e antes da resposta (a resposta é recusada). Classe `Leitura` na tabela
de métodos.

## Mapa por motor

O mapa vive em `host/src/fork/skills_doutor.rs`; mude os dois juntos. "Coberto" =
sondado pelo doutor. "A confirmar" = **não** sondado e **não** conta como coberto:
mantém `limpo` em `false`.

| Motor | Home (coberto) | Projeto: pasta e ancestrais (coberto) | A confirmar (não coberto) |
|---|---|---|---|
| Claude Code (`claude`) | `~/.claude/skills`, `~/.claude/CLAUDE.md`, `~/.claude/rules`, `~/.claude/commands` | `.claude/skills`, `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md`, `.claude/rules`, `.claude/commands` | Política gerenciada (`/etc/claude-code`; fora da home e do bot); skills de plugins, de `--add-dir` e aninhadas. `CLAUDE_CONFIG_DIR` troca `~/.claude`. |
| Codex (`codex`) | `$CODEX_HOME/skills`, `$CODEX_HOME/AGENTS.md`, `$CODEX_HOME/AGENTS.override.md` (padrão `~/.codex`), `~/.agents/skills` | `.agents/skills`, `.codex/skills`, `AGENTS.md`, `AGENTS.override.md` | `project_doc_fallback_filenames` do `config.toml`; `.codex/skills` e `.agents/skills` sem confirmação por versão. |
| OpenCode (`opencode`) | `~/.config/opencode` (a pasta inteira, por cautela: pode ter só configuração; vira `nativa_possivel`), `~/.claude/CLAUDE.md` (fallback), `~/.claude/skills`, `~/.agents/skills` | `.opencode` (idem), `AGENTS.md`, `CLAUDE.md` (fallback), `.claude/skills`, `.agents/skills` | Array `instructions` do `opencode.json` (arquivos, globs e URLs); pastas nativas de skills. `OPENCODE_DISABLE_CLAUDE_CODE*` desligam os fallbacks do Claude. |
| Devin (`devin`) | — | `.dsh/skills`, `.agents/skills`, `AGENTS.md` | Nenhuma fonte confirmada em doc oficial. |
| Gemini / Antigravity (`gemini`, `antigravity`, `agy`) | `~/.gemini/GEMINI.md` | `GEMINI.md`, `.gemini` (a pasta inteira, por cautela) | `GEMINI.md` just-in-time em subpastas; skills, comandos e regras dentro de `.gemini/`. |
| Qualquer outro | união de todas as linhas acima | união de todas as linhas acima | `motorMapeado: false`, nunca limpo. |

Docs oficiais consultadas (15/11/2026):
[Claude Code: memória](https://code.claude.com/docs/en/memory) e
[skills](https://code.claude.com/docs/en/skills) (`CLAUDE.md`, `.claude/CLAUDE.md`,
`CLAUDE.local.md`, `.claude/rules`, `~/.claude/skills`, `.claude/commands`,
política em `/etc/claude-code`);
[Codex: AGENTS.md](https://developers.openai.com/codex/guides/agents-md)
(`AGENTS.override.md`, `CODEX_HOME`; a página redireciona e o conteúdo novo não
foi lido, então fallbacks e skills do Codex seguem a confirmar);
[OpenCode: regras](https://dev.opencode.ai/docs/rules/) (`AGENTS.md`, fallback
`CLAUDE.md` e `~/.claude/CLAUDE.md`, `~/.claude/skills`, `.claude/skills`,
`instructions`, variáveis `OPENCODE_DISABLE_CLAUDE_CODE*`);
[Gemini CLI: GEMINI.md](https://geminicli.com/docs/cli/gemini-md/)
(`~/.gemini/GEMINI.md`, `GEMINI.md` no workspace e ancestrais).

Referência de desenho, sem código copiado: o Tucano isola o Antigravity (`agy`)
negando config, rules e skills nativas; o Codync separa perfis de login por pasta
(`docs/features/login-profiles.md` no upstream).

## Limites desta etapa e próxima etapa

Esta etapa só **diagnostica**. Fica para a etapa seguinte:

1. **Bloqueio antes da execução**: chamar o doutor no início do motor
   (`host/src/agent/bot/session.rs`, em `launch_commands`/`start_agent`) e recusar
   ou isolar quando houver `nativa_vazando`, cobrindo todos os comandos e
   adaptadores candidatos, e aplicando as flags/variáveis do mapa (depois de
   confirmadas por versão do motor).
2. **Entrega por conteúdo**: hoje o bloco do perfil e o `/id` anunciam caminhos de
   arquivo (`host/src/fork/skill_catalog/entrega.rs` e `preparo.rs`); o motor pode
   ler o caminho depois que o conteúdo mudou. A entrega deve passar só os bytes
   conferidos (o leitor `host/src/fork/bot_skills/read.rs` já faz isso para o
   preparo interno), sem caminho.
3. Linhas próprias para os demais motores do registro ACP e confirmar as fontes
   "a confirmar" por versão do motor, até `limpo` poder ser `true`.
4. Tela no app para o dono ver o diagnóstico.
