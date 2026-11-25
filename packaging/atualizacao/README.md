# Atualização privilegiada — wrapper, etapa administrativa e coordenador

Não instalar em servidor real antes da revisão independente e da revisão de
segurança; a custódia real de chaves é responsabilidade da operação. Este
diretório fica inativo por padrão: nada aqui é ligado a instalador, serviço ou CI.

## O que o binário faz

`donnabot-atualizar-privilegiado` aceita somente as três formas do desenho e
despacha todas para o executor transacional (`host/src/atualizacao/transacao.rs`):

- **Atualizar (sem argumentos):** registra o início na auditoria root, entra no
  cgroup root-owned e confere os limites, retoma pelo journal qualquer transação
  interrompida, obtém timestamp/snapshot/targets/delegações pela origem fixa da
  política, verifica a cadeia TUF e a assinatura Ed25519 do manifesto no horário
  revalidado, descarta o slot inativo de transação concluída, grava manifesto,
  assinatura e snapshot root da transação, baixa os dois artefatos no slot novo
  0700, confere tamanho e SHA-256 de novo a partir do disco, aplica dono, modo e
  `cap_setuid,cap_setgid+ep` só ao ajudante verificado e publica: confere o par
  anterior, fecha seu slot (0700), zera as capabilities, encerra por pidfd quem
  ainda executa o inode ou mantém capabilities sob o UID do Hub (descritor aberto
  falha fechado), expõe o slot novo, troca o seletor atômico e só então abre a
  janela pendente, se ainda restar a validade mínima naquele instante; senão volta
  ao par anterior. Cada fase é gravada no journal antes do efeito.
- **`--reverter`:** usa só journal, manifesto e snapshot root da última transação;
  confere e rearma o par anterior, troca o seletor e fecha o novo (ou remove o
  seletor na primeira migração), restaura `motor.conf` do snapshot e registra
  `revertida`. É idempotente e retomável em cada fase.
- **Requisito de publicação:** a release precisa garantir
  `manifesto.versao == donnabot-host --version == hello.version == CARGO_PKG_VERSION`
  (o pacote publica o binário do próprio build). Se as versões divergirem, o aceite
  falha e toda atualização reverte.
- **`--aceitar`:** exige um registro root-owned da verificação da transação, do
  hash e da versão do Hub, da porta e do instante. O coordenador confere a porta,
  executa `--version`, inicia o serviço e confere o `hello`; uma chamada à forma
  fixa `--aceitar` com o pedido limitado na entrada grava o registro, e outra sem
  entrada o valida antes do aceite. O arquivo é removido na conclusão ou reversão.
  O registro fica na transação privilegiada, separado do journal: assim o guard
  antigo continua lendo o mesmo formato. A garantia depende do fluxo do Hub, que
  declara as conferências. Qualquer processo com o UID do Hub pode solicitar o
  registro: ele não é prova independente contra comprometimento desse UID. Um
  administrador root também precisa das duas chamadas para aceitar manualmente:
  primeiro `--aceitar` com o pedido da verificação na entrada (grava o registro),
  depois `--aceitar` sem entrada (valida e conclui); sem registro válido, o aceite
  direto é recusado inclusive para root.
  Antes de alterar o serviço, o coordenador exige o protocolo novo do wrapper;
  instalações com wrapper anterior precisam atualizar a etapa administrativa
  antes de usar este fluxo. O guard antigo continua lendo o journal sem mudança.
- **Retomada:** `preparada`/`slot_preparado` descartam o slot novo;
  `desativacao_iniciada`, `anterior_desativado` com seletor antigo e
  `cancelamento_iniciado` restabelecem o par anterior (`cancelada`, marca não
  avança); seletor já trocado conclui a publicação se a validade permitir; fases de
  reversão terminam a reversão. Expiração continua bloqueando duravelmente.

Falha no meio da troca aciona a volta imediata pelo journal; se a volta também
falhar, a fase gravada fica para a próxima chamada. Durante a reversão, o guard
não solicita parada antes do prazo para permitir o gate de retomada; na expiração
volta a bloquear e parar o serviço. O diagnóstico da última falha fica em `diagnostico` (root 0600) no estado.

Quando o guard cai (falha do monitor ou `ExecStopPost=... --bloquear`), a contenção
usa só sinais root: com a reversão dentro da janela (trava ocupada, ou trava livre
com retomada válida pelo journal) nada é parado nem bloqueado; com a trava do estado
ocupada por wrapper ou etapa administrativa, ou no desligamento do sistema, só o
serviço é parado; fora disso grava `emergencia`, para o serviço e bloqueia
(fail-closed). A `emergencia` só é gravada depois de obter a trava do estado: se um
wrapper a obteve depois da leitura do journal, a contenção cai para só parar; erro
na trava mantém a emergência. `TimeoutStopSec=30s` cobre o pior caso do
`ExecStopPost` (sonda 3 s, duas auditorias até 5 s cada, `systemctl` até 10 s). O motivo da queda vai ao journal do sistema
(`donnabot-guarda-atualizacao`). Ajuste do relógio (NTP) reavalia o prazo em vez de
derrubar o guard, e a parada de uma unidade ainda não carregada (instalação limpa)
é sucesso.

Resíduos registrados na revisão de segurança (P3, não bloqueantes): um
`donnabot-host` do UID do Hub rodando fora da unidade não é morto pela parada com
unidade não carregada (as trocas seguem exigindo a prova de serviço parado pelo
kernel e a expiração grava `bloqueio`); o `ECANCELED` real do timer ainda não foi
reproduzido com passo de relógio na VM (`date -s` durante a janela e durante a
reversão com a trava ocupada), e nesse ramo de trava ocupada a reversão é
reconhecida pelo relógio de parede, conferido só quando a trava libera.

