# Contrato único de provedor de sandbox

Nenhum bot, rotina ou ferramenta usa este módulo ainda. Código em `host/src/fork/sandbox/`.

## Por que existe

Hoje cada lugar de execução (processo no host, contêineres e máquinas
descartáveis) teria código próprio. O contrato faz de todos "provedores" do
mesmo tipo: a escolha
do executor vira dado da tarefa, e admissão, limites e ciclo de vida são iguais
para todos. Ideia inspirada nos providers bind-mount/isolated do Sandcastle (MIT);
nenhum código foi copiado.

## Peças

| Peça | O que define |
| --- | --- |
| `Provedor` | `tipo`, `modo`, `contencao`, `verificar` (falha ruidosa, sem fallback) e `preparar` |
| `Sandbox` | `executar`, `copiar_para_dentro`, `copiar_para_fora`, `encerrar`, `estado`, `copia` |
| `Modo` | `Montagem` (cópia no host montada dentro) ou `Isolado` (entra e sai por cópia) |
| `Contencao` | `Nenhuma` < `ArquivosEAmbiente` (bwrap, Seatbelt) < `MaquinaDescartavel` |
| `Confianca` | `DoDono` (bots do próprio dono) ou `Desconhecida` (terceiros) |
| `Execucao` | argv literal sem shell, env explícito, pasta relativa à cópia, `Limites` |
| `Limites` | prazo (100 ms a 24 h), teto de saída somada (até 64 MiB), carência até SIGKILL (até 60 s) |

## Regras do contrato

- **Admissão fail-closed:** agente `Desconhecida` só roda com contenção
  `MaquinaDescartavel`; nunca no host do Hub, nem em bwrap/Seatbelt.
- **Ambiente:** nada é herdado do Hub. O provedor define `HOME` e `TMPDIR`
  privados (0700, por sandbox), `PATH` fixo de pastas absolutas e `LANG`. O pedido
  não pode definir `HOME`, `TMPDIR`, `TMP`, `TEMP`, `PATH`, `USER`, `LOGNAME`,
  `SHELL` nem `LD_*`/`DYLD_*`.
- **Prazo e watchdog:** cada execução roda em grupo de processos próprio. O líder do
  grupo nunca é recolhido antes de a sinalização acabar (observado com
  `waitid(WNOWAIT)`; o zumbi segura o PGID, então `killpg` não atinge grupo alheio).
  No prazo, o grupo recebe SIGTERM e a carência corre **inteira** (sem recolher, não há
  como saber se membros seguem vivos), depois SIGKILL no grupo. Isso vale só para o
  `ProcessoLocal`; no bwrap o prazo é SIGKILL imediato (veja abaixo). Falha de `waitid`
  diferente de ECHILD (interrupções são repetidas) não desarma o guarda: só a prova de
  que o líder foi recolhido (ECHILD) o desarma; qualquer outro erro devolve erro com
  contexto e o guarda mata o grupo. Ao fim normal, o grupo
  também é morto: filho deixado para trás não sobrevive nem trava a leitura da saída.
- **Cancelamento:** se a future de `executar` for descartada (`timeout`, `select!`), um
  guarda mata o grupo (SIGKILL) e aborta os leitores em qualquer saída. O estado fica
  `Desconhecido` durante toda a execução e só volta a `Preparado` quando ela conclui com
  sucesso.
- **Saída:** teto somado de stdout+stderr, aplicado durante a captura com um orçamento
  compartilhado pelos dois leitores (nunca se guarda mais que o teto; vale a ordem de
  chegada). O excedente é descartado e marcado `truncada`. Os dois pipes são drenados
  concorrentemente sob um único prazo, a carência (com piso de 50 ms para os leitores
  esvaziarem pipes já fechados quando a carência é 0): se um processo escapado segurar
  um pipe, a saída volta parcial e marcada `incompleta` em cerca de uma carência.
- **Ciclo de vida:** `Preparado` → `Executando` → `Preparado` (execuções em
  sequência). Falha ou cancelamento sem prova vira `Desconhecido`: não executa nem copia
  de novo, só encerra. `Encerrado` recusa tudo e só é marcado com a limpeza concluída.
