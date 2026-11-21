# Mexerica — backup cifrado do Hub

Documento adaptado em 28/10/2026. Este é o procedimento versionado do componente; não registra o estado operacional de uma instalação e não autoriza repetir backup, restore ou ativação já concluídos.

**A ativação real depende das conferências de ativação e das chaves provisionadas pelo administrador.** Este
diretório não instala nada sozinho. Os scripts não iniciam nem param o Hub, não
usam sudo e não criam recursos na Cloudflare. A prova usa apenas dados fictícios.

## O que é preservado

O pacote `hub-*.dbkp` contém:

- Uma cópia consistente de `codync.db`, feita pela API Online Backup do SQLite,
  com o banco original aberto em leitura. Inclui bots/configurações, entradas das
  conversas, rotinas, memória do banco, regras e os registros já cifrados do cofre.
- `bots/<UUID>/`: workspace, memória longa (`bot-memory`), arquivos publicados e
  anexos. Inclui `shared-memory`, `skills` e `harness-context.json` quando existem.
- Conversas de Space: workspaces e uploads de
  `spaces/<espaco>/conversations/<chat>/` e os arquivos que a conversa publica em
  `bots/<chat>/files/`. A memória de cada camada de Space
  (`bots/<uuid>/bot-memory`, referenciada por `donna.space.memory.*` no banco)
  também entra. `mexeriquinhas/gerados/` (avatares gerados) e `grupos/<id>/pasta`
  entram pelo mesmo critério de dado do usuário.
- **Modo de motor separado** (`/etc/donnabot/motor.conf` presente): os mesmos
  dados do usuário ficam na raiz de trabalho configurada em `motor_work`
  (normalmente `/var/lib/donnabot/trabalho`): `bots/`, `spaces/` e `skills/`
  dessa raiz. Cwds sob `motor_work` precisam seguir `bots/<id>/workspace` ou
  `spaces/<id>/conversations/<id>/workspace` (ou descendentes); caminhos como
  `background/` são recusados. O usuário do Hub precisa conseguir ler as raízes
  selecionadas, inclusive quando o motor usa `umask 0077`. Ajuste o acesso por
  grupo/ACL sem abrir perfis privados; falha de leitura aborta com
  `raiz_sem_permissao:<raiz>` em vez de omitir dados. Fora dela ficam `/var/lib/donnabot/motor` (perfis e
  credenciais dos motores), `/opt/donnabot` (binários) e `trabalho/background`,
  `trabalho/probe` e `trabalho/docs` (efêmeros ou derivados, como no legado).
- Workspaces externos **somente** quando os caminhos absolutos foram incluídos
  em `external_workspaces`; são guardados em `external/<UUID>`. Um bot ativo com
  pasta externa fora da lista faz a operação falhar, em vez de perder arquivos
  silenciosamente. Pastas ausentes de bots ativos também causam falha. Uma pasta
  dentro da própria árvore do bot ou nos layouts permitidos do modo separado é
  incluída uma vez, sem exigir lista externa.
- Manifesto v2 com SHA-256, tamanho/estado das referências a anexos, modos rwx,
  texto dos links simbólicos e contagens de bots ativos, chats com
  entradas e entradas. Não existe tabela independente de conversas: cada bot tem
  seu chat, e as entradas preservam seus turnos.

Ficam fora: token administrativo (`token`), identidade do computador
(`identity.json`), WAL/SHM vivos, locks, logs, cache/instaladores dos motores,
logins `.codex`, `.claude`, `.anthropic`, `.config`, `.ssh`, `.aws`, `.local`,
`.gnupg`, `.docker`, `.kube`, `auth.json`, `credentials.json`, `.netrc`, `.npmrc`,
`.pypirc`, históricos de shell, `.env`/`.env.*`, arquivos `.pem`/`.key`, `.git`,
`node_modules`, `target`, `.venv` e `__pycache__`. Não se percorre o home inteiro.

Exclusões deliberadas de credenciais e sessão, com a consequência registrada:

- **Sessão WhatsApp** (`whatsapp/<bot>/session.db` e arquivos `-wal`/`-shm`):
  contém a credencial de vínculo do número e **não entra no backup** —
  exclusão **provisória**. Consequência: depois de um
  restore é preciso **re-parear os números** do WhatsApp.
