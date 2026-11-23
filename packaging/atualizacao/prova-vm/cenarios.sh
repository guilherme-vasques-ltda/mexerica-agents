#!/bin/bash
# Cenários reais, como root na VM descartável: instalador administrativo, coordenador do Hub,
# guard instalado, amostragem de serviço parado e rotação da raiz TUF.
set -uo pipefail
[ "${PROVA_VM:-}" = 1 ] || { echo 'defina PROVA_VM=1: confirme que está numa VM descartável antes de executar' >&2; exit 1; }
D=$(dirname "$(readlink -f "$0")"); S=/srv/prova; E=/var/lib/donnabot/atualizacao; R=/opt/donnabot/releases
W=/opt/donnabot/sbin/atualizar-privilegiado; H=/home/hublogin; U=$(id -u hublogin)  # vazamento-ok: conta de teste criada na VM descartável
I=$S/pacote/donnabot-instalar-atualizacao
FALHAS=0
SUDOERS_PROVA=/etc/sudoers.d/zz-prova
SUDOERS_PROVA_CRIADO=0
limpar_sudoers_prova() {
  if [ "$SUDOERS_PROVA_CRIADO" = 1 ] && [ ! -L "$SUDOERS_PROVA" ] && [ -f "$SUDOERS_PROVA" ]; then
    rm -f -- "$SUDOERS_PROVA"
  fi
  SUDOERS_PROVA_CRIADO=0
}
trap limpar_sudoers_prova EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
como_hub() { runuser -u hublogin -- env XDG_RUNTIME_DIR=/run/user/$U DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$U/bus MEXERICA_HOME=$H/dados "$@"; }
coord() { como_hub $H/.local/bin/donnabot-host coordenar-atualizacao "$@"; }
servico() { como_hub systemctl --user is-active donnabot-host.service 2>/dev/null; }
exe_hub() { local p; p=$(como_hub systemctl --user show -p MainPID --value donnabot-host.service); [ "$p" -gt 0 ] 2>/dev/null && readlink /proc/$p/exe || echo "-"; }
caso() { echo; echo "=== $1"; }
espera() { local esperado=$1 desc=$2; shift 2; "$@" > /tmp/saida.txt 2>&1; local rc=$?
  if [ "$rc" = "$esperado" ]; then echo "  OK   rc=$rc $desc"; else echo "  FALHA rc=$rc (esperado $esperado) $desc"; FALHAS=$((FALHAS+1)); sed 's/^/    | /' /tmp/saida.txt | tail -8; fi; }
