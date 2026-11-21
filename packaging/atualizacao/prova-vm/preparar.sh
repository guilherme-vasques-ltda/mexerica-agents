#!/bin/bash
# VM descartável, como root: Hub LEGADO real (unidade systemd --user), repositório TUF de
# teste por HTTPS local e o pacote administrativo. NÃO instala a etapa administrativa
# (isso é feito pelo instalador real em cenarios.sh). Chaves de teste geradas aqui.
set -euo pipefail
umask 077
D=$(dirname "$(readlink -f "$0")"); T=${CARGO_TARGET_DIR:-/data/target}/debug; S=/srv/prova
# Hub legado REAL, anterior ao gate de partida: donnabot-host de uma revisão anterior ao gate (ver
# README). O build atual não serve de legado: ele já tem o gate e mascararia o corte.
L=${LEGADO_ANTERIOR:?defina LEGADO_ANTERIOR com o binário donnabot-host anterior ao gate}
if "$L" coordenar-atualizacao --help >/dev/null 2>&1; then
  echo "LEGADO_ANTERIOR conhece coordenar-atualizacao: não é anterior ao gate" >&2; exit 1
fi
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq && apt-get install -y -qq python3-cryptography libcap2-bin systemd-timesyncd dbus dbus-user-session >/dev/null 2>&1 || true
systemctl unmask systemd-timesyncd >/dev/null 2>&1 || true
systemctl enable --now systemd-timesyncd >/dev/null 2>&1 || true
timedatectl set-ntp true || true
for i in $(seq 1 60); do [ "$(timedatectl show -p NTPSynchronized --value)" = yes ] && break; sleep 1; done
# Servidor configurado: proteção de hardlinks persistente (o wrapper recusa sem ela).
echo 'fs.protected_hardlinks = 1' > /etc/sysctl.d/90-protected-hardlinks.conf
sysctl -q fs.protected_hardlinks=1
id hublogin >/dev/null 2>&1 || useradd -m -s /bin/bash hublogin
systemctl start dbus
loginctl enable-linger hublogin
systemctl restart user@$(id -u hublogin).service
for i in $(seq 1 50); do [ -S /run/user/$(id -u hublogin)/bus ] && break; sleep 0.2; done
H=/home/hublogin  # vazamento-ok: conta de teste criada na VM descartável
# Hub legado: binário no usuário, dados em ~/dados, unidade de usuário.
runuser -u hublogin -- mkdir -p $H/.local/bin $H/.config/systemd/user $H/dados
install -m 0755 -o hublogin -g hublogin "$L" $H/.local/bin/donnabot-host
chmod 700 $H/dados
cat > $H/.config/systemd/user/donnabot-host.service <<UNI
[Unit]
Description=Mexerica Hub (legado de teste)

[Service]
Environment=MEXERICA_HOME=$H/dados
ExecStart=$H/.local/bin/donnabot-host serve --port 7777 --bind 127.0.0.1
Restart=on-failure
RestartSec=2

[Install]
WantedBy=default.target
UNI
chown hublogin:hublogin $H/.config/systemd/user/donnabot-host.service
como_hub() { runuser -u hublogin -- env XDG_RUNTIME_DIR=/run/user/$(id -u hublogin) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u hublogin)/bus "$@"; }
como_hub systemctl --user daemon-reload
como_hub systemctl --user enable --now donnabot-host.service
for i in $(seq 1 60); do [ -f $H/dados/codync.db ] && break; sleep 1; done
sleep 3; como_hub systemctl --user is-active donnabot-host.service
# TLS e repositório de teste.
install -d -m 0755 $S $S/tls $S/art $S/pacote $S/novo
# Build atual (com o gate): coordenador da recusa e binário da atualização preparatória.
install -m 0755 $T/donnabot-host $S/novo/donnabot-host
cd $S/tls
openssl req -x509 -nodes -newkey ec -pkeyopt ec_paramgen_curve:P-256 -keyout ca.key -out ca.crt -days 2 -subj "/CN=CA de teste" \
  -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign,cRLSign 2>/dev/null
openssl req -nodes -newkey ec -pkeyopt ec_paramgen_curve:P-256 -keyout servidor.key -out servidor.csr -subj "/CN=releases.prova.test" 2>/dev/null
printf "subjectAltName=DNS:releases.prova.test\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature\nextendedKeyUsage=serverAuth\n" > ext
openssl x509 -req -in servidor.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out servidor.crt -days 2 -extfile ext 2>/dev/null
install -m 0644 ca.crt /usr/local/share/ca-certificates/prova-teste.crt && update-ca-certificates >/dev/null 2>&1
grep -q releases.prova.test /etc/hosts || echo "127.0.0.1 releases.prova.test" >> /etc/hosts
cp $T/donnabot-host $S/art/hub1; cp $T/donnabot-host $S/art/hub2; printf 'v2' >> $S/art/hub2
cp $T/donnabot-host $S/art/hub3; printf 'v3' >> $S/art/hub3
cp $T/donnabot-motor-exec $S/art/aj
python3 $D/repo.py init $S > $S/init.json
python3 $D/repo.py publicar $S 1 0 $S/art/hub1 $S/art/aj
chmod -R a+rX $S/www
nohup python3 $D/servidor.py > $S/servidor.log 2>&1 < /dev/null & echo $! > $S/servidor.pid
# Pacote administrativo (o que o administrador recebe por canal oficial).
P=$S/pacote
cp $T/donnabot-atualizar-privilegiado $P/atualizar-privilegiado
cp $T/donnabot-guarda-atualizacao $P/donnabot-guarda-atualizacao
cp $T/donnabot-instalar-atualizacao $P/donnabot-instalar-atualizacao
cp $T/donnabot-motor-copia $P/donnabot-motor-copia
cp $S/www/repo/1.root.json $P/raiz.json
python3 - <<PY
import json; ini = json.load(open("$S/init.json"))
json.dump({"endpoint": "https://releases.prova.test/repo/", "canal": "estavel", "chave_manifesto": ini["chave_manifesto"],
  "tamanho_maximo": 1073741824, "tempo_rede_segundos": 120, "janela_minima": 60, "janela_maxima": 120,
  "cgroup_memoria": 1073741824, "cgroup_processos": 64, "cgroup_cpu_quota": 200000, "cgroup_cpu_periodo": 100000}, open("$P/politica.json", "w"))
PY
(cd $P && sha256sum atualizar-privilegiado donnabot-guarda-atualizacao donnabot-instalar-atualizacao donnabot-motor-copia raiz.json politica.json > SHA256SUMS)
sha256sum $P/SHA256SUMS | cut -d' ' -f1 > $S/fp-pacote; sha256sum $P/raiz.json | cut -d' ' -f1 > $S/fp-raiz
chmod -R a+rX $S/art
curl --noproxy '*' -sS -o /dev/null -w 'https %{http_code}\n' https://releases.prova.test/repo/timestamp.json
echo "NTP: $(timedatectl show -p NTPSynchronized --value)"
echo "preparado"