- **Perfis e logins dos motores** (`/var/lib/donnabot/motor` no modo separado e
  `engine-setup/accounts/` no legado): são credenciais de motor do cliente e
  **ficam fora por decisão fechada**. A justificativa contratual do
  projeto aguarda registro formal. Consequência: o restore **exige novo
  login de cada motor** (Claude, Codex etc.).
- **Binários** (`/opt/donnabot` e equivalentes): fora do backup por serem
  reinstaláveis, não dado do usuário.
- As árvores `.codex`, `.claude`, `.grok`, `.anthropic`, `.gemini`, `.opencode`
  e `engine-setup` aninhadas nas raízes selecionadas são excluídas por inteiro,
  assim como `.claude.json`. Raízes explícitas que apontem para esses perfis ou
  abrangem os diretórios de perfis conhecidos são recusadas. O Hub usa perfis
  Codex e Claude; os demais nomes são uma proteção adicional.

**O pacote é sensível.** Arquivos arbitrários de workspaces, Spaces e anexos
podem conter segredos escritos por usuários ou agentes. A higiene por nomes é
best-effort e não detecta segredos em conteúdo arbitrário. Fora de anexos e
publicados, a cópia também exclui arquivos com cabeçalho PEM completo de chave
privada (inclusive RSA, DSA, EC, OpenSSH e chave cifrada). Anexos e publicados
nos caminhos reais do Hub são preservados mesmo com nome ou conteúdo sensível.
Diretórios de perfil de motor continuam fora,
inclusive sob uploads. Proteja o pacote inteiro: AES-GCM, bucket R2
privado com object-lock e chave privada no 1Password. Antes de usar retenção
indefinida, considere que um segredo enviado pode não ser removível. O risco
residual aguarda aceitação formal antes do uso.

Fora desses anexos/publicados, os nomes abaixo são comparados sem distinguir
maiúsculas, por componente inteiro, nunca por substring:

- `.codex`, `.claude`, `.anthropic`, `.cursor`, `.continue`, `.aider`, `.azure`,
  `.gcloud`, `.config`, `.local`, `.ssh`, `.aws`,
  `.gnupg`, `.grok`, `.gemini`, `.opencode`, `.claude.json`, `engine-setup`,
  `.git`, `node_modules`, `target`, `.venv`, `__pycache__`, `token`,
  `identity.json`, `auth.json`, `credentials.json`, `.netrc`, `.docker`,
  `.kube`, `.npmrc`, `.pypirc`, `.bash_history`, `.zsh_history`, `.mcp.json`,
  `.git-credentials`, `.pgpass`, `.vault-token`, `hosts.yml`, `hosts.yaml`,
  `Login Data`, `Local State`, `credential`, `credentials`, `secret`, `secrets`,
  `privatekey`, `private_key`,
  `private-key`, `oauthcreds`, `oauth_creds`, `oauth-creds` e variantes desses
  três últimos com `.json`;
- `.env` e `.env.*`, `cookies` e `cookies.*`, `id_rsa`, `id_dsa`,
  `id_ecdsa`, `id_ed25519` e variantes desses quatro com sufixo `.*`;
- `credential`, `credentials`, `secret`, `secrets` e `token` com extensão `.json`,
  `client_secret.json`, `client_secrets.json` e `client_secret_<identificador>.json`
  (também com hífens; o identificador aceita letras, dígitos, ponto, `_` e `-`);
- `service_account*.json` (sufixo com letras, dígitos, ponto, `_` e `-`),
  `application_default_credentials.json`, `private_key.json`, `secrets.toml` e
  `credentials.tfrc.json`;
- qualquer componente terminado em `.pem`, `.key`, `.p12` ou `.pfx`.

Diretórios aninhados com prefixo `.mexerica`, `.donnabot` ou `.codync` também
são excluídos. Cada novo snapshot com exclusões registra no stderr/journal um
aviso JSON `itens_excluidos_do_backup` com a quantidade de itens ou raízes omitidos, sem
caminhos nem conteúdo. A lista detalhada fica em `not_read` no manifesto cifrado.
Uma raiz externa com nome reservado é omitida com o mesmo aviso; uma raiz de
perfil de motor continua recusada. Exclua apenas o necessário e confira o
manifesto localmente se o aviso indicar dados desejados.

O valor `kv.token` é apagado do SQLite copiado, junto dos arquivos de token
e identidade que já ficam fora da seleção. `secure_delete` + `VACUUM` removem
restos do token das páginas livres. Na primeira inicialização autorizada de uma
cópia, o Hub encontra `kv.token` ausente e chama `rotate_token` para gerar um
valor novo e gravar o arquivo local; nenhum valor é exibido por estes scripts.
A restauração recusa um pacote cujo banco ainda contenha `kv.token`.

