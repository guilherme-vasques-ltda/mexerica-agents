#!/bin/bash
# Desfaz o ambiente E2E desta VM descartável (somente teste).
[ "${PROVA_VM:-}" = 1 ] || { echo 'defina PROVA_VM=1: confirme que está numa VM descartável antes de limpar' >&2; exit 1; }
U=$(id -u hublogin 2>/dev/null)
pkill -f "servidor.py"; pkill -f "amostrador.py"
[ -n "$U" ] && systemctl disable --now donnabot-guarda-atualizacao@$U.service 2>/dev/null
systemctl disable --now donnabot-cgroup-atualizacao.service donnabot-motor-copia.socket donnabot-motor-copia.service 2>/dev/null
rm -f /etc/systemd/system/donnabot-guarda-atualizacao@.service /etc/systemd/system/donnabot-cgroup-atualizacao.service /etc/systemd/system/donnabot-motor-copia.socket /etc/systemd/system/donnabot-motor-copia.service /etc/sudoers.d/donnabot /run/donnabot-motor-copia.sock
rm -rf /etc/systemd/user/donnabot-host.service.d /opt/donnabot /var/lib/donnabot /etc/donnabot /srv/prova
rmdir /sys/fs/cgroup/donnabot-atualizacao 2>/dev/null
userdel donnabot-motor 2>/dev/null; groupdel donnabot-motor 2>/dev/null; groupdel donnabot-trabalho 2>/dev/null
if [ -n "$U" ]; then loginctl disable-linger hublogin; sleep 2; pkill -u hublogin; sleep 1; userdel -r hublogin 2>/dev/null; fi
systemctl daemon-reload; systemctl reset-failed 2>/dev/null
echo limpo
