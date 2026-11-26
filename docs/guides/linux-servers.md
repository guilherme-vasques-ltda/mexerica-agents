# Mexerica em servidor Linux

O Hub Linux roda sem tela, no usuário de login, com serviço `systemd --user`.
Os motores usam uma conta de sistema separada, sem acesso ao token, DB e binário
privados do Hub. A origem deste fork é o Codync; nomes técnicos herdados são
preservados quando necessários. Os binários e o fluxo de instalação próprios
estão no [guia de instalação do Hub Linux](instalacao-hub-linux.md).

## Modo separado dos motores

Requer Linux, `ca-certificates`, systemd e os utilitários de conta/capacidades
listados no guia. Node só é necessário aos adaptadores que o utilizam. O
instalador pede `sudo`, cria `/var/lib/donnabot/{motor,trabalho}`, aplica
capacidades ao ajudante e mantém o link `~/.local/bin/donnabot-host` que o app
procura por SSH. Reinstalação preserva dados; desinstalação conserva as contas
salvo opção explícita. Toda atualização usa o modo separado.

O serviço mantém o D-Bus da sessão para o cofre. O gerenciador de usuário precisa
estar disponível numa sessão real; linger mantém o serviço após sair do SSH.
Confira a sentinela do cofre depois de reiniciar sem SSH antes de aceitar uma
migração. O contrato das raízes e lançamentos está em
[segurança dos motores](../features/seguranca-dos-motores.md).

Adaptadores, Node/npm e CLIs do setup são instalados e atualizados como motor,
pelo ajudante: download, integridade, extração e publicação ficam sob
`/var/lib/donnabot/motor`. Descoberta e estado consultam essa identidade sem o
Hub atravessar as pastas privadas. Pastas antigas do Hub continuam recusadas;
a migração de perfil não move binários: reinstale o conector para obter uma
referência nova na raiz do motor. O Hub confere a publicação por consulta ao
motor, com descritores sem links. Isso não restringe todas as escritas de scripts
a essa raiz: por exemplo, `/tmp` continua sujeito às permissões do motor. O modo separado divide áreas e anexos em
`/var/lib/donnabot/trabalho`, perfis privados em `/var/lib/donnabot/motor` e
expõe `migrar-raizes --data-dir PATH --inventario` (somente inventário JSON) e
`migrar-raizes --data-dir PATH` (referências transacionais após a cópia).
Para gravar, o comando aguarda até um segundo pela `host.lock` do diretório e
recusa se estiver ocupada; pare o Hub antes de executá-lo diretamente.
O instalador e a cópia com privilégios chamam esses subcomandos reais.
Instalações em uso passam pelo comando de migração com cópia e volta. Consulte o [contrato dos motores](../features/seguranca-dos-motores.md).
Sem configuração, os fluxos anteriores continuam iguais.

A migração `donnabot-host migrar-motor` é exclusiva da instalação de teste do
dono, com cópia privada fora da home e volta por hashes. Copia sempre os perfis
CLI já utilizados, sem inferir perfis pessoais; se uma CLI recusar a cópia, refaça
o login pelo app. As guardas de escopo continuam no Hub. A liberação de pastas por
bot não é entregue pelo instalador: ele apenas registra o ABI Landlock.
## Instalação isolada

Só para uma instalação de teste autorizada, nunca sobre a instalação atual. Numa pasta
de dados nova e vazia (`MEXERICA_HOME`), como o usuário do serviço, numa sessão
SSH cuja identidade do servidor já foi conferida por origem independente:

```sh
MEXERICA_HOME=/caminho/novo donnabot-host personal-link init
```

Depois o `serve` dessa pasta abre só o procedimento local; o app usa
`donnabot-host personal-link helper` como comando remoto da âncora SSH.
Hoje nenhum relógio está qualificado e o app não tem âncora: a raiz recusa
(`clockUnqualified`). Cada runtime novo, inclusive o primeiro, espera mais de
24 h antes de novas admissões. Se o Hub reiniciar no meio da primeira entrada, a admissão pendente fica
fechada: refaça o procedimento no app (novo desafio, chave nova). Se faltar marca,
identidade, master ou base, a
pasta fica preservada e fechada; não apague para "consertar".