## Etapa administrativa (`donnabot-instalar-atualizacao`, root direto)

`instalar --login L --pacote DIR --fingerprint-pacote SHA --fingerprint-raiz SHA`:
recusa se executada pelo UID do Hub, confere o pacote (wrapper, guard, instalador, canal do motor,
`raiz.json`, `politica.json`) contra `SHA256SUMS` e o fingerprint dele, e a raiz TUF
contra o fingerprint independente. Preflight: `fs.protected_hardlinks=1`, cgroup v2,
systemd, `/opt` e `/var/lib` sem `nosuid`, relógio com `NTPSynchronized=yes`, D-Bus
de sistema e gerenciador `systemd --user` do Hub alcançável (é o caminho do guard).
Login inequívoco em `/etc/passwd`. Estado: todos os objetos ausentes (instala),
todos presentes (só confere) ou misto/F4 (recusa sem alterar). A release só aceita
modo local: se `/etc/donnabot/motor.conf` estiver presente ou inacessível, a
instalação e a atualização recusam antes de mexer no serviço ou nos dados. Preserve
a instalação anterior e consulte o administrador antes de migrar; nenhum fluxo
remove a configuração automaticamente. No Linux, o `donnabot-host install` direto
também recusa, pois instalaria o modo separado; para uma instalação nova em modo
local, use o fluxo gerido com `--hub-novo`.
Antes de qualquer alteração grava a baseline root (presença anterior
de cada objeto, `motor.conf` e o Hub legado por hashes). Cria conta `donnabot-motor`
nologin, grupo `donnabot-trabalho`, `/var/lib/donnabot/motor` 0700 e `trabalho`
2770 e wrapper/guard/instalador root 0755 em `/opt/donnabot/sbin`, sem criar `motor.conf`;
`/opt/donnabot/releases`, estado root 0700 com política (UID/GID locais),
histórico com a raiz pinada e marca do relógio, cgroup com os limites da política
(recriado no boot por `donnabot-cgroup-atualizacao.service`), guard habilitado e,
por último, o sudoers validado com `visudo` (arquivo e configuração completa) e
conferido por `sudo -l`. Os textos das unidades (guard, cgroup e drop-in
`KillMode=control-group`) vivem em `host/src/atualizacao/instalacao.rs`.
`verificar --login L` confere tudo sem alterar, inclusive o canal do motor (texto
das unidades, socket ativo, dono, grupo e modo); `repinar-raiz` está descrito em
"Rotação da raiz TUF"; `revogar` está descrito abaixo. O pacote tem 7 arquivos:
wrapper, guard, instalador, canal do motor, raiz, política e `SHA256SUMS`. O
`fs.protected_hardlinks=1` é exigido, mas não é configurado pelo instalador: o
administrador deve persisti-lo (`sysctl.d`). Algumas imagens voltam a 0 no boot, e
o wrapper então recusa a atualização.

## Coordenador (`donnabot-host coordenar-atualizacao`, UID do Hub)

**Atualização preparatória comprovada.** Antes de qualquer efeito, o coordenador exige
que a unidade execute o seletor gerido ou exatamente o próprio binário do coordenador
(mesmo SHA-256). O gate de partida (abaixo) só protege se o binário da unidade o tiver,
e a F3 confirma o DB antes de a unidade ser apontada. Um Hub legado anterior ao gate é
recusado sem parar nada: primeiro o próprio Hub legado é atualizado para esta versão,
e o coordenador roda por ele. No layout gerido, todo par publicado já tem o gate.

Para a unidade e espera o cgroup dela esvaziar; obtém `host.lock` e a mantém
durante a troca; faz backup consistente do SQLite (`VACUUM INTO`) com integridade,
hash e schema conhecido, gate F3 (inventário de raízes sem erro), unidade e conjunto
exato de drop-ins; chama o wrapper (`sudo -n`, sem argumentos). Se o wrapper não
publicar, reinicia o serviço anterior. Se publicar, grava o comprovante vinculado à
certidão root, aponta a unidade para `/opt/donnabot/releases/ativo` por drop-in
próprio, libera a trava, inicia e confere a saúde (ativo pelo período e executando
o binário gerido). O drop-in fixa `serve --bind 127.0.0.1` (qualquer `--bind`
anterior, inclusive `0.0.0.0`, é substituído; sem `--bind`, o `serve` também escuta
só em `127.0.0.1`): o Hub gerido escuta só em loopback, e o acesso remoto é pelo
túnel SSH do app. Saudável: `--aceitar`. Senão (ou com `--forcar-reversao`): para,
reobtém a trava, confere o backup contra certidão e comprovante, guarda o estado
posterior em cópia separada, restaura o DB e os drop-ins, chama `--reverter` e
reinicia o par anterior. A espera pela parada usa o `TimeoutStopUSec` efetivo da
unidade mais folga (mínimo 120 s). `--sem-aceite` e `--forcar-reversao` são opções
ocultas só de prova e exigem `--prova`. Depois da publicação e antes de apontar a
unidade, executa a F3 (passo 6 do desenho, abaixo); falha na F3 aciona a mesma volta
completa.

## Canal do motor e F3 (passo 6)

Com a `host.lock` ainda mantida, o coordenador lê o inventário F3
(`motor_migration::inventario_copias`; vazio se a marca `d330.raizes.v1` já
existe) e copia cada raiz **lendo como Hub**, sempre por descritores. A raiz da origem é
aberta componente a componente a partir de `/` (um ancestral trocado por link é
recusado). Cada entrada é conferida com `fstatat` sem seguir links, aberta com
`openat(O_NOFOLLOW)` e precisa ser o mesmo inode conferido. Links, arquivos especiais e arquivos com mais de um
vínculo (hard link) são recusados, o que faz a atualização voltar. Por isso uma troca
concorrente de pasta ou arquivo não consegue levar a cópia para fora da raiz.