Os pareamentos e tickets de dispositivos (`devices`, `push_tickets`,
`activity_tickets`) são apagados **só na cópia**. `secure_delete` e `VACUUM`
retiram seus restos das páginas livres. Um Hub recuperado requer novo token,
nova identidade e novo pareamento. Não se exportam logins dos motores.

O pacote **separado** `vault-*.dbkp` guarda `keyrings/` do Secret Service, inclusive
`login.keyring` e `user.keystore`, sem ler nem imprimir uma chave pelo serviço.
Ele contém o cofre desse usuário e deve ter acesso restrito. Nunca anexá-lo a um
relatório nem compartilhá-lo como pacote comum. Não inclui logins CLI externos.

## Consistência e limites

O SQLite é copiado pela API de backup, nunca copiando o arquivo vivo. Compara-se
`PRAGMA data_version` antes/depois e inventário de arquivos (inode, tamanho,
mtime/ctime), além das verificações durante cada leitura. Alteração detectada fora dos anexos ou
arquivo especial causa falha; tentar novamente em horário quieto. Links comuns
(inclusive venv com alvo absoluto ou quebrado) são guardados como links, sem
abrir o alvo, e restaurados com o mesmo texto. Um tar com arquivo descendente
de um link é recusado, independentemente da ordem dos membros.

Anexos e publicados podem ter sido editados ou apagados desde a entrada. Isso
não bloqueia o backup: o manifesto registra `copied` com SHA-256/tamanho dos
bytes efetivamente copiados, ou `missing` se não houver arquivo recuperável.
`not_read` registra caminhos que desapareceram no inventário ou na leitura,
inclusive sem referência em uma entrada. Só ENOENT é tolerado nesses caminhos;
EACCES, ENOTDIR, ELOOP e troca de pasta por link causam falha, sem publicar ponto
incompleto. Um arquivo ilegível nunca some de um backup marcado como sucesso.
`matches_entry` informa se coincide com a entrada histórica, sem exigir isso.
A restauração confere os arquivos e as referências contra o **manifesto desse
ponto**, preservando inclusive a ausência registrada. Não recria conteúdo já
apagado. Modos rwx e execução são preservados; setuid/setgid não são restaurados.

Isso é uma janela estável conferida, **não um snapshot atômico do filesystem**.
Um agente com shell no mesmo usuário continua podendo adulterar dados e scripts
(D62). A separação de usuários é outra etapa. O manifesto detecta divergências
da cópia, não prova autoria contra alguém que já controla o Hub.

O script verifica `host.lock` antes e durante a cópia SQLite e depois de copiar
os arquivos. Se detectar manutenção, aborta com `hub_em_manutencao`; o daemon
`donnabot-host serve` pode coexistir. Se a trava estiver ocupada ou seu estado
for indeterminado e `/proc/locks` não puder identificar o detentor, a operação
falha fechada com erro claro. A identidade do banco também é conferida durante
a cópia e ao fim da janela: uma substituição aborta o ponto. Essas verificações
não tornam o filesystem um snapshot atômico; mantenha os timers pausados nas
janelas de manutenção sempre que possível.

Limite inicial: 256 MiB de arquivos por pacote e de arquivo comprimido, com
operações em blocos de 1 MiB. Se exceder, falha antes do envio. O limite é para
evitar crescimento silencioso; não é garantia de que a conta R2 inteira esteja
no plano gratuito. Conferir o volume total com o dono antes de aumentar o limite.
Credenciais de CLI excluídas devem ser recuperadas separadamente.

Temporários ficam sob `state_dir`, modo 700, arquivos 600; são removidos na saída
normal, falha ou SIGTERM. A exceção é `hub-pending/` ou `vault-pending/`: contém
somente envelope cifrado e registro opaco persistentes para retomar a conferência. Após falta de energia/SIGKILL, conferir e remover os
diretórios `job-*` abandonados antes de reativar. Não colocar `state_dir` dentro
de um workspace ou da pasta de dados do Hub.

## Cifra e envio

Usa `cryptography` já presente: AES-256-GCM em fluxo, chave aleatória de 32 bytes
e nonce aleatório de 12 bytes por objeto. A chave AES é encapsulada por uma chave
pública RSA-4096, OAEP com SHA-256 e MGF1-SHA256. O rótulo OAEP é `DNBKP001`.
Formato: `DNBKP001` (8 bytes), chave encapsulada (512), nonce (12), ciphertext e
tag GCM (16). Os 532 bytes do cabeçalho são dados autenticados (AAD). Restauração
autentica a tag inteira antes de abrir o tar. Não usa CMS nem `openssl enc`.