confere() { if eval "$2"; then echo "  OK   $1"; else echo "  FALHA $1"; FALHAS=$((FALHAS+1)); fi; }
journal() { python3 -c 'import json;j=json.load(open("/var/lib/donnabot/atualizacao/journal"));print(j["etapa"],j["estado"]["fase"]["fase"],"seq",j["sequencia_nova"])' 2>/dev/null || echo "(sem journal)"; }
# Consultas de prova: não exibem o token local nem o corpo do hello.
porta_livre() { local linhas; linhas=$(ss -H -ltn 'sport = :7777') || return 1; [ -z "$linhas" ]; }
versao_hello_confere() {
  como_hub python3 -c 'import json, subprocess, urllib.request
from pathlib import Path
exe = "/opt/donnabot/releases/ativo/donnabot-host"
versao = json.loads(Path("/srv/prova/www/repo/manifesto.json").read_text())["versao"]
assert subprocess.check_output([exe, "--version"], text=True).strip().split()[-1] == versao
token = Path("'"$H"'/dados/token").read_text().strip()
req = urllib.request.Request("http://127.0.0.1:7777/api/hello", data=b"{}", headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
with urllib.request.urlopen(req, timeout=10) as resposta:
    assert json.load(resposta).get("version") == versao'
}
sentinela() { como_hub python3 -c "import sqlite3;print(sqlite3.connect('$H/dados/codync.db').execute(\"select v from kv where k='prova_sentinela'\").fetchone())"; }

python3 $D/amostrador.py $U $S/amostras.log & AMOSTRADOR=$!

caso "etapa administrativa: recusas fail-closed antes de qualquer alteração"
espera 1 "fingerprint de pacote errado" $I instalar --login hublogin --pacote $S/pacote --fingerprint-pacote $(printf '0%.0s' $(seq 64)) --fingerprint-raiz $(cat $S/fp-raiz)
espera 1 "fingerprint de raiz errado" $I instalar --login hublogin --pacote $S/pacote --fingerprint-pacote $(cat $S/fp-pacote) --fingerprint-raiz $(printf '0%.0s' $(seq 64))
espera 1 "login inexistente" $I instalar --login naoexiste --pacote $S/pacote --fingerprint-pacote $(cat $S/fp-pacote) --fingerprint-raiz $(cat $S/fp-raiz)
mkdir -p /opt/donnabot/releases
espera 1 "estado misto (releases já existe)" $I instalar --login hublogin --pacote $S/pacote --fingerprint-pacote $(cat $S/fp-pacote) --fingerprint-raiz $(cat $S/fp-raiz)
grep -q "estado misto" /tmp/saida.txt && echo "  OK   motivo: estado misto" || { echo "  FALHA motivo"; FALHAS=$((FALHAS+1)); }
rmdir /opt/donnabot/releases /opt/donnabot
confere "nada foi criado pelas recusas" '[ ! -e $E ] && [ ! -e /etc/sudoers.d/donnabot ] && ! id donnabot-motor >/dev/null 2>&1'
espera 1 "UID do Hub não pode executar o instalador (SUDO_UID)" env SUDO_UID=$U $I instalar --login hublogin --pacote $S/pacote --fingerprint-pacote $(cat $S/fp-pacote) --fingerprint-raiz $(cat $S/fp-raiz)

caso "etapa administrativa real"
espera 0 "instalar" $I instalar --login hublogin --pacote $S/pacote --fingerprint-pacote $(cat $S/fp-pacote) --fingerprint-raiz $(cat $S/fp-raiz)
sed 's/^/    | /' /tmp/saida.txt
confere "conta motor nologin, home 0700, trabalho 2770" 'id donnabot-motor >/dev/null && [ "$(stat -c %U:%a /var/lib/donnabot/motor)" = donnabot-motor:700 ] && [ "$(stat -c %U:%G:%a /var/lib/donnabot/trabalho)" = hublogin:donnabot-trabalho:2770 ]'
confere "motor.conf root 0644 com hub_uid" '[ "$(stat -c %U:%a /etc/donnabot/motor.conf)" = root:644 ] && grep -qx "hub_uid=$U" /etc/donnabot/motor.conf'
confere "sudoers 0440 validado, wrapper/guard/instalador root 0755" '[ "$(stat -c %U:%a /etc/sudoers.d/donnabot)" = root:440 ] && [ "$(stat -c %U:%a $W)" = root:755 ] && visudo -cq'
confere "cgroup com limites da política" '[ "$(cat /sys/fs/cgroup/donnabot-atualizacao/memory.max)" = 1073741824 ] && [ "$(cat /sys/fs/cgroup/donnabot-atualizacao/cpu.max)" = "200000 100000" ]'
confere "guard ativo e unidade de cgroup habilitada" 'systemctl is-active -q donnabot-guarda-atualizacao@$U && systemctl is-enabled -q donnabot-cgroup-atualizacao'
confere "baseline com objetos ausentes e legado por hash" 'python3 -c "import json;b=json.load(open(\"$E/baseline\"));assert b[\"objetos\"][\"usuario:donnabot-motor\"] is False and b[\"legado\"][\"unidade_sha256\"]"'
confere "Hub legado continua ativo com o guard instalado (sem journal)" '[ "$(servico)" = active ]'
espera 0 "reexecução: instalação existente é conferida" $I instalar --login hublogin --pacote $S/pacote --fingerprint-pacote $(cat $S/fp-pacote) --fingerprint-raiz $(cat $S/fp-raiz)
espera 0 "verificar" $I verificar --login hublogin

caso "wrapper recusa a troca com o Hub rodando (sem alterar nada)"
espera 1 "sudo -n wrapper com serviço ativo" como_hub sudo -n $W
confere "motivo: Hub em execução; sem journal nem seletor" 'grep -q "Hub em execução" $E/diagnostico && [ ! -e $E/journal ] && [ ! -L $R/ativo ]'

. $D/preparatoria.sh
preparatoria

GERENTE=$(systemctl show -p MainPID --value user@$U.service)
caso "coordenador: 1ª migração (seq 1) com aceite"
# Layout real de perfil gerido: home <uuid> com a marca .donnabot-profile (raiz F3 enumerada).
ACC=ac_$(printf 'd%.0s' $(seq 64)); HOMEID=6f1e2d3c-4b5a-4c6d-8e7f-0a1b2c3d4e5f
PERFIL=$H/dados/engine-setup/accounts/$ACC/$HOMEID/.claude  # vazamento-ok: conta de teste criada na VM descartável
como_hub systemctl --user stop donnabot-host.service
confere "porta do Hub livre após parada" 'porta_livre'
como_hub mkdir -p $PERFIL/sub && como_hub sh -c "touch $(dirname $PERFIL)/.donnabot-profile; chmod 600 $(dirname $PERFIL)/.donnabot-profile; printf '{\"tema\":\"x\"}' > $PERFIL/settings.json; printf '#!/bin/sh\necho perfil\n' > $PERFIL/sub/run.sh; chmod 755 $PERFIL/sub/run.sh"
# Skill com script executável (0755): a cópia de skills é 0640 por contrato e não pode divergir.
como_hub sh -c "mkdir -p $H/dados/skills/prova && printf '#!/bin/sh\necho skill\n' > $H/dados/skills/prova/run.sh && chmod 755 $H/dados/skills/prova/run.sh"
como_hub python3 - <<PY
import json, sqlite3, uuid
c = sqlite3.connect("$H/dados/codync.db")
c.execute("insert or replace into kv values('prova_sentinela','antes')")
conta = {"id": "$ACC", "backend": "claude", "billing": "subscription", "state": "connected",
         "launch": {"kind": "cli_profile", "executable": "/bin/true", "directory": "$PERFIL"},
         "scope": "owner", "quotaIdentity": "$ACC", "verifiedAt": 1,
         "subscriptionProof": {"method": "claude_auth_status", "plan": "max"}, "models": [{"id": "haiku", "options": []}]}
c.execute("insert or replace into kv values('fork.engine-accounts.v1', ?)", (json.dumps({"salt": str(uuid.uuid4()), "accounts": [conta]}),))
c.commit()
PY
como_hub systemctl --user start donnabot-host.service; sleep 3
espera 0 "coordenar-atualizacao" coord
sed 's/^/    | /' /tmp/saida.txt | grep coordenador
confere "versões do binário e do hello correspondem ao manifesto de teste" 'versao_hello_confere'
confere "Hub ativo executando o par gerido do slot-a" '[ "$(servico)" = active ] && [ "$(exe_hub)" = $R/slot-a/donnabot-host ]'
confere "status liberado, dados preservados" 'grep -q liberado $R/status.json && sentinela | grep -q antes'
M=/var/lib/donnabot/motor/engine-setup/accounts/$ACC/$HOMEID/.claude
confere "F3: perfil copiado pelo canal como donnabot-motor (0700/0600, executável preservado)" '[ "$(stat -c %U:%a $M)" = donnabot-motor:700 ] && [ "$(stat -c %U:%a $M/settings.json)" = donnabot-motor:600 ] && [ "$(stat -c %a $M/sub/run.sh)" = 700 ] && cmp -s $M/settings.json $PERFIL/settings.json'
confere "F3: referências reescritas e marca d330 gravada; origem preservada" 'como_hub python3 -c "import json,sqlite3;c=sqlite3.connect(\"$H/dados/codync.db\");r=json.loads(c.execute(\"select v from kv where k=?\",(\"fork.engine-accounts.v1\",)).fetchone()[0]);assert r[\"accounts\"][0][\"launch\"][\"directory\"]==\"$M\";assert c.execute(\"select 1 from kv where k=\x27d330.raizes.v1\x27\").fetchone()" && [ -f $PERFIL/settings.json ]'
T=/var/lib/donnabot/trabalho; RAIZES="$T/skills $T/bots"
confere "F3: raízes de trabalho copiadas pelo Hub com grupo de trabalho" '[ -d $T/skills ] && [ -d $T/bots ] && [ -z "$(find $RAIZES ! -user hublogin)" ] && [ -z "$(find $RAIZES ! -group donnabot-trabalho)" ]'
confere "canal: socket donnabot-motor:hublogin 0660; serviço executa como donnabot-motor" '[ "$(stat -c %U:%G:%a /run/donnabot-motor-copia.sock)" = donnabot-motor:hublogin:660 ] && [ "$(systemctl show -p User --value donnabot-motor-copia.service)" = donnabot-motor ]'
confere "canal: outro UID não conecta (modo do socket)" '! runuser -u nobody -- python3 -c "import socket;s=socket.socket(socket.AF_UNIX);s.connect(\"/run/donnabot-motor-copia.sock\")" 2>/dev/null'
confere "F3: skill executável copiada sem divergência, 0640 por contrato" '[ "$(stat -c %a /var/lib/donnabot/trabalho/skills/prova/run.sh)" = 640 ] && cmp -s /var/lib/donnabot/trabalho/skills/prova/run.sh $H/dados/skills/prova/run.sh'
confere "canal: root conecta mas é recusado pelo SO_PEERCRED" 'python3 $D/canal.py "UID do Hub" >/dev/null'
install -m 0644 $D/canal.py /tmp/prova-canal.py
confere "canal: Hub fora da janela da 1ª migração é recusado (após o aceite)" 'runuser -u hublogin -- python3 /tmp/prova-canal.py "janela" >/dev/null'
confere "canal: registro durável só com as raízes F3 copiadas" '[ "$(cat /var/lib/donnabot/motor/engine-setup/accounts/.donnabot-raizes)" = "$ACC/$HOMEID" ]'
confere "backup privado 0700 com comprovante vinculado" '[ "$(stat -c %a $H/dados/atualizacao-privilegiada)" = 700 ] && ls $H/dados/atualizacao-privilegiada/*/comprovante.json >/dev/null'

caso "coordenador: seq 2 publicada e volta forçada completa (DB, drop-ins, --reverter)"
python3 $D/repo.py publicar $S 2 1 $S/art/hub2 $S/art/aj && chmod -R a+rX $S/www
espera 0 "coordenar-atualizacao --forcar-reversao" coord --forcar-reversao --prova
sed 's/^/    | /' /tmp/saida.txt | grep coordenador
confere "voltou ao slot-a ativo e saudável; journal revertido" '[ "$(servico)" = active ] && [ "$(exe_hub)" = $R/slot-a/donnabot-host ] && journal | grep -q "concluida revertida"'
confere "estado posterior preservado em cópia separada" 'ls $H/dados/atualizacao-privilegiada/*/posterior.db >/dev/null'

caso "coordenador: seq 3 aceita (recicla o slot revertido)"
python3 $D/repo.py publicar $S 3 1 $S/art/hub3 $S/art/aj && chmod -R a+rX $S/www
espera 0 "coordenar-atualizacao" coord
confere "slot-b ativo" '[ "$(exe_hub)" = $R/slot-b/donnabot-host ] && journal | grep -q "concluida aceita"'

confere "gerenciador systemd --user do Hub intacto após três trocas" '[ "$(systemctl show -p MainPID --value user@$U.service)" = "$GERENTE" ]'

caso "sudo empacotado como link simbólico (P2 da pré-revisão)"
mv /usr/bin/sudo /usr/libexec/sudo-real-prova && ln -s /usr/libexec/sudo-real-prova /usr/bin/sudo
espera 0 "aceite repetido pelo Hub com /usr/bin/sudo sendo link" como_hub sudo -n $W --aceitar
confere "auditado com origem sudo e UID do Hub" 'tail -1 $E/auditoria | grep -q "\"uid\":$U,\"origem\":\"sudo\""'
rm /usr/bin/sudo && mv /usr/libexec/sudo-real-prova /usr/bin/sudo

caso "opções de prova exigem --prova"
espera 1 "--sem-aceite sem --prova é recusado" coord --sem-aceite
confere "recusado antes de parar o Hub" 'grep -q "exigem --prova" /tmp/saida.txt && [ "$(servico)" = active ]'

caso "rotação da raiz TUF"
python3 $D/repo.py rotacionar $S 2 --so-nova
python3 $D/repo.py publicar $S 4 1 $S/art/hub1 $S/art/aj && chmod -R a+rX $S/www
espera 1 "raiz 2 só com o quórum novo é recusada" coord
grep -q "limiar TUF" $E/diagnostico && echo "  OK   motivo: limiar TUF não atingido" || { echo "  FALHA motivo: $(cat $E/diagnostico)"; FALHAS=$((FALHAS+1)); }
for i in $(seq 1 20); do [ "$(servico)" = active ] && break; sleep 1; done
confere "Hub restabelecido no slot-b sem troca" '[ "$(servico)" = active ] && [ "$(exe_hub)" = $R/slot-b/donnabot-host ]'
rm -f $S/www/repo/2.root.json
python3 $D/repo.py rotacionar $S 2
python3 $D/repo.py publicar $S 4 1 $S/art/hub1 $S/art/aj && chmod -R a+rX $S/www
espera 0 "raiz 2 com os dois quóruns e nova chave de timestamp" coord
confere "histórico na raiz 2 e par 4 ativo" 'python3 -c "import json;h=json.load(open(\"$E/historico-tuf\"));assert h[\"raiz\"]==2" && journal | grep -q "seq 4"'

caso "guard: expiração sem aceite para o serviço e bloqueia"
python3 $D/repo.py publicar $S 5 1 $S/art/hub2 $S/art/aj && chmod -R a+rX $S/www
espera 0 "coordenar-atualizacao --sem-aceite" coord --sem-aceite --prova
confere "janela pendente com o Hub ativo" '[ "$(servico)" = active ] && grep -q pendente $R/status.json'
PRAZO=$(python3 -c "import json;print(json.load(open(\"$E/journal\"))[\"estado\"][\"fase\"][\"prazo\"])")
echo "  prazo em $((PRAZO - $(date +%s))) s"; while [ "$(date +%s)" -le $((PRAZO + 5)) ]; do sleep 2; done; sleep 5
confere "guard parou o Hub e bloqueou duravelmente" '[ "$(servico)" != active ] && [ -e $E/bloqueio ] && grep -q bloqueado $R/status.json && journal | grep -q expirada'
como_hub systemctl --user start donnabot-host.service; sleep 6
confere "Hub não volta a iniciar com o bloqueio (gate do status)" '[ "$(servico)" != active ]'
espera 1 "wrapper recusa nova operação após expiração" coord

kill $AMOSTRADOR

caso "revogação administrativa"
espera 1 "login que não é o Hub da instalação é recusado sem efeito" $I revogar --login nobody
confere "nada alterado pela recusa" '[ -e /etc/sudoers.d/donnabot ] && [ -e $E ] && [ -e $R ]'
if [ -e "$SUDOERS_PROVA" ] || [ -L "$SUDOERS_PROVA" ]; then echo 'FALHA arquivo sudoers de prova já existe'; exit 1; else
  (set -o noclobber; printf 'hublogin ALL=(root) NOPASSWD: ALL\n' > "$SUDOERS_PROVA") && SUDOERS_PROVA_CRIADO=1 || { echo 'FALHA criando sudoers de prova'; FALHAS=$((FALHAS+1)); }
fi
[ "$SUDOERS_PROVA_CRIADO" = 1 ] && chmod 440 -- "$SUDOERS_PROVA"
espera 1 "regra ALL residual impede declarar a revogação" $I revogar --login hublogin
confere "falha auditada, estado não arquivado" 'grep -q "\"operacao\":\"revogar\".*\"resultado\":\"falha\"" $E/auditoria && [ -e $E ]'
if [ "$SUDOERS_PROVA_CRIADO" = 1 ]; then
  if [ ! -L "$SUDOERS_PROVA" ] && [ -f "$SUDOERS_PROVA" ]; then
    printf 'Cmnd_Alias MX = /opt/donnabot/sbin/*\nhublogin ALL=(root) NOPASSWD: MX\n' > "$SUDOERS_PROVA"
  else
    echo 'FALHA arquivo sudoers de prova foi substituído'; exit 1
  fi
fi
espera 1 "alias com curinga também é detectado" $I revogar --login hublogin
limpar_sudoers_prova
SUDOERS_PROVA_CRIADO=0
python3 $D/pausado.py $W /tmp/prova-pausado.pid > /tmp/prova-pausado.log 2>&1 & sleep 1
confere "chamada do wrapper suspensa antes da trava (ptrace no exec)" 'P=$(cat /tmp/prova-pausado.pid) && [ "$(readlink /proc/$P/exe)" = $W ] && grep -qE "^State:\s+t" /proc/$P/status'
flock $E/lock sleep 6 & TRAVA=$!; sleep 1; T0=$(date +%s)
espera 0 "revogar (espera a trava do wrapper em uso)" $I revogar --login hublogin
confere "revogação serializada pela trava do wrapper (esperou a operação em curso)" '[ $(( $(date +%s) - T0 )) -ge 4 ]'
sleep 1; confere "chamada suspensa encerrada pela revogação (SIGKILL), sem processo restante" 'grep -q "sinal 9" /tmp/prova-pausado.log && [ ! -e /proc/$(cat /tmp/prova-pausado.pid) ]'
wait $TRAVA 2>/dev/null
sed 's/^/    | /' /tmp/saida.txt | grep revogar
confere "sudoers removido e sudo -l sem o wrapper" '[ ! -e /etc/sudoers.d/donnabot ] && ! sudo -n -l -U hublogin 2>/dev/null | grep -q atualizar-privilegiado && visudo -cq'
confere "nenhuma capability restante em /opt; slots, seletor e binários removidos" '[ -z "$(getcap -r /opt 2>/dev/null)" ] && [ ! -e $R ] && [ ! -e /opt/donnabot/sbin ]'
confere "guard, canal e cgroup desligados e removidos" '! systemctl is-active -q donnabot-guarda-atualizacao@$U && ! systemctl is-active -q donnabot-motor-copia.socket && [ ! -e /etc/systemd/system/donnabot-guarda-atualizacao@.service ] && [ ! -e /run/donnabot-motor-copia.sock ] && [ ! -e /sys/fs/cgroup/donnabot-atualizacao ]'
confere "estado arquivado com auditoria da revogação; conta e dados do motor preservados" 'A=$(ls -d /var/lib/donnabot/atualizacao.revogada-* | head -1) && [ "$(stat -c %U:%a $A)" = root:700 ] && grep -q "\"operacao\":\"revogar\",\"sequencia_atual\":null,\"sequencia_anterior\":null,\"resultado\":\"sucesso\"" $A/auditoria && id donnabot-motor >/dev/null && [ -d $M ] && [ ! -e $E ]'
confere "Hub parado após a revogação" '[ "$(servico)" != active ]'
espera 0 "revogar de novo é idempotente" $I revogar --login hublogin
caso "prova serviço-parado (amostragem a cada 50 ms)"
python3 $D/analisar.py $S/amostras.log; rc=$?
[ $rc = 0 ] && echo "  OK   nenhuma fase crítica ou troca de seletor com o Hub rodando" || { echo "  FALHA análise"; FALHAS=$((FALHAS+1)); }
caso "auditoria"
AUD=$(ls -d /var/lib/donnabot/atualizacao.revogada-* | tail -1)/auditoria
grep -c '"operacao":"administrar"' $AUD | sed 's/^/  eventos administrativos: /'
grep -c '"origem":"guarda_root"' $AUD | sed 's/^/  eventos do guard: /'
echo; echo "RESULTADO: $FALHAS falha(s)"
[ "$FALHAS" = 0 ]