- **Raízes de trabalho** (`workspace`, `uploads`, `skills`) vão para
  `/var/lib/donnabot/trabalho`, que pertence ao Hub. O próprio Hub as copia em
  staging `.copia-*`, com os modos do contrato F3:
  - workspace: pastas 2770, arquivos 0660, e o bit de execução do dono vale também
    para o grupo;
  - uploads e skills: pastas 2750, arquivos 0640, nunca executáveis.

  O manifesto (caminhos, tamanhos e SHA-256) é calculado nos dois lados já com essa
  transformação, e os modos são conferidos à parte.

  **Grupo de trabalho explícito.** O grupo é o `work_gid` do `motor.conf`, que precisa
  ser o grupo da raiz `trabalho/` (do Hub, 2770) e diferente do grupo primário do Hub.
  Cada pasta e arquivo da cópia (e cada ancestral criado) recebe esse grupo por
  descritor (`fchown`) e só então o modo do contrato; nada depende de herança de
  setgid. A conferência exige dono Hub e esse grupo em toda a árvore, na cópia nova e
  na repetição, e nos ancestrais já existentes. Uma árvore com o grupo primário do Hub
  deixaria o motor sem acesso e é recusada (a atualização volta). Assim um script 0755 em skills
  não gera divergência. Arquivos e pastas são sincronizados de baixo para cima e a
  publicação usa `renameat2(RENAME_NOREPLACE)`. Depois o pai e os ancestrais
  (inclusive os recém-criados) também são sincronizados.

  Se o destino já existir, só é aceito com o mesmo conteúdo e os modos do contrato
  (retomada); qualquer diferença recusa.

  **Staging privado.** A cópia é montada dentro de `trabalho/.donnabot-staging-hub`,
  uma pasta do Hub em modo 0700. O motor escreve em `trabalho/` pelo grupo, mas não
  cria, renomeia nem apaga nada dentro dela. A cópia sai dali só pelo
  `renameat2(RENAME_NOREPLACE)` que a publica no destino. Depois disso nenhum registro
  aponta mais para o staging.
  - A identidade da área (dispositivo, inode e instante de criação, por `statx`) fica
    em `staging-trabalho.id`, no diretório privado de backups do Hub.
  - Depois de uma queda, a F3 seguinte, sob a `host.lock`, apaga só o conteúdo da área.
    Isso só acontece se a área for exatamente a registrada, do Hub e 0700.
  - Uma pasta posta no lugar da área (o motor pode renomear entradas em `trabalho/`)
    é recusada sem remover nada. A F3 volta, e a pasta é preservada.
  - Uma área vazia pode ser readotada sem risco, porque não há nada a apagar. Isso
    cobre a queda entre criar a área e registrá-la.
  - Pastas `.copia-*` fora da área nunca são tocadas.
- **Perfis de CLI** vão para a home privada do motor pelo canal
  `donnabot-motor-copia`. É um serviço de sistema com `User=donnabot-motor`,
  ativado pelo socket `/run/donnabot-motor-copia.sock`
  (`donnabot-motor:<grupo primário do Hub>` 0660, `RemoveOnStop=yes`). A unidade é
  endurecida: `NoNewPrivileges`, `ProtectSystem=strict` com escrita só em
  `/var/lib/donnabot/motor`, `CapabilityBoundingSet=` vazio e só `AF_UNIX`. Esta é a
  alternativa ao gerenciador `systemd --user` do motor que o desenho deixava a
  validar; dispensa linger para uma conta nologin e sobe no boot (provado abaixo).
  - **Quem pode chamar:** o serviço recusa rodar como root, confere `SO_PEERCRED`
    contra o `hub_uid` de `/etc/donnabot/motor.conf` e exige que a home do motor
    pertença ao seu próprio UID.
  - **Quando:** só na janela da **primeira migração**, conferida nos documentos
    públicos root sem seguir links:
    - `status.json` está `pendente` e dentro do prazo;
    - `transacao.json` é da mesma sequência e não tem sequência anterior.

    É a única transação em que a F3 copia raízes; depois dela o inventário fica
    vazio. Antes da publicação, depois do aceite, depois de uma volta ou com bloqueio,
    o canal recusa.
  - **O quê:** a **única operação** recebe uma **raiz lógica F3 enumerada**: a conta, no
    mesmo formato do registro de contas (`ac_` + 64 hexadecimais), e uma das raízes de
    perfil que `motor_migration` gera:
    - a home de perfil gerida `<uuid>` (o layout de `engine_accounts::managed`), que
      precisa conter a marca `.donnabot-profile`;
    - o perfil de CLI externo `legacy/.claude`, `legacy/.codex` (pastas) ou
      `legacy/.claude.json` (arquivo).

    Qualquer outro caminho é recusado antes de criar pastas. Cada raiz é registrada
    de forma durável em `accounts/.donnabot-raizes` antes de qualquer gravação. Raiz nova que contenha uma registrada, ou esteja contida nela,
    é recusada antes de criar qualquer pasta. Destino existente que não está no
    registro também é recusado. Assim o canal só cria raízes inteiras e nunca insere
    objetos dentro de uma raiz já copiada.
  - **Limites:** 200 mil arquivos, 250 mil entradas (pastas + arquivos), 16 GiB,
    linha de 8 KiB, 64 componentes, 30 s de inatividade por leitura e 2 h por
    transferência. Numa recusa, o dreno da entrada tem 5 s e 64 MiB no total. O serviço
    atende uma conexão por vez e não fica preso por um par ocioso.
  - **Autorização viva:** a janela é revalidada a cada segundo durante a leitura, de
    novo imediatamente antes de publicar ou confirmar uma repetição e mais uma vez logo
    depois do `renameat2` que publica. A conferência e o rename não são atômicos: se a
    janela fechou entre os dois, a conferência posterior desfaz a publicação (volta ao
    nome de staging anotado, que é removido) e recusa. A garantia é esta: só permanece
    publicada a cópia cuja janela ainda estava aberta numa conferência feita depois do
    rename. Um fechamento depois dessa última conferência não desfaz a cópia, que é
    idêntica à origem e fica numa raiz nova da home do motor.
  - **Gravação:** tudo com `openat`/`mkdirat` sem seguir links, modos 0700/0600, em
    staging. Antes do `RENAME_NOREPLACE`, a árvore é sincronizada de baixo para cima
    e precisa ter exatamente as entradas do manifesto. Depois, o pai e cada ancestral
    são sincronizados.
  - **Repetição:** confere o **destino real**: tipos, modos, tamanhos, bytes e
    nenhuma entrada a mais. Não depende de marca. Cada caminho só pode aparecer uma
    vez no fluxo, e uma raiz de arquivo exige exatamente um arquivo. Qualquer diferença
    recusa (e a atualização volta). Hash divergente descarta o staging sem restos.
  - **Staging:** cada staging é anotado em `accounts/.donnabot-staging` antes de
    existir. A chamada seguinte remove só os anotados, por descritor e sem seguir
    links, e registra a remoção no journal do serviço.
  - **Erros:** numa recusa, o serviço responde e drena a entrada com limite, para o
    Hub receber o motivo. O cliente confere que o hash devolvido é o que ele calculou.