Só a chave pública fica no Hub. A privada fica no 1Password e só é usada no
computador isolado de recuperação. O Hub consegue cifrar, mas não decifrar.
Quem tem a pública pode criar um pacote novo; GCM não substitui assinatura de
origem, controle do bucket e escolha de um ponto de recuperação confiável.

R2: API S3 por HTTPS, curl `--aws-sigv4 aws:amz:auto:s3`, sem redirecionamentos,
credenciais passadas por stdin e fora de argumentos/ambiente/logs. Faz PUT do
objeto cifrado, baixa-o e compara SHA-256 antes de registrar sucesso.
Antes do PUT, publica atomicamente `hub-pending/` ou `vault-pending/`, com
fsync do envelope e metadados, e persiste que o envio começou. Após PUT incerto
ou falha do GET, as próximas tentativas fazem **só GET do mesmo objeto**, com os
mesmos bytes; não criam outro nome nem sobrescrevem o anterior. O envelope fica
rastreável até conferir. Se o PUT nem chegou ao R2, a pendência bloqueia novos
envios: um operador autorizado deve conferir a existência pelo R2, preservar
qualquer objeto enviado e só remover a pendência após reconciliar o resultado.
Não apagar pendências automaticamente. Uma pendência `hub-pending` criada antes
desta política falha com `pendente_hub_politica_antiga` sem envio nem GET:
reconciliar seu estado no armazenamento e preservar o registro conforme o
procedimento autorizado antes de liberar novos pontos. A data original `point_at` permanece no
status; confirmar um ponto velho não o faz passar pelo alerta de 26 h. Nenhum
arquivo em claro é enviado. Há prazo de rede e limite de tamanho no download.

## Retenção e proteção contra exclusão

A política exige decisão explícita antes do primeiro envio. O script não escolhe
prazos, não configura regras R2, não lista nem apaga objetos e não prova que o
config corresponde ao bucket. O exemplo continua inválido até a configuração
ser completada pelo operador autorizado.

### Política de 7 dias

Quando a janela aprovada de backup e recuperação for **7 dias**, configurar:

```json
{"retention":"bucket-lock-age","lock_days":7,"expiration_days":7}
```

No bucket existente, aplicar trava por idade de **604800 segundos** e expiração
após **7 dias** somente aos prefixos autorizados dos pacotes Hub e cofre
(`backups/hub-` e `backups/vault-`, quando esse for o alcance aprovado).
Recuperar as regras atuais antes de preparar a mudança; preservar regras e
objetos alheios. Regra geral ou concorrente com retenção maior é conflito a
resolver com o responsável, não autorização para removê-la ou aceitar outra
janela silenciosamente. Manter a regra de abortar multipart incompleto após
7 dias, que é distinta da expiração de objetos completos.

O parser aceita `lock_days` inteiro **a partir de 7** e `expiration_days` inteiro
**igual ou maior** que a trava. Assim, aceita 7/7 sem declarar uma política falsa
35/90. Outras janelas e `bucket-lock-indefinite` permanecem compatíveis com a
fonte, mas dependem de autorização própria; não são fallback para a decisão
7 dias. Retenção não altera a agenda diária 03:15 UTC nem o alerta de 26 h.

A trava impede exclusão/sobrescrita antecipada enquanto vigente; a conta
administrativa pode mudar regras. Lifecycle normalmente remove em até 24 h
após a expiração, com possíveis atrasos. **Não prometer exclusão física exata
em 168 horas nem recuperação além da janela aprovada.** Após 7 dias sem pontos
novos pode não restar nenhum backup. Ao alerta de 26 h, investigar; ampliar
retenção, suspender expiração ou preservar pontos além da janela exige decisão
explícita. Não manter automaticamente o último sucesso para sempre.

Essa política abrange cópias Hub/cofre, não dados originais, chave de recuperação,
perfil, relatórios ou rollback do app. Não apagar backups anteriores por script
ou limpeza manual nesta instalação. Qualquer regra que os alcance precisa ter
seu alcance e efeitos explicitamente aprovados.

### Conferências antes de ativar