- **Origens ancoradas:** cópia, home, tmp e pasta privada são conferidos no preparo
  (identidade dispositivo+inode) e reabertos sem seguir links (`O_NOFOLLOW` em todos os componentes,
  ancestral link ou gravável por terceiros recusado) no momento de montar cada comando.
  Link ou troca por outra pasta entre preparar e usar (executar **ou copiar**) vira erro (`copia_trocada`,
  `home_trocada`, `tmp_trocada`), nunca monta o alvo do link. `pasta` relativa é aberta do
  mesmo jeito dentro da cópia; link em qualquer componente é `pasta_fora_da_copia`
  (o `ProcessoLocal` entra na pasta por `fchdir` no descritor).
- **Cópias:** só fora de execução e só arquivo comum. Para dentro: pastas que
  faltam nascem 0700; pasta trocada por link é recusada; origem e destino no mesmo
  arquivo (caminho idêntico ou link, mesmo dispositivo+inode) é `copia_mesmo_arquivo`,
  sem truncar nada. As cópias são feitas relativas ao descritor da cópia já conferido
  (`openat`/`mkdirat` sem seguir links), então uma cópia trocada por outro diretório real
  é `copia_trocada`. Para fora: link, hardlink,
  FIFO e caminho com `..` ou absoluto são recusados; o destino no host é novo e
  nunca sobrescrito; o arquivo exportado precisa ser do usuário do Hub. Teto de 256 MiB
  por arquivo. Cada pasta atravessada dentro da cópia (cópias e pasta de execução) é
  conferida: dono do Hub e sem escrita para grupo ou terceiros; senão é recusada
  (fail-closed, inclusive com umask 002).
- **Encerrar:** apaga home/tmp privados e preserva a cópia; exportação e limpeza
  da cópia seguem o contrato do runner (§9). A limpeza abre a pasta
  privada sem seguir link, confere a identidade do preparo (`privada_trocada` se foi
  substituída: nada é apagado) e remove tudo relativo a descritores (`unlinkat`, filhos
  abertos com `O_NOFOLLOW`, nunca `remove_dir_all` por caminho). Pastas próprias sem
  u+rwx recebem a permissão **pelo descritor do objeto já conferido** (`fchmod`), nunca
  pelo nome: entre conferir e mudar, o nome poderia virar hardlink para um arquivo
  sensível. Pasta sem leitura (`chmod 000` do agente) só se abre com
  `O_PATH|O_NOFOLLOW|O_DIRECTORY` (Linux; diretório não aceita hardlink) e a permissão muda
  por `/proc/self/fd/N`; fora do Linux essa limpeza falha com erro. Só a ausência comprovada
  (`ENOENT`) conta como "já apagado"; erro de acesso (ex.: raiz sem permissão de busca) sobe.
  Se a limpeza falhar, o estado fica `Desconhecido` e `encerrar` pode ser repetido; o
  `Drop` de sandbox não encerrado também tenta de novo.

## Provedores

| Provedor | Modo | Contenção | Estado |
| --- | --- | --- | --- |
| `ProcessoLocal` | Montagem | Nenhuma | Referência, testado |
| `Bwrap` (Linux) | Montagem | ArquivosEAmbiente | Testes puros; teste real exige Linux com `/proc` completo |
| `Seatbelt` (macOS) | Montagem | ArquivosEAmbiente | Depois do bwrap |
| Frota de máquinas descartáveis | Isolado | MaquinaDescartavel | Planejado |

## Limites honestos

- `ProcessoLocal` não contém nada: o processo enxerga o que o usuário do Hub
  enxerga. Serve para provar o contrato e para bots do dono.
- Processo que sai do grupo (`setsid`) escapa do watchdog do `ProcessoLocal`.
  O bwrap cobre isso com PID namespace próprio. O mesmo vale para o cancelamento:
  o guarda só alcança o grupo.