Em seguida, `migrar_com_trava` reabre o DB sob a mesma `host.lock` (mesmo inode) e
reescreve as referências. As colunas `cwd` de Donna e Berga são protegidas por
gatilho. Por isso, quando a tabela `donna_fixa_dono` existe, a reescrita dos bots
roda pelo caminho de dono (`donna_fixa::como_dono`). O passo F4 aponta para o
ajudante gerido (`/opt/donnabot/releases/ativo/donnabot-motor-exec`) quando o
seletor existe.

**Coordenação interrompida.** O `comprovante.json` da transação fica durável antes
da F3. Ao apontar a unidade para o par novo, o coordenador grava a marca `apontado`
no backup, só depois de sincronizar o drop-in e o diretório dele (e o pai, se o
diretório foi criado agora). Ao começar uma volta, grava `voltando`. A coordenação
está interrompida quando a janela é retomável e o comprovante dessa transação não tem
`apontado`, ou tem `voltando`. Isso acontece, por exemplo, numa queda no meio da cópia
F3, logo depois da F3 confirmada no DB, ou no meio da volta.

- **Janela retomável:** status `pendente`, ou `revertendo` dentro do prazo. O wrapper
  publica `revertendo` ao registrar o início da reversão (e mantém até concluí-la),
  então uma segunda queda dentro do `--reverter` é distinguida do bloqueio definitivo
  e agenda a mesma retomada. `bloqueado` (inclusive o fim normal de uma primeira
  migração revertida), `liberado` e `revertendo` vencido não são retomados
  automaticamente.
- **Fail-closed:** só a ausência comprovada (`ENOENT` do diretório de backups, do
  diretório de publicação, do `status.json` ou do comprovante de um backup) dispensa a
  conferência. Erro de acesso, de integridade ou de I/O ao ler status, certidão,
  comprovante, marcas ou manifesto impede o Hub de iniciar.
- **Durabilidade da volta:** a restauração dos drop-ins remove e recria as entradas e
  sincroniza o diretório antes do `--reverter`; os diretórios novos de backup têm o
  vínculo sincronizado no pai, e o backup inteiro fica durável antes do wrapper.

- **Nenhum par grava antes da retomada.** O Hub, legado ou gerido, confere isso ao
  iniciar, logo depois de obter a `host.lock` e antes de abrir o DB. Se a coordenação
  estiver interrompida, ele recusa iniciar (fail-closed, também com estado ilegível).
  Assim nenhuma gravação feita depois do boot sai do banco ativo quando a retomada
  restaura o backup.
- **Retomada automática.** Ao recusar, o Hub agenda a retomada na unidade transitória
  `donnabot-retomar-atualizacao` (`systemd-run --user`, fora do cgroup do Hub). Ela
  executa o coordenador, que para o serviço, conclui pela volta (drop-ins e
  `--reverter`) e reinicia o par anterior. Depois disso, uma nova execução do
  coordenador tenta a atualização de novo.
  - Na primeira migração, o DB só é restaurado se a F3 chegou a ser confirmada (marca
    `d330`). Senão o banco atual, que é o do backup, é preservado.
  - Nas demais, a volta é completa.
- **Volta retomável.** O estado atual do DB vai para uma cópia nova (`posterior.db`,
  `posterior-2.db`…), primeiro num nome temporário e depois por vínculo exclusivo.
  Uma cópia posterior de tentativa anterior nunca é sobrescrita nem impede a retomada.
  Se o DB já é o do backup (queda depois do rename), a restauração não é refeita.

## Instalação limpa do Hub no layout gerido (`instalar --hub-novo`)

Para um login **sem Hub legado** (sem unidade `donnabot-host.service` própria, sem
drop-ins e sem `~/.local/bin/donnabot-host`), o administrador instala o Hub direto no
layout novo:

```sh
donnabot-instalar-atualizacao instalar --login L --pacote DIR \
  --fingerprint-pacote SHA --fingerprint-raiz SHA --hub-novo [--porta 19422]
```

1. **Antes de qualquer efeito:** as mesmas conferências da etapa administrativa, e
   ainda a recusa de qualquer sinal de Hub legado. Uma instalação antiga migra pelo
   coordenador, nunca por esta opção.