1. Confirmar o bucket existente privado, dedicado, Standard, sem domínio público,
   jurisdição e prefixos autorizados. Não criar conta/bucket/plano por inferência.
   Conferir volume atual, uso agregado e orçamento; 256 MiB por pacote não é teto
   de custo. Alertas de orçamento não bloqueiam cobrança.
2. Conferir por metadados administrativos as regras de lock/lifecycle efetivas e
   o diff restrito aos prefixos autorizados. Para 7/7, não usar trava indefinida,
   35 dias ou expiração de 90 dias. Se a credencial de objetos não permite essa
   consulta, obter conferência fora do Hub; não entregar poder administrativo
   ao servidor ou criar um token mais poderoso por conveniência.
3. Credencial S3 **Object Read & Write**, só no bucket escolhido, sem administrar
   regras. Esse perfil pode excluir objetos quando a trava não estiver vigente.
   Guardar no 1Password e provisionar só o arquivo 600 necessário ao usuário do
   Hub. Conferir validade; registrar `credential_expires_at` quando existir e
   combinar rotação fora do Hub. Não imprimir credenciais nem URLs assinadas.
4. No computador confiável do dono, gerar par **RSA-4096**:

   ```sh
   umask 077
   openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:4096 -aes-256-cbc -out /PASTA/SEGURA/private.pem
   openssl pkey -in /PASTA/SEGURA/private.pem -pubout -out /PASTA/SEGURA/public.pem
   ```

   Guardar privada cifrada/senha, pública/impressão e privadas antigas em item
   novo de recuperação no **cofre privado individual integrado** do dono
   (`Private`/`Personal`, conforme a conta). Nunca no `Acesso a Bots`, no
   `Donnabot · Pessoal` ou em cofre acessível à Service Account do Hub. Conferir
   persistência e leitura de recuperação pelo `op`, comparando em memória sem
   revelar valores, antes de usar a pública. Não sobrescrever item/chave antiga.
   Só a pública vai ao Hub; a privada nunca vai ao servidor dos bots. Arquivos
   temporários necessários ficam em pasta 700/arquivos 600 e são removidos após
   a verificação. Os dois pacotes usam o mesmo par RSA.
5. No mesmo cofre individual, preservar o procedimento/senha de recuperação do
   keyring, sem inferir senha nem exportar chave-mestra. Acesso a R2/1Password
   precisa continuar disponível se o Hub for perdido. Pacote do cofre é
   restrito e separado; nunca anexá-lo a relatório ou Git, mesmo cifrado.
6. Após custódia recuperável, regras e fonte conferidas, executar cópias reais
   Hub/cofre sequencialmente, confirmar GET/hashes e restaurar em destinos novos
   inertes no computador confiável. Não iniciar Hub/modelos/conectores/rotinas.
   Extração íntegra do keyring não prova autenticação funcional do Secret Service;
   esta requer ambiente confiável isolado e prova própria, sem importar no perfil
   original de Mac/Hub. Registrar esse limite quando o ambiente não existir.
   Em objeto **fictício** autorizado sob os prefixos, a conferência de proteção
   pode testar negação de DELETE/sobrescrita; nunca usar objeto antigo/real nem
   fingir que uma prova local valida R2. Só após os GET/restores reais e regras
   conferidas habilitar os dois timers. Nenhum prazo de lifecycle foi observado
   apenas por ler/aplicar a configuração; registrar horário e comportamento real.