## Endereço de escuta

Sem `--bind`, o `serve` escuta só em `127.0.0.1` (porta padrão 19422). As unidades
gravadas por `donnabot-host install` e pela atualização gerida fixam `--bind 127.0.0.1`.
`--bind` vazio, com espaços ou com porta é recusado; a porta vai em `--port`.

Para acesso remoto, use o túnel SSH do app ou o relay: os dois funcionam com o Hub em
loopback. Escuta direta na rede (celular pela rede local ou VPN) só com `--bind`
explícito, e o Hub registra um aviso no log. O `--bind` aceita um único endereço, e as
ferramentas locais (`pair`, `status`, `info`, `update` e o app neste computador) falam
com `127.0.0.1`: um `--bind` só no endereço da VPN deixa essas ferramentas sem acesso.
Se a escuta direta for mesmo necessária, use `--bind 0.0.0.0` e limite a porta à
interface da VPN no firewall, mantendo o loopback liberado para as ferramentas locais.
A API local continua aceitando só chamadas do loopback, e o `/health` sem prova
devolve só `ok`, `version`, `environment` e `busy`; a identidade do Hub e do binário exige
a prova do token local (`?nonce=` e `?proof=`: o HMAC do token sobre um nonce novo) vinda
deste computador. Nessa chamada o token não trafega, e a resposta leva um `receipt` (o HMAC
do mesmo token sobre o nonce e a identidade devolvida) para o app saber que falou com o Hub
de verdade. As chamadas do `/api` seguem com o bearer de sempre.

**Migração:** uma unidade antiga sem `--bind` passa a escutar só em loopback quando o
Hub novo reinicia. Um celular que falava direto com o Hub pela rede local ou VPN perde
esse acesso até passar para o túnel ou o relay (ou, em último caso, para um `--bind`
explícito como acima). Num reinício simples, uma unidade com `--bind 0.0.0.0` escrito à
mão continua como está e passa a gerar o aviso. A atualização gerida e a reinstalação
regravam esse `--bind` para `127.0.0.1`; depois delas, a escuta na rede precisa ser
configurada de novo com um `--bind` explícito.

## Referência herdada do Codync

O app conecta por SSH ao Hub usando o link estável. Mantenha a porta do Hub
fechada publicamente; pareamento e canais seguem o contrato existente. Recursos
de tela precisam de sessão gráfica e não são entregues num servidor sem tela.
Logs do serviço: `journalctl --user -u donnabot-host.service`.

A verificação herdada do Codync em contêineres e releases upstream não comprova
a instalação deste fork. O texto abaixo, em inglês, é o procedimento histórico do
upstream, mantido como referência; para instalar esta cópia use o fluxo próprio
acima e o [guia de instalação](instalacao-hub-linux.md), com
`donnabot-host install --ajudante`.
`codync-host` runs on any 64-bit Linux machine: a desktop, a home server, or a headless cloud VM (AWS, GCP, Azure, Hetzner, DigitalOcean…). It needs no display. Bots run on that machine and you message them from the iPhone, the desktop app or `codync-host tui`.

## Requirements

| Need | Why |
| --- | --- |
| x86_64 or arm64 Linux | Release builds exist for both architectures |
| `ca-certificates` | HTTPS to the ACP registry, push relay and cloud. Every normal VM image has it; bare container images (`debian`, `ubuntu`) need `apt-get install ca-certificates`. Without it every command exits with `TLS setup failed: install the ca-certificates package` |
| systemd | Only for `codync-host install` (background service). Without systemd, run `codync-host serve` under your own supervisor |
| Node.js (optional) | Claude Code, Codex and Pi run through ACP adapters fetched with `npx`. Without Node, an agent's setup offers to add Codync's own copy (glibc distros; over SSH: `codync-host node`); on musl distros (Alpine) install Node.js |
| The agents you want, signed in | The host drives the CLIs installed on this machine with their own credentials. Sign in over SSH (`claude`, `codex login`…) as the same user that runs the host |

Distro and C library don't matter. From the first release built by the updated workflow, Linux release binaries are static musl builds with no glibc dependency. Earlier releases (≤ 2.1.0) were built against glibc 2.39 and only start on Ubuntu 24.04 or newer. On older distros they fail with `GLIBC_2.39 not found`; build from source or use a newer release there.