- bwrap e Seatbelt contêm arquivos e ambiente, não a rede, e não são
  isolamento de kernel. Por isso só aceitam bots do dono.

## bwrap

**Requisito: bubblewrap 0.10 ou mais novo** (`--bind-fd` e `--ro-bind-fd`). Versão antiga é
`Indisponivel` com a correção "atualize o bubblewrap para 0.10 ou mais novo", detectada pela sonda.

O agente vê somente leitura: `/usr`, as pastas de biblioteca da raiz (`/bin`, `/sbin`,
`/lib`, `/lib64`, `/lib32`: link vira link, pasta vira montagem) e só estes itens de
`/etc`, cada um apenas se existir: `ssl/certs`, `pki/tls/certs`, `pki/ca-trust/extracted`,
`ca-certificates.conf`, `resolv.conf`, `hosts`, `nsswitch.conf`, `passwd`, `group`, `localtime`,
`ld.so.cache`, `ld.so.conf`, `ld.so.conf.d`, `alternatives`, `gitconfig`. Nunca `/etc`, `/etc/ssl`,
`/etc/pki` nem `/etc/ca-certificates` inteiros (podem guardar chaves privadas). `/opt` **não** é
montado por padrão (quem precisar declara como ferramenta). Além disso: a cópia em `/trabalho`,
home privada em `/home/user`, tmp privado em `/tmp` e as ferramentas. Depois de todas as
montagens, `--remount-ro /` deixa a raiz inteira somente leitura (a raiz tmpfs do bwrap, onde
nascem `/etc` e afins, é gravável por padrão); `/trabalho`, `/home/user` e `/tmp` seguem
graváveis por serem binds próprios. Não vê a home do Hub, `/var`, `/run`, `/root`, logins,
cofre nem dados do Hub. Usa user, PID, IPC e UTS namespaces próprios, `--disable-userns`,
`--cap-drop ALL`, `--new-session` e `--die-with-parent`: ao fim ou no prazo, todo processo de
dentro morre, inclusive os que usaram `setsid`. `sudo` não ganha privilégio (`NoNewPrivs`).

**Origens fixas resolvidas:** o bwrap segue links na origem, então cada origem fixa existente
(`/usr`, pastas da raiz, itens de `/etc`) é resolvida (`canonicalize`), conferida contra dados do
Hub, home do Hub e raiz das áreas privadas (também resolvidos, nos dois sentidos) e montada pelo
caminho **resolvido**. A origem resolvida e todos os seus ancestrais não podem ser do usuário do
Hub nem graváveis por terceiros sem sticky (assim um concorrente com o mesmo UID não a troca entre
a conferência e a montagem). Origem existente que não passa é erro (`montagem_expoe_dados`,
`montagem_insegura`), nunca é pulada; só a ausente é ignorada. A conferência roda em `new`
(textual), em `verificar` e a cada execução (resolvida). `MEXERICA_HOME=/usr/local/mexerica` é
recusado; sob `/opt` passa, porque `/opt` não é montado.

**Descritores:** cópia, home, tmp e cada ferramenta são abertos sem seguir links na hora de
montar o comando e entram por `--bind-fd FD DEST` (cópia, home, tmp) e `--ro-bind-fd FD DEST`
(ferramentas). Os descritores nascem com `FD_CLOEXEC` no pai (nenhum outro spawn do Hub os
herda); só no filho, em `pre_exec`, o `FD_CLOEXEC` é limpo (`fcntl` é async-signal-safe). Os
arquivos ficam vivos até o spawn.