Preços e franquia agregada devem ser conferidos antes de estimar custo:
[preços R2](https://developers.cloudflare.com/r2/pricing/). Nenhuma hipótese de
1 MiB/dia ou custo zero substitui medição do pacote e uso atual da conta.

## Configuração e instalação após as conferências

Tudo como o usuário do Hub, sem sudo. Copiar `backup.py` para
`~/.local/lib/donnabot-backup/backup.py` e as unidades para
`~/.config/systemd/user/`. Verificar antes: `python3 -c 'import sqlite3,
cryptography'` e `curl --version`. Se faltarem, propor a instalação; não instalar
automaticamente. Não é necessário compilar o Hub.

Criar `~/.config/donnabot-backup` (700). Adaptar `config.example.json` com caminhos
**absolutos**, salvar como `config.json` (600). No modo de motor separado,
preencher `motor_work` com a raiz de trabalho absoluta (por exemplo
`/var/lib/donnabot/trabalho`); no modo legado, manter `null`. O valor precisa
ser um diretório existente, não pode se sobrepor a `hub_home` nem aos perfis
conhecidos e suas subárvores `bots`, `spaces` e `skills`, quando presentes,
precisam ser diretórios reais, sem links.
Copiar só `public.pem` do item de
recuperação para essa pasta. Criar `r2.json` (600) a partir do 1Password, sem saída
no terminal, no formato abaixo (os marcadores são apenas nomes de campos):

```json
{"access_key_id":"ACCESS_KEY_ID","secret_access_key":"SECRET_ACCESS_KEY"}
```

O script recusa configuração, credencial ou chave privada sem dono correto e modo
600. Pastas ancestrais simbólicas, como `/tmp` e `/var` no Mac, são resolvidas
para caminhos canônicos antes da abertura segura; links no último componente
de configuração/chave/estado continuam recusados com erro claro. Cada operação tem seu lock em `state_dir`, prazo de 45 min e prioridade
baixa. Hub e cofre usam unidades independentes; uma falha não cancela o outro
e cada unidade tenta novamente após 15 min (`Restart=on-failure`), com
**no máximo três partidas em uma janela de um dia** (`StartLimitIntervalSec=1d`,
`StartLimitBurst=3`). Depois do limite, corrigir a causa antes de resetar a falha;
não desabilitar o limitador para insistir.
Depois da prova real e da autorização, `systemctl --user daemon-reload` e
`systemctl --user enable --now donnabot-backup.timer donnabot-backup-health.timer` habilitam a agenda diária,
03:15 UTC com atraso aleatório de até 15 min; `Persistent=true` recupera uma
execução perdida quando o usuário/servidor volta. Isso não depende do app aberto.
O timer dispara `donnabot-backup.target`, que pede separadamente
`donnabot-backup.service` e `donnabot-vault-backup.service`; não há `Requires`
entre os jobs. Nenhuma unidade é habilitada por estes arquivos sozinhos.

O journal e `state_dir/{backup,vault-backup,restore}.json` registram sucesso,
horário, duração, objeto opaco e bytes, sem conteúdo, caminhos pessoais ou nomes.
O journal também recebe a quantidade de itens excluídos pelo filtro.
`*-failure.json` preserva a última falha sem apagar o sucesso anterior; comparar
as datas. `systemctl --user status donnabot-backup.timer donnabot-backup.service`
mostra falhas/interrupções e o próximo disparo. SIGTERM/SIGINT gravam falha
(`interrompido_SIGTERM`/`interrompido_SIGINT`) e limpam temporários; o último
sucesso fica separado, nunca é reescrito como sucesso da tentativa interrompida.
`restore.json` inclui objeto, data do manifesto restaurado e bytes cifrados.

O timer `donnabot-backup-health.timer` confere de hora em hora (primeira vez
15 min após boot) **os dois** pacotes: falta de sucesso, sucesso com mais de
26 h, tentativa falha posterior ao sucesso ou token a vencer em 14 dias geram
status não saudável e falha no serviço, com evento de erro no journal e
`check-health-failure.json`. `python3 .../backup.py check-health --config ...`
permite a mesma consulta. O JSON de falha contém todos os códigos em `alerts`;
uma validade inválida não esconde atraso/falha dos pacotes. Datas sem fuso,
inclusive apenas AAAA-MM-DD, são UTC. É um **alerta local consultável**, não uma mensagem
push/e-mail; monitor externo e seu canal precisam de autorização separada.
O futuro painel deve considerar
**os dois pacotes**, sua idade e a última restauração real, não só exit code.

## Recuperar em pasta nova, sem afetar o Hub vivo

1. Preparar máquina/usuário isolado, com disco privado e sem Hub iniciado. Não
   reutilizar o diretório do Hub vivo. Antes de executar um Hub recuperado,
   bloquear egress e desabilitar rotinas, filas/ações pendentes e conectores;
   isso é uma etapa operacional separada, **não é feito por este script**.
2. Recuperar credenciais R2 e a privada do 1Password diretamente para arquivos
   600 em pasta temporária 700, sem `cat`, `echo`, logs ou valores no chat. Usar
   exportação do app ou `op document get ... --out-file /PASTA/SEGURA/private.pem`.
   A senha da privada é pedida de forma oculta. Preparar configuração local com
   `state_dir`, credenciais e `hub_home` original; a pública não é usada ao decifrar.
3. Escolher os objetos `backups/hub-...dbkp` e `backups/vault-...dbkp` do mesmo
   ponto diário no bucket privado. Não existe comando que importe no Hub vivo.
   Exemplo (substituir os caminhos/objeto):

   ```sh
   python3 /PACOTE/backup.py restore --config /RECUPERACAO/config.json --object backups/hub-DATA-ID.dbkp --private-key /RECUPERACAO/private.pem --destination /RECUPERACAO/hub-novo
   python3 /PACOTE/backup.py restore --config /RECUPERACAO/config.json --object backups/vault-DATA-ID.dbkp --private-key /RECUPERACAO/private.pem --destination /RECUPERACAO/cofre-novo
   ```

   Os nomes acima são marcadores; usar os nomes exatos registrados pelo backup.
   A pasta pai precisa existir; o destino **não pode existir**. Destinos symlink
   e caminhos que entram no Hub configurado são recusados. Links dentro do
   pacote são preservados sem seguir seus alvos. Não iniciar o Hub nem
   o Secret Service durante essa extração. Há verificação GCM, SHA-256 de todos
   os arquivos, `PRAGMA integrity_check`, contagens iguais ao manifesto,
   arquivos e referências a anexos conforme o manifesto, incluindo ausências
   registradas. A promoção entre discos ocorre com diretórios graváveis, e só depois aplica
   os modos finais, das pastas mais fundas para as rasas. Uma falha de promoção
   remove o destino, inclusive pastas 0555; erros distinguem destino removido de
   limpeza incompleta (esta exige intervenção, sem declarar sucesso).
4. Conferir `manifest.json` **localmente**, sem publicar seu conteúdo: bots,
   conversas, entradas, arquivos e data. Abrir amostras de memória/arquivos nessa
   máquina isolada. O script preserva `cwd`/caminhos originais no SQLite: em
   máquina nova, um operador deve ajustar os caminhos **nessa cópia** ou montar
   os diretórios recuperados nos caminhos esperados. Não iniciar antes disso;
   não há migração automática nem promessa de iniciar qualquer dump sem ajustes.
5. **Cofre:** com o Hub parado e o Secret Service do usuário de recuperação
   parado, copiar os arquivos de `cofre-novo/keyrings/` para o keyring **desse
   usuário isolado**, preservando 700 na pasta e 600 nos arquivos. Iniciar o
   Secret Service e desbloquear com a senha do item de recuperação usando sua
   interface local; não imprimir/exportar a chave-mestra. O SQLite preserva
   `vault.id` e `vault.initialized`; a entrada do serviço
   `app.donnabot.credentials` precisa corresponder ao mesmo ID. Não apagar esses
   campos nem gerar chave nova para tentar ler registros antigos. Em teste de
   recuperação autorizado, conferir um registro sentinela pela interface local,
   sem copiar seu valor ao chat/log. Se falhar, manter os arquivos e parar a
   ativação; recuperar o pacote/senha corretos. Não trocar o cofre do Hub vivo.
6. Refazer login dos motores — obrigatório, pois os perfis conhecidos ficam
   fora do pacote — e refazer os pareamentos, incluindo os números de
   WhatsApp enquanto a sessão `session.db` permanecer fora do backup por decisão
   provisória. Rever permissões e autorizar a promoção dessa cópia somente após
   as conferências. Remover arquivos privados temporários e credenciais R2 da
   máquina de teste. Registrar data, duração, idade do ponto restaurado e
   resultado, sem conteúdo privado.

Um job diário oferece ponto recuperável com até aproximadamente **24 h** de idade
(mais atrasos/falhas). **Um backup por dia é o RPO aceito para o beta — definição
do dono**, não uma limitação a resolver nesta entrega. Não cumpre sozinho o alvo
histórico de 15 min. RTO de 4 h e recuperação completa do cofre dependem de prova
real cronometrada após as conferências de ativação.

## Prova fictícia (Linux, usuário do Hub)

```sh
python3 /PACOTE/prove.py --root /PASTA/DE/TESTE/NOVA
```

Cria apenas fixtures, mantém um SQLite WAL aberto, faz backup/cifra/envio local,
baixa/decifra em pasta nova e confere as contagens e arquivos. Verifica exclusões,
preservação do último ponto, corrupção, chave errada, destino vivo/existente,
tar hostil, links de venv, execução/modos, pasta aninhada, anexos reservados,
editados/ausentes, mudança concorrente e o pacote separado do cofre. Corrompe
pacotes com cifra válida para exigir hash, integrity_check, contagens, conjunto
de arquivos, referências, modo, diretórios, links, tipo, token e formato;
executa **12 mutantes**, removendo cada proteção individualmente e
**exige que a própria prova falhe**. Testa SIGTERM real em subprocesso, status
com id/data do restore, atraso de 27 h e jobs independentes. Confere pastas
ancestrais simbólicas, restauração entre dois filesystems com pasta 0555 e
limpeza após falha, três tentativas após PUT/GET incerto com um só objeto,
validade sem fuso/múltiplos alertas e limites dos caminhos de uploads. Reproduz
backup/cifra/restauração de `credentials.json`, `.key`, `.pem` e `normal.txt` no
workspace atual, após trocar cwd e após apagar o bot com remoção das entradas;
confere também grupo, pasta própria, publicados e referências ausentes. Cobre
Spaces nos dois layouts (workspaces, uploads de conversa, arquivos publicados,
camadas de memória por `kv` e avatares gerados), o modo de motor separado com
`motor_work`, a manutenção de `host.lock` e as exclusões de credenciais/sessão.
Um mutante retira o workspace histórico e precisa falhar nesse cenário. Para executar só
esse diferencial, acrescente `--payload-history-only`. Usa
`/dev/shm` apenas como segundo filesystem fictício, com limpeza garantida. Cria Secret Service em uma
sessão D-Bus e HOME exclusivos de teste, guarda chave fictícia de 32 bytes,
recupera o pacote em outro HOME e confere a mesma chave pelo serviço/ID sem
exibir o valor. Usa `dbus-run-session`, `gnome-keyring-daemon` e `secret-tool` já
presentes. A interface curl é conferida sem rede. Remove a pasta e as chaves
fictícias ao terminar; recusa pasta existente. Não usa dados reais nem valida
o cofre do usuário ou o R2 real.

Fontes técnicas: [SQLite Online Backup](https://docs.python.org/3/library/sqlite3.html#sqlite3.Connection.backup),
[GCM](https://cryptography.io/en/latest/hazmat/primitives/symmetric-encryption/#cryptography.hazmat.primitives.ciphers.modes.GCM),
[RSA/OAEP](https://cryptography.io/en/latest/hazmat/primitives/asymmetric/rsa/#encryption),
[API S3 R2](https://developers.cloudflare.com/r2/api/s3/api/),
[permissões R2](https://developers.cloudflare.com/r2/api/tokens/) e
[lifecycle](https://developers.cloudflare.com/r2/buckets/object-lifecycles/),
[bucket locks](https://developers.cloudflare.com/r2/buckets/bucket-locks/) e
[jurisdição](https://developers.cloudflare.com/r2/reference/data-location/),
[preços](https://developers.cloudflare.com/r2/pricing/) e
[alertas de orçamento](https://developers.cloudflare.com/billing/manage/budget-alerts/).
Documentação R2 conferida em 22/10/2026, sem acessar conta/bucket real.

## D520 B0 — árvores privadas aninhadas

Em todo inventário, incluindo `shared-memory`, `skills` e `bots`, os novos
prefixos `.mexerica`, `.donnabot` e `.codync`, sem distinguir caixa, excluem somente diretórios
aninhados, incluindo Dev, sandboxes e backups com segredos. Arquivos regulares,
como `.donnabot-receipt.json` das skills, não são excluídos só pelo prefixo.
O tipo é o do próprio item, sem seguir symlinks; links conservam a política
anterior de guardar o texto, sem ler o destino. O tratamento de desaparecimento
de anexos (`payload_path`) permanece e isenta esses arquivos do filtro de nomes.
Raízes explícitas de perfis são recusadas; prefixos de
instalação como `.mexerica` continuam aceitos em raízes legítimas.
Backups frios continuam privados (0700/0600, sem ACL para o motor); o filtro
não substitui as permissões. Prova focal sintética na nuvem:
`python3 packaging/hub-backup/prove.py --root /tmp/d520-backup-fixture --private-roots-only`.
Não foi executada no Mac nesta entrega.


### D520-B1 — raiz atual e backup frio separado

A configuração própria passa a apontar para `.mexerica`; o default de proteção
do restore também passa a `~/.mexerica`. Confira `hub_home` explicitamente antes
de qualquer operação: o backup usa seu arquivo de configuração, não importa
variáveis antigas. Recibos e arquivos regulares mantêm a política B0.
O exportador filtrado não substitui backup frio integral. Mudança de raiz,
backup frio e movimento único manual pela Operação continuam **NÃO EXECUTADOS**.
Não use `/tmp` para dados ou backup de usuário.
