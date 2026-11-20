# CI (ci-ok)

O workflow [CI](../../.github/workflows/ci.yml) roda em todo pull request e publica o
check **`ci-ok`**, que a proteção da `main` vai exigir. Nenhum job roda em runner
hospedado pelo GitHub: todos usam runners da **Instabox**, conectada ao repositório por app
do GitHub.

## Máquinas por rodada

| Job | Máquina | Quando liga |
|---|---|---|
| `leves` | `instabox-small` | sempre (menos fork) |
| `vazamento` | `instabox-small` | sempre (menos fork) |
| `hub` | `instabox-large` | Hub selecionado, PR pronto e com base `main` |
| `desktop` | `instabox-medium` | Desktop selecionado, PR pronto e com base `main` |
| `ci-ok` | `instabox-small` | sempre |

Uma rodada liga de 3 a 5 máquinas. Job que não foi selecionado fica "skipped" e não liga
máquina. Um push novo no mesmo PR cancela a rodada anterior inteira, inclusive os jobs que já
estão rodando. "Re-run failed jobs" repete só o que falhou e o que depende dele.

## O que cada job faz

- **leves:** numa máquina só, primeiro o que vem da `main` e não executa código do PR, depois
  o código do PR:
  1. **seleção:** compara a `main` com o merge do PR e decide se Hub e Desktop precisam rodar.
     Mudança em `.github/` ou `tools/ci-*` roda tudo; configuração do Cargo, do rustfmt ou do
     Clippy na raiz roda o Hub. O Desktop só é dispensado quando o PR muda apenas Hub, motor ou
     `apps/screen-linux`. Se o `git diff` falhar, o job reprova em vez de publicar seleção
     vazia. Rascunho e PR empilhado (base diferente da `main`) não ligam Hub nem Desktop.
  2. **segredos:** Gitleaks 8.30.1 (binário com SHA-256 fixo) nos commits do PR, com a
     configuração `.gitleaks.toml` da `main` e o executor `tools/ci-gitleaks.mjs` da `main`.
     Só localização e regra são impressas, nunca o valor.
  3. **Semgrep:** Semgrep OSS 1.184.0 instalado com os hashes de
     `.github/security/semgrep-requirements.txt` e as regras de `.github/security/semgrep.yml`,
     os dois lidos da `main`. Os achados são só aviso, mas a análise precisa concluir.
  4. **ferramentas de CI:** testa os executores de segredos e vazamento e o contrato do
     workflow (`tools/ci-workflow.test.mjs`), sem secrets. Daqui em diante roda código do PR.
  5. **auditoria:** `npm audit` e `cargo audit` só como aviso (`continue-on-error` no passo).
- **vazamento:** sozinho na máquina, porque é o único job com secret. `tools/ci-vazamento.mjs`
  confere linhas adicionadas, nomes de arquivo e mensagens de commit contra regras públicas
  (caminho pessoal, IP privado, host interno, identificador de serviço, referência de cofre,
  arquivo sensível) e contra termos pessoais vindos do secret `VAZAMENTO_TERMOS`. Olha o
  resultado final e também cada commit do PR; num merge, olha o que a resolução trouxe além do
  merge automático. Merge com mais de dois pais é recusado. O secret só chega ao passo cujo
  executor veio da `main`. Sem o secret, o job falha fechado para qualquer autor; PRs do
  Dependabot leem o secret de Dependabot de mesmo nome.
- **hub:** typecheck e testes do motor, `cargo fmt`, Clippy com `-D warnings`, a suíte
  inteira com cargo-nextest 0.9.152 (SHA-256 fixo), doctests e a prova de dois UIDs do Motor
  Mexerica. `MEXERICA_EXIGIR_BWRAP=1` faz o job reprovar se o isolamento real não puder rodar.
- **desktop:** garante `/usr/bin/ssh` (os testes de túnel o chamam de verdade), `npm ci`,
  typecheck, `npm test` (inclui o gate de textos e os testes de avisos) e `npm run test:open`
  quando a variante Open existe na árvore.

## Regras do ci-ok

- Roda sempre (`if: always()`), sem checkout e sem permissões.
- Exige `leves` e `vazamento` com sucesso.
- Hub e Desktop: se o `changes` selecionou, o job precisa ter sucesso. Se não selecionou,
  o job precisa ter sido pulado. Seleção ausente ou inválida reprova.
- Em rascunho, Hub e Desktop não rodam. Se algum deles foi selecionado, o `ci-ok` reprova
  até o PR ser marcado como pronto. Virar rascunho não dispara rodada.
