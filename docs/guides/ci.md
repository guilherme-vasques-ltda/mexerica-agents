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
     Mudança em `.github/workflows/ci.yml` roda tudo, porque ela define os jobs pesados;
     configuração do Cargo, do rustfmt ou do Clippy na raiz roda o Hub. O Desktop é dispensado
     em dois casos: quando o PR muda apenas Hub, motor ou `apps/screen-linux`, e quando muda
     apenas `.github/` fora do `ci.yml` (como o workflow do macOS), `tools/ci-*` ou este guia.
     Nesse segundo caso o Hub também é dispensado: roda só o `leves` e o `vazamento`, e o
     `ci-ok` aceita os pulados. Os testes do Desktop leem outros documentos e arquivos de
     `tools/`, então mudar um deles continua ligando o Desktop. Se o `git diff` falhar, o job reprova em vez de publicar seleção
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

## Revisão automática opcional

O [Open Code Review](../../.github/workflows/code-review.yml) roda fora do `ci-ok` em PRs
internos e sem rascunho para `main`, na abertura, reabertura ou atualização do código. Se a
configuração estiver ausente, o job pula a revisão. Ele precisa dos Secrets `OCR_LLM_URL`,
`OCR_LLM_AUTH_TOKEN` e `OCR_LLM_MODEL`. O nome do modelo não é lido de uma Variable.
As Variables opcionais são
`OCR_LLM_PROTOCOL` (`openai`, `openai-responses` ou `anthropic`, padrão `openai`),
`OCR_EFFORT` (`low`, `medium` ou `high`, padrão `medium`) e `OCR_LANGUAGE` (`Portuguese` ou
`English`, padrão `Portuguese`) e `OCR_MAX_TOKENS_BUDGET` (inteiro positivo até 2.000.000,
padrão 500.000).

**Ao ativar, o provedor configurado recebe o diff e pode receber trechos adicionais do
código para contexto.** Use um endpoint HTTPS confiável que não redirecione requisições
e imponha seu próprio limite de gasto: o orçamento do revisor é avaliado entre chamadas.
Mantenedores com permissão de push são parte da fronteira de
confiança, pois `pull_request` pode usar o workflow modificado numa branch interna.
O revisor usa um repositório isolado, separado do checkout confiável, e lê o head apenas
por objetos Git. Nenhuma saída bruta é publicada ou enviada como artifact. Só achados
filtrados passam num artifact da própria execução, retido por um dia. O publicador não
recebe segredos do provedor e reconfere o formato, os SHAs, o diff e as linhas. Os
comentários incluem categoria, severidade e explicação em texto simples, sem links,
menções ou sugestões de código. O filtro é conservador e descarta textos que não pode
publicar com segurança. Há limite de 20 achados e 1.500 caracteres por comentário. Uma
revisão parcial só pode publicar se a CLI confirmar que a única causa foi o orçamento;
nesse caso o resumo avisa. O revisor não aprova o PR.

## macOS

O workflow `macOS` tem o job de build, `macos`, e um job de recusa, `maquina-mac`. Eles
ainda **não entram no `ci-ok`**: aparecem no PR como um check à parte e não travam a fila
enquanto estabilizam.

- **Máquina:** a escolha acontece antes de alocar a máquina e depende da visibilidade que o
  evento informa.
  - **Repositório privado:** sempre `nscloud-macos-tahoe-arm64-6x14` (Namespace: macOS Tahoe,
    Apple Silicon, 6 vCPU e 14 GB). A variável `CI_MAQUINA_MAC` não muda nada.
  - **Repositório público:** só a máquina macOS do GitHub (`macos-15`, Apple Silicon), e só se
    a variável `CI_MAQUINA_MAC` vale `macos-15` (Settings → Secrets and variables → Actions →
    Variables; uma variável de mesmo nome na organização também conta). Vazia ou com outro
    valor, o `macos` não roda e o `maquina-mac` falha com a explicação, sem build e sem
    máquina paga.
  - Duas barreiras no início do `macos`: reprova máquina do GitHub sem o evento confirmar
    repositório público, e reprova máquina que não é do GitHub com o repositório público.
- **Quando roda:** em PR pronto (não rascunho, não fork) que muda `apps/desktop/`, `host/`,
  `motor/`, `packaging/`, a toolchain ou o Cargo da raiz, ou o próprio workflow; e por
  disparo manual (Actions → macOS → Run workflow) **a partir da `main`**. Disparo escolhendo
  outro branch deixa os jobs pulados. Exceção: o Actions compara texto sem diferenciar
  maiúsculas, então um branch como `MAIN` passa na condição e liga a máquina; o primeiro passo
  compara a ref exata e reprova antes do checkout e do build. Isso vale para o workflow da
  `main`: o disparo usa o YAML do branch escolhido.
- **O que faz:** os testes nativos do Codex da Open no Hub (`fork::open_codex` e
  `open_hub` com `CODYNC_ENV=open`), `npm ci`, `npm run test:open` e
  `npm run dist:mac:open`. Depois confere a assinatura ad hoc do pacote e que o Hub e o
  helper de fala embutidos são `arm64`.
- **Artefato:** só no disparo manual da `main`, gera
  `Mexerica-Open-macos-arm64-teste-ci-<commit>.zip` e `SHA256SUMS` e os guarda como artefato
  `mexerica-open-macos-arm64-teste-ci-<commit>` do próprio Actions por 3 dias. É um **teste de
  CI, não uma versão oficial**: a assinatura ad hoc e o hash só provam a integridade daquela
  rodada, não autoria nem aprovação. O resumo do job diz isso e mostra commit e ref. Não
  compartilhar nem instalar sobre a versão de uso. Não há release, tag nem envio para fora do
  GitHub.
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

