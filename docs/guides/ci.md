# CI (ci-ok)

O workflow [CI](../../.github/workflows/ci.yml) roda em todo pull request e publica o
check **`ci-ok`**. Quem faz o merge confere o selo; o GitHub não o impõe, porque um PR pode
redefinir o próprio workflow (veja Limites conhecidos). Enquanto o repositório for privado,
nenhum job roda em runner hospedado pelo GitHub: os jobs do CI usam runners da **Instabox**, e o
workflow [macOS](../../.github/workflows/macos.yml) usa máquinas macOS da **Namespace**.
Os dois fornecedores se conectam ao repositório por app do GitHub.

## Trocar a máquina sem editar o workflow

A máquina de cada classe de job vem de uma variável do repositório (Settings → Secrets and
variables → Actions → Variables). Sem a variável, vale o padrão da Instabox:

| Variável | Jobs | Padrão | Valores aceitos |
|---|---|---|---|
| `CI_MAQUINA_LEVE` | leves, vazamento, ci-ok | `instabox-small` | `instabox-small`; com o repositório público, `ubuntu-24.04` |
| `CI_MAQUINA_MEDIA` | desktop | `instabox-medium` | `instabox-medium`; com o repositório público, `ubuntu-24.04` |
| `CI_MAQUINA_PESADA` | hub | `instabox-large` | `instabox-large`; com o repositório público, `ubuntu-24.04` |

- Use `ubuntu-24.04`, nunca `ubuntu-latest`: o Semgrep e o Hub conferem Ubuntu 24.04 x64, e o
  `latest` muda de versão sozinho.
- O primeiro passo de todo job recusa máquina do GitHub enquanto o repositório for privado,
  mesmo que a variável aponte para ela.
- A variável só escolhe a máquina; as checagens e o `ci-ok` não mudam.
- Trocar as variáveis muda onde o teste roda, e só isso. **Não é o procedimento para abrir o
  repositório.** Um PR pode pedir outros rótulos no próprio workflow, então antes de abrir é
  preciso, fora do workflow, tirar este repositório do alcance dos runners de terceiros e
  exigir aprovação para rodar workflows de quem é de fora.

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

## macOS (Namespace)

O workflow `macOS` tem um único job, `macos`, que roda em `nscloud-macos-tahoe-arm64-6x14`
(macOS Tahoe, Apple Silicon, 6 vCPU e 14 GB). Ele ainda **não entra no `ci-ok`**: aparece
no PR como um check à parte e não trava a fila enquanto estabiliza.

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

- O CI só tem o evento `pull_request`. Não há `pull_request_target`, `push`, `schedule`
  nem `workflow_dispatch`. O workflow macOS acrescenta só o disparo manual, sem entradas.
- `permissions: {}` no topo e permissões explícitas por job, só leitura.
- Actions fixadas por SHA completo. Todo checkout usa `persist-credentials: false`.
- Concorrência por PR com cancelamento da rodada anterior. Todo job tem `timeout-minutes`.
- `tools/ci-workflow.test.mjs` reprova rótulo de runner fora da lista, action sem SHA,
  job sem permissões ou timeout, secret fora do job `vazamento` e gatilho proibido.
- O fornecedor da VM (Instabox ou Namespace) pode ler o código privado e, no job `vazamento`, o
  secret em memória. Isolamento e exclusão da VM são declarações do fornecedor.
- No PR que introduz `tools/ci-gitleaks.mjs` e `tools/ci-vazamento.mjs`, a `main` ainda não
  tem os executores. Só nesse caso, e só em `pull_request`, eles vêm do próprio PR, e o
  passo de vazamento roda só com as regras públicas, sem o secret. Esse PR depende de
  revisão humana dos próprios executores.

## Limites conhecidos

- O evento `pull_request` usa o workflow do próprio PR, de fork ou não. Quem abre o PR pode
  redefinir o workflow, a recusa de fork, o `ci-ok` e o uso do secret. Por isso o `ci-ok` só
  prova algo quando quem escreve no repositório é confiável, mudanças em `.github/` e
  `tools/ci-*` pedem leitura humana, e PR de fora só deve rodar depois de aprovado por quem
  mantém o repositório. Uma checagem que o PR não consegue redefinir precisa vir de fora deste
  arquivo (workflow obrigatório da organização a partir de referência protegida, ou um app
  dedicado como fonte obrigatória do check).
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
forks, mas o workflow que roda é o do fork, que pode tirar essa recusa. Por isso, antes de
abrir o repositório: exigir aprovação para rodar workflows de todos os colaboradores de fora
(Settings → Actions) e tirar o repositório do alcance dos runners de terceiros, no painel de
cada fornecedor. Só depois troque as variáveis de máquina.

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
