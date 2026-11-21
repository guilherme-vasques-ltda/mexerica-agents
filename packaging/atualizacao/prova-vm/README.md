# Prova ponta a ponta em VM descartável

Somente para VM descartável, como root. Cria a conta de teste `hublogin` com um
Hub **legado real** (`donnabot-host serve` como unidade `systemd --user`, linger),
CA e chaves **de teste** geradas na própria VM, repositório TUF local em
`https://releases.prova.test/repo/` (TLD reservado `.test`, via `/etc/hosts`) e o
pacote administrativo com `SHA256SUMS`. Nunca use chave real.

Pré-requisitos: Debian 13 com systemd, cgroup v2; `CARGO_TARGET_DIR` apontando
para um build com `donnabot-host`, `donnabot-motor-exec`,
`donnabot-atualizar-privilegiado`, `donnabot-guarda-atualizacao`,
`donnabot-instalar-atualizacao` e `donnabot-motor-copia`. O preparo também persiste
`fs.protected_hardlinks=1` em `/etc/sysctl.d` (configuração de servidor exigida
pelo instalador). O preparo instala `dbus`, `dbus-user-session`,
`systemd-timesyncd` e `python3-cryptography`.

O Hub legado da VM precisa ser **realmente anterior ao gate de partida**: o
`donnabot-host` legado compilado em pasta e alvo próprios, nunca o build atual
(que já tem o gate e mascararia o corte). O histórico público começa na
importação que já contém o gate, então não há revisão anterior para compilar
aqui: forneça em `LEGADO_ANTERIOR` um binário construído de uma revisão
anterior ao gate e confirme antes que ele desconhece o comando. O preparo
recusa um legado que conheça `coordenar-atualizacao`.

```sh
sudo LEGADO_ANTERIOR=/data/legado-anterior/donnabot-host bash preparar.sh
sudo bash cenarios.sh      # imprime OK/FALHA e "RESULTADO: N falha(s)" e falha se houver falhas
```

Antes da primeira migração, `cenarios.sh` e `reboot.sh instalar` (`preparatoria.sh`)
provam que o coordenador do build atual recusa migrar o legado anterior sem nenhum
efeito (serviço ativo no legado, sem backup, seletor nem drop-in) e fazem a
atualização preparatória: o legado passa a ser o build atual, com o gate. Só então a
migração e os cortes de energia seguem; durante toda a coordenação a unidade executa
um binário que tem o gate.

Provas de reboot: cada fase `-antes` dispara o reboot (ou o corte de energia) de forma
assíncrona e retorna; o shell morre no reboot. Por isso **não use laço**: rode uma fase
por vez, confira o status de saída de cada uma (`echo $?` deve ser 0) e, depois de cada
`-antes`, espere a VM voltar e abra uma sessão nova antes da `-depois`.

```sh
sudo bash limpar.sh; sudo CARGO_TARGET_DIR=/data/target LEGADO_ANTERIOR=/data/target-base/debug/donnabot-host bash preparar.sh
sudo bash reboot.sh instalar;  echo $?
# R4 e R5 fazem a 1ª migração de que R1-R3 dependem; a ordem é obrigatória.
sudo bash reboot.sh r4-antes;  echo $?   # corta a energia; espere o boot (boot_id novo)
sudo bash reboot.sh r4-depois; echo $?   # nova sessão
sudo bash reboot.sh r5-antes;  echo $?   # retorna logo; o vigia corta a energia após a F3
#   espere o corte e o boot; confira que o boot_id mudou antes de seguir
sudo bash reboot.sh r5-depois; echo $?
sudo bash reboot.sh r1-antes;  echo $?   # reboot ordenado
sudo bash reboot.sh r1-depois; echo $?
sudo bash reboot.sh r2-antes;  echo $?   # corte de energia
sudo bash reboot.sh r2-depois; echo $?
sudo bash reboot.sh r3-antes;  echo $?   # corta 3 s antes do prazo
#   R3 exige que o prazo vença com a máquina DESLIGADA: só ligue a VM depois do prazo
#   impresso pela fase (a -depois confere corte < prazo < início do boot)
sudo bash reboot.sh r3-depois; echo $?
sudo bash limpar.sh        # desfaz tudo (somente na VM de teste)
```