## Release (rascunho manual)

O workflow [Release](../../.github/workflows/release.yml) monta os instaladores das três
plataformas e deixa tudo pronto, mas **nunca publica sozinho**: no modo `release` ele cria
apenas um **rascunho** no GitHub Releases (`--draft --prerelease`), sem tag e sem
publicação; virar release de verdade é sempre um passo manual de quem mantém o repositório.
Ele fica fora do `ci-ok`, como o workflow macOS: aparece só nas Actions.

- **Quando roda:** só por disparo manual (Actions → Release → Run workflow) **a partir da
  `main`**, e só com o repositório público. Cada job repete a barreira antes do checkout:
  runner do GitHub, repositório público e ref exatamente `main` (comparação exata: um branch
  `MAIN` não passa, mesmo passando na condição do job).
- **Modos:** `release` exige a versão no formato `v0.0.1` (sufixo opcional como `-rc.1`),
  recusa se a tag ou uma release publicada dessa versão já existem, apaga rascunhos antigos
  da mesma versão e recria o rascunho limpo com as notas e os arquivos. `preview` não usa a
  versão: nomeia os arquivos com `preview-<commit>` e os deixa só nos artefatos do Actions,
  com o aviso de que não é versão oficial.
- **Máquinas:** só runners grátis do GitHub (`macos-15`, `ubuntu-24.04`, `windows-latest`),
  com timeouts de 75/45/75/15 minutos e artefatos guardados por 7 dias.
- **Versão:** no modo `release`, `tools/ci-release-version.mjs` grava o semver (sem o `v`) no
  `apps/desktop/package.json` da VM descartável antes de empacotar, então `app.getVersion()`,
  o `Info.plist` do macOS, o `.deb`, o `.AppImage` e o NSIS reportam exatamente a versão da
  rodada — e cada job confere o que empacotou: `CFBundleShortVersionString` no macOS, campo
  `Version` do `.deb` no Linux e o `package.json` de dentro do `app.asar` no Windows. No modo
  `preview`, `preview-<commit>` não é versão válida para `.deb` nem NSIS: os metadados internos
  mantêm a versão base do `package.json` e `preview-<commit>` vai só no nome dos arquivos e no
  resumo. Os binários do Hub continuam com a versão do `host/Cargo.toml`, preservando builds
  `--locked`.
- **Perfis:** apenas o pacote de macOS usa o perfil isolado `Mexerica Open`
  (`~/.mexerica-open`, porta `19524`). Os pacotes de Linux e Windows usam o perfil padrão do
  app (`Donnabot` / `.mexerica`, porta `19422`) e não são isolados de uma instalação
  existente — as notas de lançamento avisam isso e trazem, para cada plataforma, o comando de
  hash que confere só o arquivo baixado.
- **O que sai:** `Mexerica-Open-<versão>-macos-arm64.zip`,
  `mexerica-open-<versão>-linux-amd64.deb`,
  `mexerica-open-<versão>-linux-x86_64.AppImage`,
  `mexerica-open-<versão>-windows-x64-setup.exe`,
  `mexerica-open-<versão>-windows-x64-portable.zip` e o `SHA256SUMS.txt`. Cada job confere o
  que empacotou (assinatura e `arm64` no macOS, conteúdo do `.deb` e ELF do `.AppImage` no
  Linux, binários nativos e `--version` do Hub no Windows) e gera o próprio hash; o job
  final baixa os três artefatos, reconfere os hashes e monta o `SHA256SUMS.txt`.
- **Notas:** `tools/ci-release-notes.mjs` pede o resumo automático à API do GitHub, passa
  pela mesma trava de bastidor do `ci-bastidor.mjs` (linha com achado sai sem aparecer em
  log) e escreve em português o texto com o aviso honesto de pré-lançamento sem assinatura;
  o texto final é re-verificado antes de sair e qualquer achado reprova a rodada.
- **Segurança:** `permissions: {}` no topo; os três jobs de build só têm `contents: read`;
  o `draft-release` é o único com `contents: write` (criar o rascunho) e `actions: read`
  (baixar artefatos). Nenhum secret: só o `github.token` do próprio job. Actions da mesma
  lista fechada por SHA, checkout sem credencial persistida e downloads de rustup e
  cua-driver conferidos por SHA-256. `tools/ci-workflow.test.mjs` cobre o contrato inteiro
  e roda no job `leves` do CI.


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
  nem `workflow_dispatch`. O workflow macOS acrescenta só o disparo manual, sem entradas;
  o Release acrescenta o disparo manual com as entradas `mode` e `version`, e só roda
  a partir da `main` de um repositório público.
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
- Além da checagem de cada PR, a varredura semanal aplica os termos de `VAZAMENTO_TERMOS` à
  árvore atual, às mensagens de commit e às linhas de texto adicionadas no histórico completo da
  `main`. Ela inclui o commit inicial e compara cada merge com o primeiro pai (sem deixar de
  percorrer os outros pais). O Gitleaks também verifica o histórico. Em repositório público,
  o resumo mostra só contagens por regra. Histórico incompleto, erro de leitura ou limite de
  varredura excedido reprovam sem publicar conteúdo.
- Domínios Clerk sem protocolo, sem hífen e com sufixo fora da lista podem passar por se confundirem com acessos do SDK.

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