**Ferramentas** (`ferramentas`) vêm da configuração do Hub, nunca do texto do bot, e são
montadas somente leitura no mesmo caminho. Allowlist de raízes: `/usr/…`, `/opt/…`,
`/nix/store/…`, `/snap/…` (sempre abaixo da raiz, nunca a raiz) ou, dentro da home do Hub,
com pelo menos 2 níveis (ex.: `~/.cargo/bin`). Tudo o mais é recusado: `/run`, `/var`,
`/srv`, `/mnt`, `/media`, `/root`, `/sys`, `/proc`, `/dev`, `/tmp`, `/etc` e `/home` de
outros. Também são recusados: a home ou seus ancestrais, filho direto da home (`~/.cargo`
guarda credenciais), dados do Hub, áreas privadas, destinos internos e pastas de login
(`.ssh`, `.gnupg`, `.aws`, `.config`, `.docker`, `.kube` etc.). Caminho com link em qualquer
componente é recusado na configuração (`ferramenta_com_link`) e de novo na abertura
(`ferramenta_recusada`): a pasta precisa ser do usuário do Hub ou de root e nenhum ancestral
pode ser gravável por terceiros (por exemplo, `/usr/local` com grupo gravável, como em
alguns Debian, recusa ferramentas dali: fail-closed).

**Prazo (limite):** no bwrap **não há término gracioso**. No prazo o monitor `bwrap` recebe
SIGKILL imediato e o PID namespace leva todo o resto (inclusive worker com handler de TERM); a
carência só vale no `ProcessoLocal`. Sinalizar o comando interno exigiria um PID lido de `/proc`,
que pode ser reutilizado antes do sinal, e manter um supervisor interno vivo foi descartado por
complexidade: a decisão é não sinalizar PID interno.

**Sonda:** `verificar` roda uma sonda real com as mesmas capacidades do lançamento: `--bind-fd`
e `--ro-bind-fd` sobre dois descritores distintos (o bwrap fecha cada um após usá-lo) de uma pasta privada temporária (sob a raiz dos sandboxes,
`FD_CLOEXEC` limpo só no filho), os isolamentos de namespace e `--remount-ro /`. Hub como root, bwrap
ausente, versão antiga, sobreposição ou origem insegura de montagem, ou `/proc` sem `/proc/sys`
(montagem `subset=pid`) viram `Indisponivel` com o passo de correção, sem fallback. Os testes reais
rodam onde a sonda passa; com `MEXERICA_EXIGIR_BWRAP=1`, bwrap indisponível reprova em vez de pular.
Prova real em bwrap 0.12: o caminho por descritor funciona.

## Testes de fronteira

`host/src/fork/sandbox/tests.rs`: admissão por confiança × contenção, recusa antes
de criar áreas, env reservada e caminhos fora da cópia, faixas de limites, ciclo de
vida (sem repetir execução incerta, encerrado não reabre), env exato do filho,
home/tmp privados, prazo matando o grupo, watchdog após fim normal, teto de saída,
cancelamento (neto morre, estado `Desconhecido`), carência chegando a worker com o líder
fora, `pasta` link, cópia trocada por link ou por outro diretório real depois de preparar
(execução e cópias), limpeza de subpasta `000` sem seguir link, limpeza que falha e repete,
limpeza recusada com pasta privada substituída, cópia para dentro no mesmo arquivo, cópia para
fora com link/hardlink/sobrescrita, cópia para dentro com pasta trocada por link, encerramento
e sandbox abandonado. Os casos com escapado (`setsid`, drenagem limitada à carência e carência 0)
são só Linux, porque o macOS padrão não tem o utilitário. `processo.rs` (módulo `testes`):
orçamento compartilhado, observação da saída sem recolher o líder, classificação de errno do
`waitid`. `bwrap_tests.rs`: argumentos montados (descritores, `--remount-ro /` depois de todas as
montagens, sem `--info-fd`), `/etc` só com certificados públicos e configuração, allowlist de
ferramentas, sobreposição sob `/opt` e `/usr/local`, origem fixa que é link para dados, home ou
áreas privadas, origem fixa insegura, ferramenta e áreas com link, bwrap de mentira (descritores
herdados, CLOEXEC no pai, sonda que rejeita `--bind-fd` ou `--ro-bind-fd` vira `Indisponivel`) e,
onde o bwrap funciona, home e dados invisíveis, raiz somente leitura (`touch /proibido` falha;
`/trabalho`, `$HOME` e `$TMPDIR` gravam), env exato, `NoNewPrivs`, processo escapado por `setsid`
e prazo que mata tudo na hora, inclusive worker com handler de TERM.