2. **Parte privilegiada:** a etapa administrativa de sempre. Se já está instalada,
   ela só é conferida.
3. **Primeira release, pelo caminho autenticado:** o instalador executa o wrapper
   como administrador root (`admin/root` na auditoria). O wrapper verifica a cadeia
   TUF e a assinatura, publica o slot, troca o seletor e abre a janela pendente da
   primeira publicação. Não existe atalho que instale o binário sem TUF.
4. **Hub como o UID do Hub:** `setpriv --reuid --regid --init-groups` (sem grupos nem
   capacidades de root) executa `/opt/donnabot/releases/ativo/donnabot-host
   instalar-gerido`. Esse passo:
   - recusa, antes de criar qualquer coisa, se o binário publicado não for de
     produção (`CODYNC_ENV=main`): o layout gerido (unidade, drop-in do guard,
     seletor) é fixo em `donnabot-host.service`, e um build dev gravaria
     `donnabot-dev-host.service` fora dele. A recusa cai no passo 5;
   - recusa se houver unidade fora do par gerido ou dados que não nasceram desta
     instalação;
   - marca os dados como desta instalação;
   - cria a unidade `systemd --user` com `ExecStart` no seletor
     `/opt/donnabot/releases/ativo/donnabot-host ... serve --port N --bind 127.0.0.1`
     (só loopback; o acesso remoto é pelo túnel SSH do app), com o ajudante gerido em
     `ExecStartPre`/`ExecStopPost`, e a inicia;
   - com o Hub saudável, para o serviço e roda a F3 na janela da primeira migração
     (cópias e marca `d330` sob a `host.lock`), para atualizações seguintes não
     dependerem do canal;
   - reinicia, confere a saúde de novo e aceita a janela.
5. **Falha em qualquer passo depois da publicação, inclusive recusa:** `--reverter`
   desfaz a primeira publicação. Toda unidade desta instalação (a gerida, ou
   qualquer uma criada nesta tentativa, inclusive com o nome do ambiente dev) é
   desabilitada, parada e removida; uma unidade alheia nunca é tocada. Se o próprio passo do Hub não
   chegar a desfazer (`setpriv` que não executa, processo encerrado no meio, erro
   antes de saber a fase), a etapa administrativa para a unidade do Hub, confere a
   parada pelo kernel e roda `--reverter` como root, só para a janela da primeira
   publicação. A reexecução retoma, e a auditoria root registra início e resultado.

Depois disso, as atualizações seguem pelo coordenador (`coordenar-atualizacao`), já no
layout gerido. Os planos de instalação remota e do Mac local partem desta instalação
limpa.

## Revogação (`donnabot-instalar-atualizacao revogar --login L`, root direto)

Segue a ordem do desenho, prova cada etapa e falha fechada na dúvida:

1. **Identidade e auditoria antes de qualquer efeito.** Relógio válido exigido. O
   estado precisa estar presente, ou ausente com um estado arquivado de uma revogação
   anterior (reexecução); erro de acesso recusa. O login precisa corresponder a
   `uid_hub`/`gid_hub` da política desse estado. O evento de início é gravado; se a
   auditoria falhar, nada é alterado.
2. **Sudoers.** Com a regra presente, o controle positivo confirma que `sudo -n -l
   -U L <wrapper> [arg]` autoriza cada uma das três formas. Depois a regra é
   removida, `visudo -c` valida e a mesma consulta precisa **negar** cada forma:
   status 1 sem saída. Qualquer outro resultado é inconclusivo e recusa. Isso pega
   regra `ALL`, alias e curingas residuais; o administrador revisa e repete.
3. **Trava.** Obtém a trava exclusiva do wrapper (espera até 10 min por uma
   operação em curso) e a mantém até depois do arquivamento e da auditoria final.
   Sem sudoers, nenhuma chamada nova começa. Uma chamada já iniciada e suspensa antes
   de obter a trava é encerrada por pidfd e conferida (nenhum processo executa o
   inode do wrapper).
4. **Paradas comprovadas.** O Hub e os filhos precisam estar provadamente parados
   pelo kernel: cgroup da unidade vazio e nenhum `donnabot-host` do UID. Guard,
   socket e serviço do canal e unidade do cgroup são desabilitados, e `systemctl
   is-active` precisa dizer `inactive`/`failed`. Falha ao parar só é aceita se a
   unidade comprovadamente não existe.
5. **Ajudante.** Para cada slot: fecha em 0700, abre o ajudante sem seguir links
   (só `ENOENT` conta como ausência; outro erro recusa), zera as capabilities e
   encerra ou recusa quem ainda o executa ou mantém `CAP_SETUID`/`CAP_SETGID` sob o
   UID do Hub. Só depois remove slots e seletor.
6. **Remoção.** Wrapper, guard, instalador, canal, unidades, drop-in `KillMode` e
   socket; o cgroup precisa estar vazio para ser removido. Por fim, `daemon-reload`.
7. **Arquivo.** O estado (política, raiz, journal, auditoria) vai para
   `/var/lib/donnabot/atualizacao.revogada-<ts>` (root 0700). O sucesso é auditado
   só depois do arquivamento durável; falha em qualquer etapa é auditada como
   falha.

Não remove conta, grupo nem dados do motor e do trabalho. O Hub fica parado até
intervenção administrativa: a unidade dele ainda aponta para o par gerido, que deixou
de existir (pelo drop-in do coordenador, numa migração, ou pela própria unidade, numa
instalação limpa). Voltar ao legado é decisão do
administrador (restaurar o backup do coordenador e remover esse drop-in).

## Serviço parado na troca

