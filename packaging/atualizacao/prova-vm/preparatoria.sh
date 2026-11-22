# Incluído (source) por cenarios.sh e reboot.sh (instalar), depois de definirem como_hub,
# servico, exe_hub, espera e confere. O Hub legado da VM é um build ANTERIOR ao gate de
# partida (revisão anterior ao gate, ver preparar.sh): o coordenador do build atual recusa
# migrá-lo sem nenhum efeito; só depois da atualização preparatória (o legado passa a ser
# o build atual, com o gate) a migração segue.
preparatoria() {
  echo; echo "=== legado anterior ao gate: coordenador recusa sem efeito; atualização preparatória"
  confere "legado instalado é anterior ao gate (não conhece coordenar-atualizacao)" '! como_hub $H/.local/bin/donnabot-host coordenar-atualizacao --help >/dev/null 2>&1'
  espera 1 "coordenador do build atual com a unidade no legado anterior" como_hub $S/novo/donnabot-host coordenar-atualizacao
  confere "motivo: atualização preparatória exigida; nada alterado" 'grep -q "nada foi alterado" /tmp/saida.txt'
  confere "legado anterior segue ativo; sem backup, sem seletor, sem drop-in" '[ "$(servico)" = active ] && [ "$(exe_hub)" = $H/.local/bin/donnabot-host ] && [ ! -e $H/dados/atualizacao-privilegiada ] && [ ! -L $R/ativo ] && [ ! -e $H/.config/systemd/user/donnabot-host.service.d/zz-donnabot-atualizacao.conf ]'
  como_hub systemctl --user stop donnabot-host.service
  install -m 0755 -o hublogin -g hublogin $S/novo/donnabot-host $H/.local/bin/donnabot-host
  como_hub systemctl --user start donnabot-host.service
  for _ in $(seq 1 30); do [ "$(servico)" = active ] && break; sleep 1; done
  confere "atualização preparatória: legado é o build atual (com o gate) e está ativo" 'cmp -s $H/.local/bin/donnabot-host $S/novo/donnabot-host && [ "$(servico)" = active ]'
}
