#!/bin/bash
# Prova de reboot real (VM descartável, como root). Uso, em ordem, com o reboot feito
# pelo próprio script nas fases "antes" e a verificação na volta ("depois"):
#   instalar | r4-antes | r4-depois | r5-antes | r5-depois | r1-antes | r1-depois
#   | r2-antes | r2-depois | r3-antes | r3-depois
# R4 corta a energia no meio da cópia F3 da 1ª migração; R5 corta logo depois da F3
# confirmada no DB; R1 reinicia com a janela pendente; R2 corta a energia com a transação
# em fase intermediária; R3 deixa a janela vencer desligada. As sequências são as
# seguintes à última do journal (o caminho de recuperação do R5 pode usar uma a mais).
# Sai com status 1 se qualquer verificação falhar.
set -uo pipefail
D=$(dirname "$(readlink -f "$0")"); S=/srv/prova; E=/var/lib/donnabot/atualizacao; R=/opt/donnabot/releases
W=/opt/donnabot/sbin/atualizar-privilegiado; H=/home/hublogin; U=$(id -u hublogin); I=$S/pacote/donnabot-instalar-atualizacao  # vazamento-ok: conta de teste criada na VM descartável
ACC=ac_$(printf 'e%.0s' $(seq 64)); HOMEID=7a6b5c4d-3e2f-4a1b-9c8d-7e6f5a4b3c2d
PERFIL=$H/dados/engine-setup/accounts/$ACC/$HOMEID/.claude  # vazamento-ok: conta de teste criada na VM descartável
CONTAS=/var/lib/donnabot/motor/engine-setup/accounts
FALHAS=0
como_hub() { runuser -u hublogin -- env XDG_RUNTIME_DIR=/run/user/$U DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$U/bus MEXERICA_HOME=$H/dados "$@"; }
coord() { como_hub $H/.local/bin/donnabot-host coordenar-atualizacao "$@"; }
# Fluxo do coordenador em duas chamadas: a primeira grava o registro root-owned de
# partida (pedido por pipe), a segunda o valida e conclui o aceite. Sem o registro,
# o aceite direto é recusado — inclusive para root.
aceite_fluxo() {
  python3 -c 'import json
j=json.load(open("'$E'/journal"))
m=json.load(open("'$E'/transacoes/"+j["transacao"]+"/manifesto"))
print(json.dumps({"transacao":j["transacao"],"hub_sha256":m["hub"]["sha256"],"versao":m["versao"],"porta":7777}))' > $S/pedido.json
  cat $S/pedido.json | espera 0 "registro da verificação de partida" como_hub sudo -n $W --aceitar
  espera 0 "aceite com o registro válido" como_hub sudo -n $W --aceitar </dev/null
}
servico() { como_hub systemctl --user is-active donnabot-host.service 2>/dev/null; }
exe_hub() { local p; p=$(como_hub systemctl --user show -p MainPID --value donnabot-host.service); [ "$p" -gt 0 ] 2>/dev/null && readlink /proc/$p/exe || echo "-"; }
confere() { if eval "$2"; then echo "  OK   $1"; else echo "  FALHA $1"; FALHAS=$((FALHAS+1)); fi; }
espera() { local esperado=$1 desc=$2; shift 2; "$@" > /tmp/saida.txt 2>&1; local rc=$?
  if [ "$rc" = "$esperado" ]; then echo "  OK   rc=$rc $desc"; else echo "  FALHA rc=$rc (esperado $esperado) $desc"; FALHAS=$((FALHAS+1)); tail -6 /tmp/saida.txt | sed 's/^/    | /'; fi; }