Cenários: recusas do instalador sem alterar nada (fingerprints, login, estado
misto, UID do Hub); instalação administrativa real e reexecução idempotente;
wrapper recusando a troca com o Hub rodando; coordenador na primeira migração com
aceite; volta forçada completa (DB, drop-ins, `--reverter`); reciclagem de slot;
gerenciador `systemd --user` intacto; rotação da raiz TUF (só quórum novo recusada,
dois quóruns aceita com nova chave de timestamp); expiração sem aceite parada pelo
guard com bloqueio durável; e a análise do `amostrador.py` (50 ms), que falha se o
Hub aparecer rodando em fase crítica do journal ou numa troca de seletor.

Na fase 3, os cenários também cobrem:
- a F3 pelo canal do motor:
  - um perfil de conta injetado é copiado como motor (0700/0600, bit de execução
    preservado);
  - uma skill com script 0755 chega como 0640 sem divergência;
  - as referências no DB são reescritas com a marca `d330`;
  - as raízes de trabalho ficam com o Hub e o grupo `donnabot-trabalho`;
- o isolamento do canal:
  - socket 0660 com o grupo do Hub, e `nobody` não conecta;
  - root é recusado por `SO_PEERCRED`, e o Hub fora da janela da primeira migração
    também é recusado (`canal.py`);
  - o perfil injetado usa o layout real (home `<uuid>` com `.donnabot-profile`), a
    única raiz de home que o canal aceita;
  - o registro de raízes tem só as raízes copiadas;
- a revogação administrativa:
  - login de outro serviço, regra `ALL` e alias com curinga são recusados (com
    auditoria de falha);
  - a revogação espera a trava do wrapper em uso e encerra uma chamada do wrapper
    suspensa antes da trava (parada no exec por ptrace, `pausado.py`);
  - sudoers, capabilities, unidades e estado arquivado são conferidos; conta e
    dados do motor são preservados; a reexecução é idempotente.

`reboot.sh` prova, nesta ordem:
- R4: queda de energia (sysrq) no meio da cópia F3 da primeira migração. No boot, o
  Hub recusa iniciar (coordenação interrompida) e agenda a retomada automática, que
  conclui pela volta preservando o DB; o Hub só entra em execução depois dela;
- R5: nova tentativa da primeira migração com queda de energia logo depois da F3
  confirmada no DB. Um vigia na VM dispara o sysrq quando o coordenador informa a F3
  concluída. Na volta, as cópias estão duráveis, completas e idênticas à origem, e o
  estado é coerente com o ponto do corte:
  - unidade ainda não apontada: nenhum par inicia antes da retomada automática, que
    volta restaurando o DB; nova tentativa reaproveita as cópias idênticas;
  - unidade já apontada: janela pendente e aceite;
- R1: reboot ordenado com a janela pendente;
- R2: queda de energia com o journal em fase intermediária;
- R3: a janela vencendo com a máquina desligada (corte < prazo < início do boot).

O roteiro sai com status 1 se houver falha.

Instalação limpa (`instalar --hub-novo`): prova a escrever com um login sem Hub legado
(sem unidade, drop-ins nem `~/.local/bin/donnabot-host`); não está nestes roteiros. O
`donnabot-host` da release publicada precisa ser de produção
(`CODYNC_ENV=main cargo build --bin donnabot-host`): um build dev é recusado antes de
criar a unidade, e a primeira publicação é revertida.

## Prova obrigatória: queda durante a reversão de uma atualização gerida

Esta prova é **manual, somente em VM descartável com root e systemd**; os comandos
`reboot.sh` acima ainda não automatizam estes quatro cortes. Execute cada caso numa
instalação gerida saudável, com duas releases de teste distintas, guard ativo,
serviço habilitado e o prazo da janela ainda válido depois do boot. Guarde o ID da
transação, os slots anterior e novo, o prazo e o `boot_id` antes de cada corte. Use
uma nova atualização para cada caso e preserve os logs da VM; não use dados reais.

