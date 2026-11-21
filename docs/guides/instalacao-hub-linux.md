# Instalar o Hub Linux da Mexerica

O Hub fica no usuário de login; motores rodam na conta de sistema
`donnabot-motor`. A etapa administrativa pede `sudo`. Não há modo antigo de
instalação nem dependência de Python, Node ou shell para executar essa etapa.
Os dois binários devem vir do mesmo pacote conferido: `donnabot-host` compilado
para `main` e `donnabot-motor-exec`. Este guia não publica nem baixa releases.

## Requisitos

Linux com systemd ativo e uma sessão de usuário com `systemctl --user` disponível;
`sudo`, `getent`, `useradd`, `usermod`, `userdel`, `groupadd`, `runuser`,
`nologin`, `setcap` e `getcap` nos diretórios de sistema. Em Debian/Ubuntu os
utilitários de capacidades vêm de `libcap2-bin`; HTTPS do Hub exige
`ca-certificates`. A instalação verifica os requisitos com uma mensagem de erro.
`/opt` precisa aceitar `security.capability` e executar capacidades de arquivo
(sem `nosuid`). O instalador confere `getcap` e executa o ajudante como Hub para
provar que a troca funciona. Landlock é consultado; ABI inferior a 3 fica
registrada, sem recusar a instalação.

## Instalação nova e reinstalação

Execute numa sessão do usuário que será dono do Hub, nunca numa sessão root:

```sh
./donnabot-host install --ajudante /caminho/conferido/donnabot-motor-exec
sudo loginctl enable-linger "$USER"
~/.local/bin/donnabot-host status
systemctl --user cat donnabot-host.service
getcap /opt/donnabot/bin/donnabot-motor-exec
```

O instalador cria o grupo `donnabot-trabalho`, a conta de sistema do motor com
shell `nologin` e home `/var/lib/donnabot/motor` (0700), e a raiz
`/var/lib/donnabot/trabalho` (Hub:grupo de trabalho, 2770).
`/etc/donnabot/motor.conf` é root 0644 e contém somente `motor_user`, `hub_uid` e
`work_gid`. O ajudante é root:grupo primário do Hub 0750, com
`cap_setuid,cap_setgid+ep`. O Hub não recebe capacidades.

O binário vai para `/opt/donnabot/bin/donnabot-host`, gravável pelo Hub para a
atualização, e o link `~/.local/bin/donnabot-host` conserva o caminho procurado
pelo app por SSH. A unidade **de usuário** usa `UMask=0077`, chama `cleanup` no
ajudante em `ExecStartPre` e `ExecStopPost`, e mantém o D-Bus da sessão para o
cofre. Não usa `NoNewPrivileges`, que impediria as capacidades do ajudante.

Reinstalar consulta `getent`, confere a conta existente e reaplica dono, modo e
capacidades. Não recria nem apaga dados. Conta incompatível, link para outro
programa ou configuração de outro Hub são recusados. O grupo novo requer uma
nova sessão para comandos interativos. Uma instalação antiga com binário regular no
caminho estável deve passar pela migração de teste abaixo.

## Desinstalação

```sh
~/.local/bin/donnabot-host uninstall
# Só quando a remoção da conta for desejada explicitamente:
~/.local/bin/donnabot-host uninstall --remover-usuario-motor
```

Remove unidade, ajudante e configuração. Mantém dados, binário do Hub, link e
usuários por padrão. A opção explícita remove só a conta, sem `userdel --remove`;
a home e os demais dados continuam no disco.

## Migração da instalação de teste do dono

É um comando administrativo, sem RPC ou ferramenta oferecida ao bot. Nunca copiar
perfil pessoal pela mera presença na home.

```sh
./donnabot-host migrar-motor conferir
./donnabot-host migrar-motor aplicar --instalacao-de-teste \
  --ajudante /caminho/conferido/donnabot-motor-exec \
  --backup /var/backups/donnabot-teste-IDENTIFICADOR
```

Antes: registrar versões, kernel, disco, linger e `/proc`; avisar o dono e gravar
uma sentinela fictícia no cofre. O comando exige o Hub respondendo com
`busy:false`, confere seu binário com o link do app e consulta o inventário
somente leitura `migrar-raizes --data-dir ... --inventario`.

Depois para o serviço e mantém `host.lock` exclusivo. Salva `data_dir`, binário,
unidade e todas as origens inventariadas em pasta do Hub 0700 em `/var/backups`,
fora da home e das raízes do motor/trabalho, sem ACL; arquivos 0600, com manifesto
de hashes SHA256. Recusa links e arquivos especiais, sem seguir suas árvores.
Só o socket transitório exatamente em `data_dir/screen.sock`, confirmado sem
seguir links, fica fora do hash e da cópia e não é exigido na volta. Outros
sockets e arquivos especiais continuam recusados, inclusive um link com esse nome.
Copia **sempre** os perfis já referidos pela Mexerica, inclusive `.claude.json`
quando inventariado; preserva os originais. Destino preexistente é recusado para
não sobrescrever dados. Copia workspace, anexos e skills e confere conteúdo e contagem. Scripts executáveis pelo dono também ficam
executáveis pelo grupo de trabalho, para o motor poder executá-los. Perfis de origem compartilhada conservam
destinos próprios por conta. A cópia do perfil não move executáveis privados
antigos do Hub: ela os recusa; prepare novamente o conector pelo app para
publicar uma instalação como motor. A migração reescreve referências numa transação por
`migrar-raizes --data-dir ...` e não transforma SQL nem JSON persistido.
O instalador mantém a `host.lock` durante a operação e a transmite à migração.
Na chamada direta, pare o Hub: se a trava estiver ocupada por mais de um segundo,
o comando recusa sem abrir o banco para escrita.

Se falhar antes de criar o manifesto, os dados, o binário e a unidade antigos
não são trocados; o instalador religa o serviço antigo ou informa o comando
para fazê-lo. A volta só é indicada quando há manifesto.

O comando sobe o serviço e mostra o checklist, mas não declara aceite. Conferir
em até **30 minutos**: app por SSH; bot respondendo; conta migrada usando CLI;
login novo pelo app; sentinela do cofre igual, inclusive após reiniciar sem SSH;
conector sem bot; anexo lido; UID dos motores/pontes/sondas e Hub sem capacidades;
negativos de token, DB, binário, `send_file` e autoridade da ponte. Se a CLI
recusar o perfil copiado, fazer login pelo app, sem pedir credencial no chat.

**Voltar** se o Hub não subir, o bot não responder após refazer login, a sentinela
falhar ou qualquer negativo falhar:

```sh
./donnabot-host migrar-motor voltar --instalacao-de-teste \
  --backup /var/backups/donnabot-teste-IDENTIFICADOR
```

A volta verifica os hashes antes de parar, guarda o estado pós-troca numa pasta
irmã do `data_dir`, conservando no caminho ativo o diretório e o mesmo inode de
`host.lock` exclusivo durante toda a restauração. O arquivo de lock do backup
não substitui o ativo; os demais dados, unidade e binário são restaurados e os
hashes conferidos antes de subir o
serviço antigo. As mensagens posteriores à troca ficam nessa pasta e não entram
no DB restaurado. A conta e o ajudante permanecem inertes, com a configuração
retirada do caminho ativo. Repetir a sentinela do cofre e confirmar um bot. Após
aceite, manter o backup por sete dias; nenhuma limpeza automática apaga a cópia.

Se a configuração e o backup estiverem em sistemas de arquivos distintos, a
desativação usa cópia sem seguir links, sincronização, troca atômica no destino
e só depois remove a configuração ativa, preservando dono e modo.