O wrapper, sem executar filhos, recusa qualquer troca se o cgroup da unidade
`donnabot-host.service` do `systemd --user` do Hub tiver processos ou se houver um
`donnabot-host` do UID do Hub fora dos seus ancestrais (o coordenador). A checagem
ocorre antes de gravar o journal, antes de fechar, rearmar ou trocar o seletor e
de novo logo após a troca (pós-condição: um Hub iniciado no intervalo faz a
transação falhar e a volta exige o serviço parado). Não há congelamento do cgroup:
o intervalo residual entre checagem e troca é coberto pela `host.lock` do
coordenador, pelo status `bloqueado` e pelo guard. Durante a transação o status público é `bloqueado` (`revertendo`
durante uma reversão), então o gate do Hub não inicia par gerido; o guard também para a unidade sempre que encontra a trava
ocupada ou o journal em fase intermediária. Na volta, o encerramento de processos
mira só o inode do ajudante e quem mantém `CAP_SETUID`/`CAP_SETGID` sob o UID do
Hub (o `systemd --user` dele tem `CAP_WAKE_ALARM` e não é tocado).

## Rotação da raiz TUF — política

- **Custódia:** as chaves da raiz ficam offline, com limiar de assinaturas
  definido no próprio `root.json`; responsáveis, quórum e cerimônia reais são
  decisão da operação. As chaves online (timestamp, snapshot) e a de targets
  nunca repetem chaves da raiz (o wrapper recusa).
- **Publicação:** cada rotação publica `N+1.root.json` na mesma origem HTTPS da
  política, assinado **pelo limiar da raiz N e pelo da raiz N+1**. Versões são
  estritamente sequenciais; não há salto.
- **Cliente (wrapper):** antes do timestamp, a cada `atualizar`, busca
  `N+1.root.json` até receber 404 (no máximo 32 por execução), confere os dois
  quóruns e a sequência e persiste raiz e histórico numa única troca atômica sob
  a trava. Raízes intermediárias podem estar vencidas; a final precisa estar
  válida no horário conferido. Se as chaves de timestamp ou snapshot mudarem, as
  versões confiáveis desses dois papéis são descartadas (recuperação de
  fast-forward, TUF 5.3.11); as de targets e delegações continuam impedindo
  rollback. Rotação inválida (quórum faltando, versão pulada, assinatura ruim)
  recusa a atualização sem troca.
- **Comprometimento do quórum da raiz:** a rotação pela rede deixa de ser
  confiável. O administrador obtém a nova raiz e o fingerprint por canal
  independente e executa `donnabot-instalar-atualizacao repinar-raiz --raiz
  ARQUIVO --fingerprint-raiz SHA256`, que exige root direto, confere o
  fingerprint, audita e zera o histórico de versões. Releases atingidas são
  bloqueadas pela política autenticada da nova raiz.
- **Vencimento:** raiz final vencida bloqueia atualizações (fail-closed) até nova
  rotação publicada ou repinagem administrativa.


## Provas (13/11, VM descartável, Debian 13, systemd 257, sudo 1.9.16p2, cgroup v2)

- fmt e Clippy `-D warnings` do workspace inteiro (todos os alvos) limpos.
- Testes focados com `umask 077`, como root: wrapper 36/36, guard 38/38 e
  instalador 36/36 (mesmo módulo, com sobreposição); Hub 4/4 no filtro
  `atualizacao`. Oito testes exigem root e são pulados como usuário comum: os sete
  da transação da rodada anterior e "Hub em execução recusa a troca sem alterar
  nada". Novos: rotação persistida da raiz (só quórum novo, versão pulada, raiz
  vencida intermediária, descarte de timestamp/snapshot só com troca de chave),
  pacote administrativo (fingerprints e somas exatas), política do pacote sem
  identidade do Hub, cgroup populado e critério de capabilities do ajudante.
- Ponta a ponta v2 (`prova-vm/`): 44 verificações, 0 falhas — instalador real com
  recusas fail-closed, coordenador com aceite e com volta completa, reciclagem,
  gerenciador `systemd --user` intacto, rotação da raiz, expiração parada pelo
  guard com bloqueio durável e gate do Hub recusando reinício, `/usr/bin/sudo`
  empacotado como link simbólico e opções de prova recusadas sem `--prova`.
  Amostragem de 50 ms: 4927 amostras, 867 em fase crítica do journal e 6 trocas de
  seletor, nenhuma com o Hub rodando.
- Defeitos encontrados pela prova e corrigidos: hash do binário legado limitado a
  1 MiB no instalador; varredura que encerrava o `systemd --user` do Hub; guard sem
  D-Bus falhava fechado (agora o instalador exige e testa o caminho do guard).

## Provas da fase 3 (13/11, VMs descartáveis, segundo ciclo de revisão técnica e de segurança)

- **Ponta a ponta v3** (`prova-vm/cenarios.sh`): 68 verificações, 0 falhas.
  - **F3 completa no coordenador**, com o layout real de perfil gerido (home
    `<uuid>` com `.donnabot-profile`):
    - perfil copiado como motor com 0700/0600 e bit de execução;
    - skill com script 0755 copiada como 0640 sem divergência;
    - DB reescrito com a marca `d330`;
    - raízes de trabalho com o Hub e o grupo `donnabot-trabalho`.
  - **Isolamento do canal:** socket 0660 com o grupo do Hub e `nobody` sem acesso.
    Root é recusado por `SO_PEERCRED`, e o Hub fora da janela da primeira migração
    também é recusado. O registro de raízes tem só a raiz copiada.
  - **Revogação:**
    - login de outro serviço é recusado sem efeito;
    - regra `ALL` e alias com curinga residuais são detectados, com auditoria de
      falha e estado mantido;
    - a revogação espera a trava do wrapper em uso e encerra (SIGKILL por pidfd) uma
      chamada do wrapper parada no exec antes da trava;
    - a reexecução é idempotente.
  - **Amostragem de 50 ms:** 4913 amostras, 853 em fase crítica, 6 trocas de
    seletor, nenhuma com o Hub rodando.