Para atingir os pontos com precisão, use na VM um build de teste com símbolos e um
checkpoint externo (por exemplo, depurador que acompanhe o processo privilegiado),
sem alterar journal, permissões ou código instalado durante a prova. Suspenda o
wrapper exatamente nos locais abaixo de `host/src/atualizacao/transacao.rs`, dentro
de `executar_reversao`, depois de iniciar a volta pelo coordenador com
`--forcar-reversao --prova`. Os nomes de `ponto(...)` identificam a posição no
código; eles são ganchos de teste unitário e **não provocam a queda no build normal**.
Enquanto o wrapper estiver suspenso, confira a etapa durável `reversao_iniciada`,
o seletor e os modos dos dois slots; capture essa evidência e corte a energia da VM
sem sync global. Inicie a VM antes de vencer o prazo. Se não for possível atingir e
comprovar exatamente um checkpoint, marque o caso como não provado.

| Caso | Checkpoint após a operação indicada | Seletor no corte | Par anterior | Par novo |
| --- | --- | --- | --- | --- |
| B1 | `reversao_iniciada`: journal e status publicados, antes de armar | novo | fechado | armado |
| B2 | `anterior_armado`: par anterior armado, antes de `apontar` | novo | armado | armado |
| B3 | `seletor_revertido`: seletor trocado, antes de `desativar` | anterior | armado | armado |
| B4 | `novo_fechado`: par novo fechado, antes de gravar `seletor_restaurado` | anterior | armado | fechado |

Depois de **cada** boot, registre o `boot_id` diferente e confira, pelos logs e pelo
estado real (inclusive `ExecStartPre` da unidade instalada), que o guard permitiu a
partida dentro do prazo, o Hub executou o gate real, recusou iniciar a aplicação por
coordenação interrompida e agendou a unidade transitória de retomada. Procure a
mensagem `coordenação da atualização interrompida` no journal do usuário do Hub e
verifique também o resultado da unidade `donnabot-retomar-atualizacao`; ausência de
evidência do gate ou da retomada é falha. Sem chamar `--reverter` manualmente,
confira que a retomada automática concluiu a reversão: journal em `concluida` e
fase `revertida`, seletor no slot anterior, slot novo fechado, registro de
verificação removido, dados preservados e serviço anterior saudável. Confirme pela
ordem dos eventos que o Hub só iniciou normalmente depois da retomada. Se o prazo
venceu antes do boot, o cenário é inválido para esta prova: repita-o com prazo válido.
A expiração desligada é outro cenário, R3.

## Prova obrigatória: protocolo do wrapper e versões

Em uma VM preparada, substitua **somente para esta prova** o wrapper instalado por
uma cópia root-owned de uma versão anterior ao protocolo, com modo e caminho válidos.
Antes de iniciar a atualização, registre hash do journal, seletor, unidade e estado
do serviço. Confirme que o coordenador recusa o wrapper antigo antes de parar ou
alterar o serviço, que esses estados permanecem iguais e que não houve aceite.
Restaure o wrapper atual pela etapa administrativa e confira seu protocolo; nunca
considere o aceite de um wrapper antigo como prova do marcador novo.

Para cada atualização ou instalação limpa testada, observe a ordem real dos eventos:
serviço anterior parado; porta configurada sem listener; caminho efetivo de
`ExecStart` e `--version` do binário novo iguais à versão do manifesto autenticado;
só então o start; em seguida, `version` do hello igual à versão esperada. Registre as
versões e o resultado sem expor token ou corpo da resposta. Injete separadamente porta
ocupada, versão do binário divergente e versão do hello divergente: cada tentativa
deve falhar e reverter, sem aceite. Com transação pendente, confirme também que
`sudo -n .../atualizar-privilegiado --aceitar` sem registro válido é recusado; teste
registro ausente, adulterado, dono incorreto e modo incorreto em cópias independentes
da VM. A aceitação só pode ocorrer após as conferências e o registro válido do Hub.
Em `reboot.sh`, os cenários já refletem o protocolo novo. R5-depois (corte na
janela antes do registro) confirma que o aceite direto sem registro é recusado e
conclui por `aceite_fluxo`, o helper do roteiro que emula as duas chamadas do
fluxo do coordenador: a primeira lê o journal como root e grava o registro de
verificação (porta de prova fixa 7777, sem as conferências de porta, `--version`
e hello do coordenador real) e a segunda valida o registro e aceita. R1-depois é
diferente: o `coord --sem-aceite --prova` de R1-antes já gravou o registro
(`registrar_verificacao` roda antes de o coordenador olhar `--sem-aceite`) e ele
é durável através do reboot, então o aceite direto é aceito sem regravar.
