#!/bin/sh
# Preflight somente leitura; não é instalador e não habilita atualização.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
IFS=':'
export PATH IFS
umask 077

falhar() { printf '%s\n' "$1" >&2; exit 1; }
[ "$#" -eq 1 ] || falhar 'Informe somente o login local do Hub.'
login=$1
case "$login" in
    ''|*[!a-z0-9_-]*|[!a-z_]*|?????????????????????????????????*)
        falhar 'Login incompatível com a regra sudoers restrita.' ;;
esac
[ "$(/usr/bin/id -u)" -eq 0 ] || falhar 'O preflight exige root.'
[ -x /usr/bin/getent ] && [ -x /usr/sbin/visudo ] || falhar 'getent ou visudo ausente.'
linha=$(/usr/bin/getent -s files passwd "$login") || falhar 'Login local não encontrado.'
# Exigir entrada única e nome literal, sem interpretar conteúdo como shell.
[ "$(printf '%s\n' "$linha" | /usr/bin/wc -l)" -eq 1 ] || falhar 'Login ambíguo.'
uid=$(printf '%s\n' "$linha" | /usr/bin/cut -d: -f3)
gid=$(printf '%s\n' "$linha" | /usr/bin/cut -d: -f4)
nome=$(printf '%s\n' "$linha" | /usr/bin/cut -d: -f1)
[ "$nome" = "$login" ] || falhar 'Nome da conta não corresponde ao login.'
case "$uid:$gid" in *[!0-9:]*|:*|*:) falhar 'UID ou GID inválido.' ;; esac
[ "$uid" -gt 0 ] && [ "$gid" -gt 0 ] || falhar 'UID ou GID inválido.'
contagem=$(/usr/bin/getent -s files passwd | /usr/bin/awk -F: -v uid="$uid" '$3 == uid { n++ } END { print n+0 }')
[ "$contagem" -eq 1 ] || falhar 'UID local ambíguo.'
[ -r /proc/sys/fs/protected_hardlinks ] || falhar 'protected_hardlinks indisponível.'
IFS= read -r protegido < /proc/sys/fs/protected_hardlinks || falhar 'Falha ao ler protected_hardlinks.'
[ "$protegido" = 1 ] || falhar 'Ative fs.protected_hardlinks antes de prosseguir.'
[ -f /sys/fs/cgroup/cgroup.controllers ] || falhar 'cgroup v2 indisponível.'
/usr/sbin/visudo -c -q || falhar 'Configuração sudoers existente inválida.'
for caminho in /etc/sudoers.d/donnabot /opt/donnabot/sbin/atualizar-privilegiado; do
    [ ! -e "$caminho" ] && [ ! -L "$caminho" ] || falhar 'Objeto de atualização já existe; revisar conflito manualmente.'
done
# Os gates de relógio, serviço, backup e a cerimônia ainda não foram integrados.
falhar 'Preflight parcial: instalação bloqueada até conclusão da Fase 1 e revisão de segurança.'