campo() { python3 -c "import json;j=json.load(open('$E/journal'));print($1)"; }
db() { como_hub python3 -c "import sqlite3;print(sqlite3.connect('$H/dados/codync.db').execute(\"$1\").fetchone())"; }
boot() { cat /proc/sys/kernel/random/boot_id; }
inicio_do_boot() { echo $(( $(date +%s) - $(cut -d. -f1 /proc/uptime) )); }
publicar() { python3 $D/repo.py publicar $S "$1" 1 $S/art/$2 $S/art/aj >/dev/null && chmod -R a+rX $S/www; }
proxima() { echo $(( $(campo 'j["sequencia_nova"]') + 1 )); }
publicar_n() { publicar "$1" hub$(( $1 % 2 == 0 ? 2 : 3 )); }
backup_atual() { ls -td $H/dados/atualizacao-privilegiada/*/ | head -1; }
# Suave: reboot ordenado. Abrupto: sysrq sem sync global (só o registro da prova é
# gravado em disco), para não mascarar dados que uma queda de energia perderia.
reiniciar() { echo "  boot antes: $(boot)"; boot > $S/boot-antes; date +%s > $S/corte; sync $S/boot-antes $S/corte $S/slot-* $S/prazo-* $S/seq-* $S 2>/dev/null
  if [ "$1" = suave ]; then (sleep 2; systemctl reboot) >/dev/null 2>&1 &
  else echo 1 > /proc/sys/kernel/sysrq; (sleep 1; echo b > /proc/sysrq-trigger) >/dev/null 2>&1 & fi; echo "  reboot $1 solicitado"; }
depois_comum() {
  confere "máquina reiniciou (boot_id novo)" '[ "$(boot)" != "$(cat $S/boot-antes)" ]'
  confere "cgroup root recriado no boot com os limites" '[ "$(cat /sys/fs/cgroup/donnabot-atualizacao/memory.max)" = 1073741824 ]'
  confere "guard e canal do motor ativos após o boot" 'systemctl is-active -q donnabot-guarda-atualizacao@$U && systemctl is-active -q donnabot-motor-copia.socket'
  [ -f $S/servidor.pid ] && kill -0 "$(cat $S/servidor.pid)" 2>/dev/null || { nohup python3 $D/servidor.py > $S/servidor.log 2>&1 < /dev/null & echo $! > $S/servidor.pid; sleep 1; }
}
case "${1:-}" in
instalar)
  espera 0 "etapa administrativa" $I instalar --login hublogin --pacote $S/pacote --fingerprint-pacote $(cat $S/fp-pacote) --fingerprint-raiz $(cat $S/fp-raiz)
  confere "Hub legado ativo, sem par gerido" '[ "$(servico)" = active ] && [ ! -e $R/ativo ]'
  . $D/preparatoria.sh
  preparatoria ;;
r4-antes)
  echo "=== R4: queda de energia no meio da cópia F3 da 1ª migração (seq 1)"
  como_hub systemctl --user stop donnabot-host.service
  # Perfil grande (2 GiB) para a cópia pelo canal durar segundos; skill e conta no DB.
  como_hub sh -c "mkdir -p $PERFIL && touch $(dirname $PERFIL)/.donnabot-profile && chmod 600 $(dirname $PERFIL)/.donnabot-profile && printf '{\"tema\":\"r4\"}' > $PERFIL/settings.json && head -c 2G /dev/zero > $PERFIL/grande.bin"
  como_hub sh -c "mkdir -p $H/dados/skills/r4 && printf '#!/bin/sh\necho r4\n' > $H/dados/skills/r4/run.sh && chmod 755 $H/dados/skills/r4/run.sh"
  como_hub python3 - <<PY
import json, sqlite3, uuid
c = sqlite3.connect("$H/dados/codync.db")
conta = {"id": "$ACC", "backend": "claude", "billing": "subscription", "state": "connected",
         "launch": {"kind": "cli_profile", "executable": "/bin/true", "directory": "$PERFIL"},
         "scope": "owner", "quotaIdentity": "$ACC", "verifiedAt": 1,
         "subscriptionProof": {"method": "claude_auth_status", "plan": "max"}, "models": [{"id": "haiku", "options": []}]}
c.execute("insert or replace into kv values('fork.engine-accounts.v1', ?)", (json.dumps({"salt": str(uuid.uuid4()), "accounts": [conta]}),))
c.commit()
PY
  como_hub systemctl --user start donnabot-host.service; sleep 3
  (coord > $S/r4-coord.log 2>&1 &)
  for i in $(seq 1 600); do
    G=$(find $CONTAS/$ACC -path '*/.copia-*/*/grande.bin' 2>/dev/null | head -1)
    [ -n "$G" ] && [ "$(stat -c %s "$G")" -gt 268435456 ] && break; sleep 0.2
  done
  echo "  staging no corte: ${G:-nenhum} ($(stat -c %s "${G:-/dev/null}" 2>/dev/null) bytes); status: $(cat $R/status.json)"
  confere "corte durante a cópia do perfil, com a 1ª migração publicada e pendente" '[ -n "$G" ] && grep -q pendente $R/status.json && [ ! -e $CONTAS/$ACC/$HOMEID ]'
  reiniciar abrupto ;;
r4-depois)
  depois_comum
  # O Hub não grava antes da retomada: o gate recusa a partida e agenda a retomada
  # automática (unidade transitória donnabot-retomar-atualizacao), que conclui pela volta.
  for i in $(seq 1 90); do ! grep -q pendente $R/status.json && [ "$(servico)" = active ] && break; sleep 1; done
  confere "sem estado misto: perfil não publicado, F3 não confirmada no DB, unidade não aponta para o par gerido" '[ ! -e $CONTAS/$ACC/$HOMEID ] && [ "$(db "select count(*) from kv where k=\x27d330.raizes.v1\x27")" = "(0,)" ] && [ ! -e $H/.config/systemd/user/donnabot-host.service.d/zz-donnabot-atualizacao.conf ]'
  confere "Hub recusou iniciar antes da retomada (gate) e agendou a retomada" 'journalctl -b _UID=$U --no-pager | grep -q "coordenação da atualização interrompida"'
  confere "retomada automática concluiu pela volta: legado ativo, janela fechada, journal revertido" '[ -e "$(backup_atual)retomada" ] && [ "$(servico)" = active ] && [ "$(exe_hub)" = $H/.local/bin/donnabot-host ] && ! grep -q pendente $R/status.json && [ "$(campo "j[\"estado\"][\"fase\"][\"fase\"]")" = revertida ]'
  confere "Hub só entrou em execução depois da retomada (nenhuma gravação antes dela)" '[ "$(stat -c %Y "$(backup_atual)retomada")" -le "$(date -d "$(como_hub systemctl --user show -p ExecMainStartTimestamp --value donnabot-host.service)" +%s)" ]'
  como_hub python3 -c "import sqlite3;c=sqlite3.connect('$H/dados/codync.db');c.execute(\"insert or replace into kv values('r4_pos_retomada','legado')\");c.commit()"
  confere "DB preservado e gravável no legado depois da retomada" 'db "select v from kv where k=\x27r4_pos_retomada\x27" | grep -q legado'
  rm -f $PERFIL/grande.bin ;;
r5-antes)
  N=$(proxima); echo "=== R5: queda de energia logo depois da F3 confirmada no DB (seq $N)"
  publicar_n $N
  boot > $S/boot-antes; sync $S/boot-antes $S
  echo 1 > /proc/sys/kernel/sysrq
  # Vigia independente desta chamada: corta a energia assim que o coordenador informa a
  # F3 concluída (cópias publicadas e DB confirmado), sem sync global.
  setsid nohup bash -c "until grep -q 'F3 concluída' $S/r5-coord.log 2>/dev/null; do sleep 0.02; done; date +%s > $S/corte; sync $S/corte; echo b > /proc/sysrq-trigger" >/dev/null 2>&1 < /dev/null &
  (coord > $S/r5-coord.log 2>&1 &)
  echo "  coordenador iniciado; corte automático após a F3" ;;
r5-depois)
  depois_comum
  confere "o corte ocorreu depois da F3 concluída (vigia disparou)" '[ -s $S/corte ] && [ "$(cat $S/corte)" -gt 0 ]'
  for i in $(seq 1 30); do [ "$(servico)" = active ] && break; sleep 1; done
  M=$CONTAS/$ACC/$HOMEID/.claude
  confere "cópias da F3 duráveis: perfil e skill publicados, completos e idênticos à origem" '[ -z "$(find $CONTAS/$ACC /var/lib/donnabot/trabalho -name ".copia-*")" ] && cmp -s $M/settings.json $PERFIL/settings.json && [ "$(find $M | wc -l)" = "$(find $PERFIL | wc -l)" ] && cmp -s /var/lib/donnabot/trabalho/skills/r4/run.sh $H/dados/skills/r4/run.sh && grep -qx "$ACC/$HOMEID" $CONTAS/.donnabot-raizes'
  if [ -e "$(backup_atual)apontado" ]; then
    echo "  caminho: a unidade já apontava para o par novo antes do corte (janela pendente)"
    confere "estado coerente: F3 confirmada no DB e Hub de volta no par pendente" '[ "$(db "select count(*) from kv where k=\x27d330.raizes.v1\x27")" = "(1,)" ] && grep -q pendente $R/status.json && [ "$(exe_hub)" = $R/$(readlink $R/ativo)/donnabot-host ]'
    espera 1 "aceite direto sem registro é recusado" como_hub sudo -n $W --aceitar </dev/null
    aceite_fluxo
  else
    DI=$H/.config/systemd/user/donnabot-host.service.d/zz-donnabot-atualizacao.conf
    echo "  caminho: corte entre a F3 e a marca de apontamento (drop-in: $([ -e $DI ] && echo gravado || echo ausente))"
    # Nenhum par grava no DB migrado antes da retomada: o gate recusa a partida e a
    # retomada automática restaura o DB do backup (F3 confirmada) e volta ao legado.
    for i in $(seq 1 90); do ! grep -q pendente $R/status.json && [ "$(servico)" = active ] && break; sleep 1; done
    confere "Hub recusou iniciar antes da retomada (gate)" 'journalctl -b _UID=$U --no-pager | grep -q "coordenação da atualização interrompida"'
    confere "retomada automática: DB restaurado do backup (sem a F3), drop-in removido e legado ativo" '[ -e "$(backup_atual)retomada" ] && [ "$(db "select count(*) from kv where k=\x27d330.raizes.v1\x27")" = "(0,)" ] && [ ! -e $DI ] && [ "$(exe_hub)" = $H/.local/bin/donnabot-host ]'
    confere "Hub só entrou em execução depois da retomada" '[ "$(stat -c %Y "$(backup_atual)retomada")" -le "$(date -d "$(como_hub systemctl --user show -p ExecMainStartTimestamp --value donnabot-host.service)" +%s)" ]'
    N=$(proxima); publicar_n $N
    espera 0 "nova tentativa (seq $N) reaproveita as cópias idênticas e conclui" coord
    sed 's/^/    | /' /tmp/saida.txt | grep -E "perfil|F3|aceita"
  fi
  confere "1ª migração aceita no par gerido e F3 confirmada" '[ "$(campo "j[\"estado\"][\"fase\"][\"fase\"]")" = aceita ] && [ "$(db "select count(*) from kv where k=\x27d330.raizes.v1\x27")" = "(1,)" ] && [ "$(exe_hub)" = $R/$(readlink $R/ativo)/donnabot-host ]' ;;
r1-antes)
  N=$(proxima); echo "=== R1: reboot ordenado com janela pendente (seq $N)"
  publicar_n $N
  espera 0 "coordenar --sem-aceite --prova" coord --sem-aceite --prova
  confere "janela pendente com o Hub ativo no par novo" 'grep -q pendente $R/status.json && [ "$(exe_hub)" = $R/$(readlink $R/ativo)/donnabot-host ]'
  readlink $R/ativo > $S/slot-r1
  echo "  prazo restante: $(( $(campo 'j["estado"]["fase"]["prazo"]') - $(date +%s) )) s"
  reiniciar suave ;;
r1-depois)
  depois_comum
  confere "janela continua pendente, sem bloqueio nem emergência (guard reconheceu o desligamento)" '[ "$(campo "j[\"etapa\"]")" = pendente ] && [ ! -e $E/bloqueio ] && [ ! -e $R/emergencia ] && grep -q pendente $R/status.json'
  for i in $(seq 1 30); do [ "$(servico)" = active ] && break; sleep 1; done
  confere "Hub voltou sozinho no par pendente" '[ "$(servico)" = active ] && [ "$(exe_hub)" = $R/$(cat $S/slot-r1)/donnabot-host ]'
  # O coordenador do r1-antes já gravou o verificacao-partida (registrar_verificacao roda
  # antes de olhar --sem-aceite) e o registro é durável através do reboot: o aceite direto
  # é aceito, sem precisar regravar.
  espera 0 "aceite direto com o registro durável de antes do reboot" como_hub sudo -n $W --aceitar </dev/null
  confere "journal aceito" '[ "$(campo "j[\"estado\"][\"fase\"][\"fase\"]")" = aceita ]' ;;
r2-antes)
  N=$(proxima); echo "=== R2: queda de energia no meio da transação (seq $N)"
  readlink $R/ativo > $S/slot-r2; echo $N > $S/seq-r2
  publicar_n $N; touch $S/atraso
  (coord > $S/r2-coord.log 2>&1 &)
  for i in $(seq 1 120); do [ "$(campo 'j["etapa"]' 2>/dev/null)" = preparada ] && [ "$(campo 'j["sequencia_nova"]' 2>/dev/null)" = $N ] && break; sleep 0.5; done
  echo "  journal no corte: $(campo 'j["etapa"], j["sequencia_nova"]')  seletor: $(readlink $R/ativo)"
  confere "corte com a transação em fase intermediária e o Hub parado" '[ "$(campo "j[\"etapa\"]")" = preparada ] && [ "$(servico)" != active ]'
  reiniciar abrupto ;;
r2-depois)
  depois_comum
  rm -f $S/atraso
  sleep 10
  confere "sem estado misto: seletor no par anterior, slot novo inacessível, status bloqueado" 'A=$(cat $S/slot-r2); [ "$(readlink $R/ativo)" = $A ] && N=$([ $A = slot-a ] && echo slot-b || echo slot-a) && [ "$(stat -c %a $R/$N 2>/dev/null || echo ausente)" != 755 ] && grep -q bloqueado $R/status.json'
  confere "fase recuperável não virou bloqueio durável" '[ ! -e $E/bloqueio ] && [ ! -e $R/emergencia ]'
  confere "Hub não roda par algum durante a fase intermediária" '[ "$(servico)" != active ]'
  espera 0 "nova chamada retoma pelo journal e conclui a atualização" coord
  confere "seq $(cat $S/seq-r2) aceita e Hub ativo no par novo" '[ "$(exe_hub)" = $R/$(readlink $R/ativo)/donnabot-host ] && [ "$(readlink $R/ativo)" != "$(cat $S/slot-r2)" ] && [ "$(campo "j[\"sequencia_nova\"], j[\"estado\"][\"fase\"][\"fase\"]")" = "$(cat $S/seq-r2) aceita" ]'
  confere "auditoria registra a recuperação" 'grep -q "\"operacao\":\"recuperar\"" $E/auditoria' ;;
r3-antes)
  N=$(proxima); echo "=== R3: janela vence com a máquina desligada (seq $N)"
  publicar_n $N
  espera 0 "coordenar --sem-aceite --prova" coord --sem-aceite --prova
  P=$(campo 'j["estado"]["fase"]["prazo"]'); echo $P > $S/prazo-r3; echo "  prazo em $((P - $(date +%s))) s; corte 3 s antes"
  while [ "$(date +%s)" -lt $((P - 3)) ]; do sleep 0.2; done
  reiniciar abrupto ;;
r3-depois)
  depois_comum
  P=$(cat $S/prazo-r3); B=$(inicio_do_boot); C=$(cat $S/corte)
  echo "  corte=$C prazo=$P início do boot=$B"
  confere "prazo venceu com a máquina desligada (corte < prazo < início do boot)" '[ "$C" -lt "$P" ] && [ "$P" -lt "$B" ]'
  sleep 8
  confere "guard expirou a janela no boot: bloqueio durável e status bloqueado" '[ -e $E/bloqueio ] && [ "$(campo "j[\"estado\"][\"fase\"][\"fase\"]")" = expirada ] && grep -q bloqueado $R/status.json'
  confere "Hub parado e impedido de iniciar" '[ "$(servico)" != active ]'
  espera 1 "wrapper recusa operações após a expiração" como_hub sudo -n $W --aceitar </dev/null;;
*) echo "fase desconhecida"; exit 2 ;;
esac
echo "RESULTADO-FASE: $FALHAS falha(s)"
[ "$FALHAS" = 0 ]