## Install

After obtaining a compatible host binary from your own build, run as the user who owns the agents:

```sh
codync-host install                    # systemd --user service, restarts on crash
sudo loginctl enable-linger "$USER"    # keep it running after you log out of SSH
codync-host status
```

`install` saves the current `PATH` into the service, so install Node and the agents first. If you add an agent later and the host doesn't find it, run `codync-host install` again. `install` needs a real login session (SSH login, not `sudo su user`), otherwise `systemctl --user` can't reach the user's service manager.

Other commands work the same as on a desktop: `codync-host pair`, `codync-host tui`, `codync-host uninstall` (keeps data in `~/.codync`). Logs go to the journal: `journalctl --user -u codync-host -f`.

## Reaching it from the phone

The phone connects over the encrypted device channel. The pairing link lists only **Tailscale** and **private (LAN/VPC)** addresses; the host never advertises a public IP.

- **Tailscale (recommended for VMs):** install Tailscale on the VM and on the phone, then `codync-host pair`. The Tailscale name comes first in the link. You don't need to open port 19222 in the cloud firewall.
- **Cloudflare cloud:** start the host with `CODYNC_CLOUD_URL=https://…` if you run your own Codync cloud. The production cloud isn't live yet.
- **Desktop app over SSH:** the desktop app (macOS or Linux) can attach an SSH computer and tunnel to its loopback API ([Accounts and SSH](accounts-and-ssh.md)). From any terminal: `ssh -L 19222:localhost:19222 vm`, then `codync-host tui` locally.

The host listens on all interfaces, port 19222. The local API (`/api/*`, `/events`, terminals; bearer token) answers **loopback callers only**, so a public address never exposes it. From outside, only `/health` (`ok`, version, environment, `busy`) and the end-to-end encrypted device channel answer, and the channel only serves devices holding this host's pairing keys. Keep 19222 closed in the cloud firewall anyway unless you deliberately pair over a VPN or private network.

## Limitations on a headless server

- **Remote screen and the `computer` tool don't work.** They need a desktop session (portals + GStreamer, `apps/screen-linux/`). The feature turns itself on only while a graphical session is running, so on a server it stays off and nothing else is affected.
- **No desktop app.** The Linux desktop app (`codync`, AppImage/deb/tar.gz) needs a graphical session; it uses the installed `codync-host`. On a server, use the phone, the desktop app on another computer over SSH, or `codync-host tui`.
- **Keep-awake is a no-op.** The host tries `systemd-inhibit` while a bot works; VMs don't sleep, and failure only logs a warning.
- **One computer per account.** A VM counts as the account's computer; multiple computers are future work.
- **Usage limits** come from the local Claude and Codex installs, same as on a desktop.

## Verification — 2026-09-26

Tested in Docker on arm64 (the x86_64 build is the same code and the same CI job):

- Static musl build of `host/` 2.2.0 started on Ubuntu 22.04, Debian 12, Amazon Linux 2023, Rocky Linux 9 and Alpine 3. `serve`, `status`, `pair --json`, `info` and `cloud` worked with no display.
- On a systemd container (Ubuntu 22.04, lingering user): `install` created and started the service, `pair` worked, a killed host was restarted by systemd with a new PID, and `uninstall` removed it and kept the data.
- The v2.1.0 glibc release failed on Ubuntu 22.04 and Debian 12 (`GLIBC_2.39 not found`). The workflow now builds musl for that reason.
- Host tests passed on Linux: 101 + 9 + 1.

Not covered: a real cloud provider VM, pairing a physical iPhone over Tailscale to a VM, and bot turns with signed-in agents on the VM (these need real credentials). Those use the same code paths as a Linux desktop.


O canal e status próprio
integra transporte próprio, mesma Autoridade e Main/IPC de consulta e fechamento local.
Somente raiz completa; sem lista, metadados, quota global,
bots, motores ou ferramentas novas. Produção continua fechada por
`TimeSource::Unqualified`/perfis vazios. Recibo histórico não cria sessão;
`credential*`, env/setup/LOCAL_ONLY e `engine.accounts` continuam recusados.