- **PR empilhado** (base diferente da `main`): rodam só `leves` e `vazamento`, e o `ci-ok`
  reprova com o aviso "PR empilhado … Não vale para merge". Quando a base passar a ser a
  `main`, é preciso uma rodada nova (push, ou fechar e reabrir o PR) para o teste completo.
- PR de fork é recusado: nenhum código de fora roda nos provedores.

## Segurança do workflow

- Só o evento `pull_request`. Não há `pull_request_target`, `push`, `schedule` nem
  `workflow_dispatch`.
- `permissions: {}` no topo e permissões explícitas por job, só leitura.
- Actions fixadas por SHA completo. Todo checkout usa `persist-credentials: false`.
- Concorrência por PR com cancelamento da rodada anterior. Todo job tem `timeout-minutes`.
- `tools/ci-workflow.test.mjs` reprova rótulo de runner fora da lista, action sem SHA,
  job sem permissões ou timeout, secret fora do job `vazamento` e gatilho proibido.
- O fornecedor da VM (Instabox) pode ler o código privado e, no job `vazamento`, o
  secret em memória. Isolamento e exclusão da VM são declarações do fornecedor.
- No PR que introduz `tools/ci-gitleaks.mjs` e `tools/ci-vazamento.mjs`, a `main` ainda não
  tem os executores. Só nesse caso, e só em `pull_request`, eles vêm do próprio PR, e o
  passo de vazamento roda só com as regras públicas, sem o secret. Esse PR depende de
  revisão humana dos próprios executores.

## Limites conhecidos

- O evento `pull_request` usa o workflow do próprio PR. Quem pode criar branch no
  repositório pode mudar o workflow, o `ci-ok` e o uso do secret. O `ci-ok` só prova algo
  quando quem escreve no repositório é confiável; mudanças em `.github/` e `tools/ci-*`
  pedem revisão cuidadosa. Fechar essa porta exige um workflow obrigatório definido fora do
  PR (regra de organização que aponta para um arquivo fixo), não este arquivo.
- Gitleaks e Semgrep mostram caminhos de arquivo nos logs. Em repositório público os logs
  são públicos; o `vazamento` já esconde caminhos com dado pessoal.
- Os jobs rodam código do PR numa VM descartável com `sudo`, como nos runners do GitHub; a
  prova de dois UIDs precisa de root. O isolamento entre jobs é do fornecedor.
- No `leves`, o código do PR roda depois dos verificadores da `main`, na mesma VM. Ele não
  muda um resultado que já saiu, mas pode reprovar o job; o `vazamento`, que tem o secret,
  continua numa VM só dele.
- Downloads diretos (Gitleaks, cargo-audit, cargo-nextest, wheels do Semgrep) são
  conferidos por hash. Node, Rust e pacotes do sistema vêm da imagem ou dos instaladores.
- O `vazamento` cobre os commits de cada PR, mas não o histórico que já está na `main`.
  Antes de abrir o repositório, faça uma varredura própria do histórico inteiro.

## Repositório público

Runner próprio em repositório público executa código de quem abre o PR. O workflow recusa
forks, mas o workflow que roda é o do fork. Antes de abrir o repositório, revise: aprovação obrigatória de workflows de
contribuidores externos em Settings → Actions, o comportamento da Instabox com repositórios
públicos e o custo das máquinas que o `ci-ok` de forks liga.

## Rodar localmente

Os comandos principais dos jobs rodam numa máquina Linux (Ubuntu 24.04) com Node 24, o Rust
do `rust-toolchain.toml`, `ssh`, `bubblewrap` e cargo-nextest 0.9.152 instalados:

```sh
node --test tools/ci-gitleaks.test.mjs tools/ci-vazamento.test.mjs tools/ci-workflow.test.mjs
(cd apps/desktop && npm ci && npm run typecheck && npm test && npm run test:open --if-present)
(cd motor && npm ci && npm run typecheck && npm test)
(cd host && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && MEXERICA_MOTOR_TESTS_REQUIRED=1 MEXERICA_EXIGIR_BWRAP=1 cargo nextest run --workspace --locked --no-fail-fast)
```

O `motor` precisa do `npm ci` antes da suíte do Hub. A prova de dois UIDs e o fallback com
`unshare` estão no job `hub` do workflow. Para os testes de `tools/ci-gitleaks.test.mjs`,
aponte `GITLEAKS_BIN` para o Gitleaks 8.30.1.
