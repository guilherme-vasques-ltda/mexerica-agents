# CI (ci-ok)

O workflow [CI](../../.github/workflows/ci.yml) roda em todo pull request e publica o
check **`ci-ok`**, que a proteção da `main` vai exigir. Nenhum job roda em runner
hospedado pelo GitHub: os jobs do CI usam runners do **Boat** e da **Instabox**, e o
workflow [macOS](../../.github/workflows/macos.yml) usa máquinas macOS da **Namespace**.
Os três fornecedores se conectam ao repositório por app do GitHub.

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
início da rodada. Se o Boat recusar um job depois:

- job que **falhou**: "Re-run failed jobs", que já vai para a Instabox;
- job **preso na fila** (o GitHub espera até 24 h por runner próprio): cancelar a rodada e
  usar "Re-run all jobs". "Re-run failed jobs" não pega job que não falhou.

## O que cada job faz

- **secrets:** Gitleaks 8.30.1 (binário com SHA-256 fixo) nos commits do PR, com a
  configuração `.gitleaks.toml` da `main` e o executor `tools/ci-gitleaks.mjs` da `main`.
  Depois da instalação, nada do PR afrouxa a política. Só localização e regra são
  impressas, nunca o valor.
- **vazamento:** `tools/ci-vazamento.mjs` confere linhas adicionadas, nomes de arquivo e
  mensagens de commit contra regras públicas (caminho pessoal, IP privado, host interno,
  identificador de serviço, referência de cofre, arquivo sensível) e contra termos pessoais
  vindos do secret `VAZAMENTO_TERMOS`. Olha o resultado final e também cada commit do PR,
  porque um commit intermediário fica no histórico; num merge, olha o que a resolução trouxe
  além do merge automático. Merge com mais de dois pais é recusado. É o único job que recebe um secret, e só
  no passo cujo executor veio da `main`. Sem o secret, o job falha fechado para qualquer
  autor; PRs do Dependabot leem o secret de Dependabot de mesmo nome.
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
- **semgrep:** Semgrep OSS 1.184.0 instalado com os hashes de
  `.github/security/semgrep-requirements.txt` e as regras de `.github/security/semgrep.yml`,
  os dois lidos da `main` (só no PR que os introduz vêm do próprio PR). Os achados são só
  aviso, mas a análise precisa concluir.
- **dependency-audit:** `npm audit` e `cargo audit` só como aviso. Não entra no `ci-ok`.

## macOS

O workflow `macOS` tem um único job, `macos`. Ele ainda **não entra no `ci-ok`**: aparece
no PR como a sonda do Boat e não trava a fila enquanto estabiliza.

- **Máquina:** por padrão, `nscloud-macos-tahoe-arm64-6x14` (Namespace: macOS Tahoe, Apple
  Silicon, 6 vCPU e 14 GB). O job só vai para a máquina macOS do GitHub (`macos-15`, Apple
  Silicon) quando as duas condições valem ao mesmo tempo: o evento confirma que o repositório é
  público e a variável `CI_MAQUINA_MAC` vale `macos-15` (Settings → Secrets and variables →
  Actions → Variables; uma variável de mesmo nome na organização também conta). Qualquer outro
  caso, inclusive variável vazia ou com outro valor, fica na Namespace. A escolha acontece antes
  de alocar a máquina. Como segunda barreira, o primeiro passo reprova se o job cair numa
  máquina do GitHub sem o evento confirmar repositório público.

- **Quando roda:** em PR pronto (não rascunho, não fork) que muda `apps/desktop/`, `host/`,
  `motor/`, `packaging/`, a toolchain ou o Cargo da raiz, ou o próprio workflow; e por
  disparo manual (Actions → macOS → Run workflow). O disparo manual só existe depois que o
  workflow está na `main`.
- **O que faz:** os testes nativos do Codex da Open no Hub (`fork::open_codex` e
  `open_hub` com `CODYNC_ENV=open`), `npm ci`, `npm run test:open` e
  `npm run dist:mac:open`. Depois confere a assinatura ad hoc do pacote e que o Hub e o
  helper de fala embutidos são `arm64`.
- **Artefato:** só no disparo manual, gera `Mexerica-Open-macos-arm64.zip` e
  `SHA256SUMS` e os guarda como artefato do próprio Actions por 3 dias. Não há release,
  tag nem envio para fora do GitHub.
- **Gasto:** filtro de caminhos, cancelamento da rodada antiga do mesmo PR, limite de 75
  minutos e cache do npm e do Cargo pelo cache do GitHub (escopo por branch: o cache de um PR
  não chega à `main` nem a outro PR). Não usa volume de cache da Namespace, que é
  compartilhado por rótulo entre todas as rodadas. O resumo do job mostra os minutos gastos.
- **Segurança:** `permissions: {}` no topo e só `contents: read` no job; nenhum secret;
  actions de uma lista fechada por SHA (checkout, setup-node, rust-cache e upload-artifact).
  A mesma lista está em Settings → Actions do repositório, que recusa qualquer outra action:
  `upload-artifact` entrou nela para guardar o zip da Open no disparo manual. Uma action nova
  precisa entrar nos dois lugares, senão o workflow nem começa ("startup failure");
  o rustup, se faltar na imagem, é baixado com SHA-256 fixo. `tools/ci-workflow.test.mjs`
  confere tudo isso.

Para entrar no `ci-ok`, falta: rodadas estáveis em PRs reais, um tempo de rodada aceitável
e mover o job (ou o check dele) para a lista do selo, num PR à parte.

## Regras do ci-ok

- Roda sempre (`if: always()`), sem checkout e sem permissões.
- Exige `route`, `secrets`, `vazamento`, `semgrep`, `changes` e `ferramentas-ci` com sucesso.
- Hub e Desktop: se o `changes` selecionou, o job precisa ter sucesso. Se não selecionou,
  o job precisa ter sido pulado. Seleção ausente ou inválida reprova.
- Em rascunho, Hub e Desktop não rodam. Se algum deles foi selecionado, o `ci-ok` reprova
  até o PR ser marcado como pronto.
- PR de fork é recusado: nenhum código de fora roda nos provedores.

## Segurança do workflow

- O CI só tem o evento `pull_request`. Não há `pull_request_target`, `push`, `schedule`
  nem `workflow_dispatch`. O workflow macOS acrescenta só o disparo manual, sem entradas.
- `permissions: {}` no topo e permissões explícitas por job, só leitura.
- Actions fixadas por SHA completo. Todo checkout usa `persist-credentials: false`.
- Concorrência por PR com cancelamento da rodada anterior. Todo job tem `timeout-minutes`.
- `tools/ci-workflow.test.mjs` reprova rótulo de runner fora da lista, action sem SHA,
  job sem permissões ou timeout, secret fora do job `vazamento` e gatilho proibido.
- O fornecedor da VM (Boat, Instabox ou Namespace) pode ler o código privado e, no job
  `vazamento`, o secret em memória. Isolamento e exclusão da VM são declarações do fornecedor.
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
- Downloads diretos (Gitleaks, cargo-audit, cargo-nextest, wheels do Semgrep) são
  conferidos por hash. Node, Rust e pacotes do sistema vêm da imagem ou dos instaladores.
- O `vazamento` cobre os commits de cada PR, mas não o histórico que já está na `main`.
  Antes de abrir o repositório, faça uma varredura própria do histórico inteiro.

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
