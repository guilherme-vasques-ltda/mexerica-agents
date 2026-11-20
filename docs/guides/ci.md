# CI (ci-ok)

O workflow [CI](../../.github/workflows/ci.yml) roda em todo pull request e publica o
check **`ci-ok`**, que a proteção da `main` vai exigir. Nenhum job roda em runner
hospedado pelo GitHub: todos usam runners do **Boat** e da **Instabox**, conectados ao
repositório por app do GitHub.

## Boat primeiro, Instabox no limite

1. `boat-probe` pede um start ao Boat (`boat-small`, só um `echo`, sem permissões).
   Ele tem `continue-on-error`, então uma recusa por limite não deixa a rodada vermelha.
2. `route` roda em `instabox-small` e lê os jobs da própria rodada (só `actions: read`).
   Se o passo "Set up runner" da sonda passar em até 60 s, a rodada usa o Boat. Se a sonda
   for recusada, ficar na fila ou o PR já estiver numa nova tentativa, a rodada usa a Instabox.
3. Todos os outros jobs leem os rótulos publicados pelo `route`. Em qualquer nova tentativa
   (inclusive "Re-run failed jobs", que não roda o `route` de novo), cada job escolhe a
   Instabox direto, sem reaproveitar a rota Boat da primeira tentativa:

| Classe | Boat | Instabox | Jobs |
|---|---|---|---|
| Leve | `boat-small` | `instabox-small` | secrets, vazamento, dependency-audit, semgrep, changes, ferramentas-ci, ci-ok |
| Média | `boat` | `instabox-medium` | desktop |
| Pesada | `boat-large` | `instabox-large` | hub |

Se o `route` falhar, o `ci-ok` roda em `instabox-small` e reprova. A sonda só vale para o
início da rodada: se o Boat recusar um job depois, esse job fica na fila ou falha e a saída é
"Re-run failed jobs", que já vai para a Instabox.

## O que cada job faz

- **secrets:** Gitleaks 8.30.1 (binário com SHA-256 fixo) nos commits do PR, com a
  configuração `.gitleaks.toml` da `main` e o executor `tools/ci-gitleaks.mjs` da `main`.
  Nada do PR afrouxa a política. Só localização e regra são impressas, nunca o valor.
- **vazamento:** `tools/ci-vazamento.mjs` confere linhas adicionadas, nomes de arquivo e
  mensagens de commit contra regras públicas (caminho pessoal, IP privado, host interno,
  identificador de serviço, referência de cofre, arquivo sensível) e contra termos pessoais
  vindos do secret `VAZAMENTO_TERMOS`. É o único job que recebe um secret, e só no passo cujo
  executor veio da `main`. Sem o secret, o job falha fechado.
- **changes:** compara a `main` com o merge do PR e decide se Hub e Desktop precisam rodar.
  Mudança em `.github/` ou `tools/ci-*` roda tudo; configuração do Cargo, do rustfmt ou do
  Clippy na raiz roda o Hub. O Desktop só é dispensado quando o PR muda apenas Hub, motor ou
  `apps/screen-linux`. Se o `git diff` falhar, o job reprova em vez de publicar seleção vazia.
- **ferramentas-ci:** testa os executores de segredos e vazamento e o contrato do workflow
  (`tools/ci-workflow.test.mjs`) numa VM sem secrets.
- **hub:** typecheck e testes do motor, `cargo fmt`, Clippy com `-D warnings`, a suíte
  inteira com cargo-nextest 0.9.152 (SHA-256 fixo), doctests e a prova de dois UIDs do Motor
  Mexerica. `MEXERICA_EXIGIR_BWRAP=1` faz o job reprovar se o isolamento real não puder rodar.
- **desktop:** garante `/usr/bin/ssh` (os testes de túnel o chamam de verdade), `npm ci`, typecheck, `npm test` (inclui o gate de textos e os testes de
  avisos) e `npm run test:open` quando a variante Open existe na árvore.
- **semgrep:** Semgrep OSS 1.184.0 instalado com hashes fixos e as regras de
  `.github/security/semgrep.yml`. Os achados são só aviso, mas a análise precisa concluir.
- **dependency-audit:** `npm audit` e `cargo audit` só como aviso. Não entra no `ci-ok`.

## Regras do ci-ok

- Roda sempre (`if: always()`), sem checkout e sem permissões.
- Exige `route`, `secrets`, `vazamento`, `semgrep`, `changes` e `ferramentas-ci` com sucesso.
- Hub e Desktop: se o `changes` selecionou, o job precisa ter sucesso. Se não selecionou,
  o job precisa ter sido pulado. Seleção ausente ou inválida reprova.
- Em rascunho, Hub e Desktop não rodam. Se algum deles foi selecionado, o `ci-ok` reprova
  até o PR ser marcado como pronto.
- PR de fork é recusado: nenhum código de fora roda nos provedores.

## Segurança do workflow

- Só o evento `pull_request`. Não há `pull_request_target`, `push`, `schedule` nem
  `workflow_dispatch`.
- `permissions: {}` no topo e permissões explícitas por job, só leitura.
- Actions fixadas por SHA completo. Todo checkout usa `persist-credentials: false`.
- Concorrência por PR com cancelamento da rodada anterior. Todo job tem `timeout-minutes`.
- `tools/ci-workflow.test.mjs` reprova rótulo de runner fora da lista, action sem SHA,
  job sem permissões ou timeout, secret fora do job `vazamento` e gatilho proibido.
- O fornecedor da VM (Boat ou Instabox) pode ler o código privado e, no job `vazamento`, o
  secret em memória. Isolamento e exclusão da VM são declarações do fornecedor.
- No PR que introduz `tools/ci-gitleaks.mjs` e `tools/ci-vazamento.mjs`, a `main` ainda não
  tem os executores. Só nesse caso, e só em `pull_request`, eles vêm do próprio PR, e o
  passo de vazamento roda só com as regras públicas, sem o secret. Esse PR depende de
  revisão humana dos próprios executores.

## Limites conhecidos

- O evento `pull_request` usa o workflow do próprio PR. Quem pode criar branch no
  repositório pode mudar o workflow, o `ci-ok` e o uso do secret. O `ci-ok` só prova algo
  quando quem escreve no repositório é confiável; mudanças em `.github/` e `tools/ci-*`
  pedem revisão cuidadosa.
- Os jobs rodam código do PR numa VM descartável com `sudo`, como nos runners do GitHub; a
  prova de dois UIDs precisa de root. O isolamento entre jobs é do fornecedor.
- Downloads diretos (Gitleaks, cargo-audit, cargo-nextest, wheels do Semgrep) são
  conferidos por hash. Node, Rust e pacotes do sistema vêm da imagem ou dos instaladores.
- O `vazamento` olha o resultado final do PR e as mensagens de commit, não cada commit
  intermediário. Antes de abrir o repositório, faça uma varredura própria do histórico.

## Repositório público

Runner próprio em repositório público executa código de quem abre o PR. O workflow recusa
forks, mas o workflow que roda é o do fork. Antes de abrir o repositório, revise: aprovação obrigatória de workflows de
contribuidores externos em Settings → Actions, o comportamento do Boat com repositórios
públicos (bloqueado por padrão) e o custo dos starts do `ci-ok` de forks.

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