- **Lint e testes:**
  - fmt e Clippy `-D warnings` do workspace limpos.
  - Testes como root com `umask 077`: wrapper 36/36, guard 38/38, instalador 36/36.
  - Hub: `atualizacao` 7/7, `motor_copia` 8/8, `coordenador` 4/4,
    `motor_migration` 4/4; binário `donnabot-motor-copia` 9/9.
  - Os testes do canal cobrem:
    - destino alterado, removido, com entrada a mais, com modo trocado ou com link;
    - caminho repetido no fluxo e raiz de arquivo sem arquivo;
    - raízes fora das enumeradas, home sem marca, sobrepostas ou não registradas;
    - autorização encerrada durante a transferência, sem publicar;
    - staging anotado coletado, com `.copia-*` legítimas e não anotadas preservadas;
    - hard link, link e ancestral da origem trocado por link.
  - No coordenador, os testes cobrem skill/upload executáveis e workspace que
    preserva a execução.
- **Reboot real** (`prova-vm/reboot.sh`), cinco cortes com `boot_id` novo. Após cada
  boot: cgroup root recriado com os limites, guard e socket do canal ativos.
  - **R4 (queda de energia no meio da cópia F3 da 1ª migração):** corte por sysrq
    com 269 MB de um perfil de 2 GiB no staging do motor e a seq 1 publicada e
    pendente.
    - Na volta, sem estado misto: perfil não publicado, F3 não confirmada no DB,
      unidade sem o drop-in gerido e o Hub legado de volta sozinho.
    - O coordenador detectou a coordenação interrompida e concluiu pela volta,
      preservando o DB: uma gravação do Hub legado feita depois do boot continuou
      lá.
  - **R5 (queda de energia logo depois da F3 confirmada no DB):** um vigia na VM
    disparou o sysrq ao ver a F3 concluída.
    - Na volta, as cópias estavam duráveis, completas e idênticas à origem, e a F3
      estava confirmada no DB. O corte caiu depois do drop-in e antes da marca
      `apontado`, e o Hub subiu no par novo, que corresponde ao DB migrado.
    - O coordenador concluiu pela volta restaurando o DB. A nova tentativa (seq 3)
      reaproveitou as cópias idênticas (staging órfão do R4 coletado) e foi aceita.
  - **R1 (reboot ordenado):** com a janela pendente; o guard reconheceu o
    desligamento, o Hub voltou no par pendente e o aceite após o boot fechou a
    janela.
  - **R2 (queda de energia no meio da transação):** corte com o journal em
    `preparada`. Na volta, sem estado misto: seletor no par anterior, slot novo
    fechado, status `bloqueado` e Hub parado. A chamada seguinte retomou pelo
    journal (auditoria `recuperar`) e concluiu.
  - **R3 (janela vence com a máquina desligada):** corte 3 s antes do prazo e boot
    17 s depois dele (corte 1794587945 < prazo 1794587948 < início do boot
    1794587965). No boot, o guard expirou a janela com bloqueio durável; o Hub ficou
    parado e o wrapper recusa operações.
  - **Execuções:** o ciclo completo (R4, R5, R1, R2 e R3) teve 47 verificações OK e
    1 falha. A falha era a expectativa do R5 para o corte entre o drop-in e a marca.
    Corrigida a verificação para aceitar os dois estados coerentes, a sequência
    instalar, R4 e R5 foi refeita: 21 verificações, 0 falhas.
  - Os cortes abruptos não fazem `sync` global; só os arquivos do próprio roteiro
    de prova são gravados antes.
- Defeitos encontrados pela prova e corrigidos:
  - a F3 falhava no gatilho da Donna fixa (agora usa o caminho de dono);
  - a resposta do canal saía em várias escritas (agora uma só);
  - o socket ficava após a revogação (`RemoveOnStop`);
  - a revogação repetida tratava "command not found" do sudo como inconclusivo.

## Quarto ciclo de revisão técnica e de segurança (15–16/11)

- Rodou na máquina de agentes (usuário comum): `cargo fmt --all --check` e Clippy
  `--all-targets -D warnings` do Hub, limpos; `bash -n` dos roteiros.
- Os testes (inclusive os novos) foram provados só pelo CI do PR (`host-build` e
  nextest): o disco da máquina não comportava o binário de teste do Hub.
- Testes novos: grupo de trabalho em toda a árvore com identidades distintas (Hub com
  grupo primário próprio, motor com outro UID e primário, só o grupo de trabalho em
  comum; credenciais trocadas só na thread de teste, exige root), conferência que
  recusa o grupo primário do Hub, status `revertendo` publicado em cada ponto de
  queda da reversão e retomável só no prazo, ausência comprovada × falha de acesso no
  gate, unidade gerida sempre em `--bind 127.0.0.1` (migração e instalação limpa),
  recusa do Hub legado sem o gate, janela fechada entre a conferência e o
  `renameat2` desfazendo a publicação.
- Não rodou: a prova em VM (`prova-vm/`, R1–R5 e a recusa do legado anterior ao
  gate com `LEGADO_ANTERIOR`), a instalação limpa ponta a ponta e os testes que exigem
  root.

## Pendências

- Recuperação administrativa após bloqueio por expiração (hoje só root manual).
- Delegações TUF além do caminho exato do manifesto.
- Custódia real de chaves, revisão independente e revisão de segurança.

## Limites conhecidos

- A pasta `docs/` dentro da raiz de trabalho é criada pelo runtime do Hub com o
  grupo primário dele, não `donnabot-trabalho`. A F3 cobre só as raízes
  inventariadas; o grupo das pastas criadas depois pelo Hub é assunto do runtime F5.
- O canal do motor não lê o DB do Hub. A autorização é a combinação de UID do Hub,
  janela root da primeira migração e raiz lógica do conjunto enumerado que a F3 gera
  (home gerida com a marca de perfil, ou `legacy/...`), nova e sem sobreposição. Na
  janela, o Hub pode pedir uma home `<uuid>` com marca para uma conta que não esteja
  no inventário. Ela fica numa pasta nova da home do motor e não dá privilégio novo,
  porque o ajudante já executa como motor. Perfis de CLI fora desses formatos fazem a
  F3 voltar.
- O desenho pedia recusar destino existente. A implementação aceita a repetição só
  com a árvore real idêntica, para a nova tentativa depois de uma volta ou queda; as
  cópias publicadas não são apagadas na volta.
- Como a origem é aberta sem seguir links em nenhum nível, uma origem cujo caminho
  passe por link (por exemplo, `/home` como link para outro volume) faz a F3 voltar;
  é preciso configurar o caminho real.
- Hard links em raízes F3 são recusados, e a atualização volta. Um clone git local
  com objetos em hard link, por exemplo, precisa ser desfeito pelo usuário antes.
- Uma queda entre o retorno do wrapper e a gravação durável do `comprovante.json`
  (alguns milissegundos) não é retomada pelo coordenador. A janela pendente vence e
  o guard bloqueia, com recuperação administrativa.
- A retomada automática de uma coordenação interrompida só corre com a janela ainda
  pendente ou com a reversão em andamento dentro do prazo. Se a janela vencer antes
  (por exemplo, máquina desligada além do prazo), o guard bloqueia e vale a
  recuperação administrativa.
- **O canal aceita qualquer conta no formato válido; a fronteira de confiança é o
  Hub, que já executa o motor.** O canal confere UID do Hub, janela da primeira
  migração e o formato da raiz (`ac_` + 64 hexadecimais e uma raiz enumerada), não a
  lista concreta de contas do inventário do Hub. Exigir essa lista só protegeria
  contra um motor que não seja do Hub; a opção de inventário exato fica registrada
  para o caso de surgir esse motor.
- **Bind:** as unidades do layout gerido (instalação limpa e migração) escutam só em
  `127.0.0.1`. O instalador Linux (`donnabot-host install`) também grava
  `--bind 127.0.0.1`, e o `serve` sem `--bind` escuta só em loopback; escuta na rede
  só com `--bind` explícito. O acesso remoto ao Hub gerido é pelo túnel SSH do app.
- O gate de partida só protege Hubs que o contêm. Por isso a coordenação exige a
  atualização preparatória do Hub legado (mesmo binário do coordenador na unidade); um
  Hub legado anterior não é migrado diretamente.
- A instalação limpa precisa alcançar a origem TUF da política no momento da
  instalação, como uma atualização.
- O canal atende uma conexão por vez. Um par ocioso o ocupa no máximo 30 s por
  leitura, e uma transferência dura no máximo 2 h.
- Revogação não restaura o legado sozinha: o Hub fica parado até o administrador
  decidir.
- O gate F3 do coordenador só exige o inventário de raízes; compatibilidade de schema
  com o par anterior é garantida por restaurar o backup do próprio par (o schema
  aceito é o conhecido em `backup_atualizacao`), não por migração reversa.
- O backup não copia diretórios de credenciais; só DB, unidade e drop-ins.
- Não há chave de produção pinada; a configuração de confiança só entra por
  `donnabot-instalar-atualizacao` com fingerprints conferidos por canal independente.

## Notas para a revisão de segurança do PR

- Decisão de projeto (13/11): auditar o chamador pelo `SUDO_UID` gerado pelo sudo,
  com a regra sudoers definida exatamente como está, sem `stay_setuid`; execução
  direta de root é registrada como `admin/root`. A revisão de segurança deve
  confirmar a sanitização nas configurações e versões de sudo suportadas e
  avaliar a identificação do processo pai e a dependência de procfs e pidfd,
  inclusive o caso de `SUDO_UID` remanescente em shell root.
- Decisão de projeto (13/11): comparar relógio de parede e monotônico e falhar
  fechado diante de suspeita ou inconsistência, sem bloquear esta fase por
  ausência de uma fonte autenticada. A comparação não autentica a hora e não
  comprova o tempo decorrido durante desligamento; a revisão de segurança deve avaliar esses limites.
- A etapa administrativa exige `NTPSynchronized=yes` (fail-closed); a revisão de segurança deve
  avaliar se essa exigência é suficiente ou excessiva para os servidores suportados.
- O wrapper identifica a chamada por sudo comparando o executável do processo pai
  com `/usr/bin/sudo` resolvido (link aceito), exigindo link, alvo e ancestrais root
  e não graváveis por grupo/outros.
- O wrapper reconhece o Hub em execução por cgroup da unidade e por nome de
  processo do UID do Hub; a revisão de segurança deve avaliar unidades renomeadas ou Hub fora do
  `systemd --user`.
- Canal do motor: a revisão de segurança deve revisar:
  - a autorização (`SO_PEERCRED` do Hub, janela root da primeira migração e registro
    de raízes sem sobreposição);
  - a escolha de unidade de sistema com `User=` no lugar do gerenciador do usuário
    motor;
  - a conferência da repetição pelo destino real.
- Revogação: a revisão de segurança deve avaliar:
  - a ordem (identidade e auditoria, sudoers, trava, paradas comprovadas,
    capabilities);
  - a consulta efetiva de cada forma por `sudo -l -U <login> <comando>`, com controle
    positivo e negação por status;
  - o arquivamento do estado no lugar de apagá-lo.
