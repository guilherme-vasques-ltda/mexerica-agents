// MEX-1307 F0 (plano §4.3.1): scripts `sh` fixos, embutidos no app com hash conferido. Nenhum byte vem do usuário nem da
// VPS: os scripts não têm parâmetros (a porta do Hub é a padrão, 19422) e só imprimem quadros (§4.6), nunca texto livre;
// o `stderr` vai para /dev/null. Escritos para este repositório (nenhum código de terceiro copiado; ver THIRD_PARTY.md).
//
// Para o `sh` os cifrões de chave aparecem aqui como `@{`, trocados por `${` na carga (o literal de template do
// TypeScript interpola `${NOME}`: só as três constantes acima entram assim, como números e nomes fixos). Os scripts não usam aspas simples nem crase, porque o comando remoto os embrulha
// em aspas simples (ssh-install-command.ts).
import { createHash } from 'node:crypto'

const sh = (texto: string) => texto.replaceAll('@{', '${')
export const sha256Texto = (texto: string) => createHash('sha256').update(texto, 'utf8').digest('hex')

/** Porta padrão do Hub (`newSSHProfile`): a sondagem confere só ela. */
export const PORTA_PADRAO_HUB = 19422
/** Nome fixo da conta do Hub (plano §4.0). */
export const CONTA_HUB = 'donnabot-hub'
/** Espaço livre mínimo em /opt e /var (KiB). Valor provisório, **[A medir]** na F6. */
export const ESPACO_MIN_KIB = 1048576

/**
 * Sondagem (a): como a conta administrativa, sem elevação, **somente leitura** (nada é gravado no servidor).
 * Fatos: `ok` = presente, `pulado` = ausente comprovado, `falha` = não consegui determinar.
 * Requisitos: `ok` = atende, `falha` = não atende, `pulado` = não consegui determinar (vale como falha).
 * V6: `sudo -n true` sem senha e sem tty; o resultado vai em `sudosemsenha` e `sudotty`.
 */
export const SCRIPT_SONDAGEM = sh(String.raw`PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
LC_ALL=C
export LC_ALL
umask 077
exec 2>/dev/null
ev() { printf "MEX1 E %s %s\n" "$1" "$2"; }
dado() { printf "MEX1 D %s %s\n%s\nMEX1 FIM %s\n" "$1" "@{#2}" "$2" "$1"; }

# 0 = presente, 1 = ausente comprovado (a pasta pai foi listada e o nome nao esta nela), 2 = nao consegui determinar.
existe() {
  [ -e "$1" ] && return 0
  pai=@{1%/*}
  [ -n "$pai" ] || pai=/
  { [ -d "$pai" ] && [ -r "$pai" ] && [ -x "$pai" ]; } || return 2
  lista=$(ls -A "$pai") || return 2
  case "
$lista
" in *"
@{1##*/}
"*) return 0 ;; *) return 1 ;; esac
}

so=pulado
if [ -r /etc/os-release ]; then
  so=falha
  id_so=$(. /etc/os-release; printf "%s" "$ID")
  ver=$(. /etc/os-release; printf "%s" "$VERSION_ID")
  maior=@{ver%%.*}
  menor=@{ver#*.}
  case "$maior$menor" in *[!0-9]*|"") ;; *)
    menor=@{menor#0}
    if [ "$id_so" = debian ] && [ "$ver" = 13 ]; then so=ok; fi
    if [ "$id_so" = ubuntu ] && [ "$maior" -ge 22 ] && { [ "$maior" -gt 22 ] || [ "@{menor:-0}" -ge 4 ]; }; then so=ok; fi
  ;; esac
fi
ev so "$so"
[ "$so" = ok ] && dado so "$id_so $ver"

arq=$(uname -m)
case "$arq" in
  x86_64|aarch64) ev arquitetura ok; dado arq "$arq" ;;
  *) ev arquitetura falha ;;
esac

if [ -d /run/systemd/system ]; then ev systemd ok; else ev systemd falha; fi

espaco=ok
for d in /opt /var; do
  linha=$(df -Pk "$d" | tail -n 1)
  set -- $linha
  livre=$4
  case "$livre" in ""|*[!0-9]*) espaco=pulado ;; *) [ "$livre" -ge ${ESPACO_MIN_KIB} ] || { [ "$espaco" = pulado ] || espaco=falha; } ;; esac
done
ev espaco "$espaco"

if command -v sudo >/dev/null; then
  ev sudo ok
  saida=$(sudo -n true 2>&1 </dev/null)
  rc=$?
  if [ "$rc" -eq 0 ]; then
    ev sudotty ok; ev sudosemsenha ok
  else
    case "$saida" in
      *"a terminal is required"*|*"must have a tty"*) ev sudotty falha; ev sudosemsenha falha ;;
      *"a password is required"*) ev sudotty ok; ev sudosemsenha pulado ;;
      *) ev sudotty ok; ev sudosemsenha falha ;;
    esac
  fi
else
  ev sudo falha; ev sudotty pulado; ev sudosemsenha falha
fi

if [ -x /sbin/setcap ] || [ -x /usr/sbin/setcap ] || [ -x /usr/bin/setcap ]; then ev libcap ok; else ev libcap falha; fi

if command -v findmnt >/dev/null; then
  opts=$(findmnt -n -o OPTIONS --target /opt)
  case ",$opts," in *,nosuid,*) ev nosuid falha ;; ",,") ev nosuid pulado ;; *) ev nosuid ok ;; esac
else
  ev nosuid pulado
fi

if [ -r /proc/sys/fs/protected_hardlinks ]; then
  if [ "$(cat /proc/sys/fs/protected_hardlinks)" = 1 ]; then ev hardlinks ok; else ev hardlinks falha; fi
else
  ev hardlinks pulado
fi

if command -v timedatectl >/dev/null; then
  case "$(timedatectl show -p NTPSynchronized --value)" in yes) ev relogio ok ;; no) ev relogio falha ;; *) ev relogio pulado ;; esac
else
  ev relogio pulado
fi

existe /var/lib/systemd/linger/${CONTA_HUB}
case $? in 0) ev linger ok ;; 1) ev linger pulado ;; *) ev linger falha ;; esac

getent passwd donnabot-motor >/dev/null
case $? in 0) ev motor ok ;; 2) ev motor pulado ;; *) ev motor falha ;; esac

existe /opt/donnabot
case $? in 0) ev hostinst ok ;; 1) ev hostinst pulado ;; *) ev hostinst falha ;; esac

uid_a=$(id -u)
uid_hub=-
hub=$(getent passwd ${CONTA_HUB})
case $? in
  0) ev contahub ok; uid_hub=$(printf "%s" "$hub" | cut -d: -f3) ;;
  2) ev contahub pulado ;;
  *) ev contahub falha ;;
esac
if [ "$(id -un)" = ${CONTA_HUB} ]; then ev ehhub ok; else ev ehhub pulado; fi
dado uid "$uid_a $uid_hub"

if command -v ss >/dev/null && sockets=$(ss -ltn) && [ -n "$sockets" ]; then
  case "$sockets" in *":${PORTA_PADRAO_HUB} "*) ev portaocupada ok ;; *) ev portaocupada pulado ;; esac
else
  ev portaocupada falha
fi
`)

/**
 * Sondagem (b): carregador de **leitura** executado como root por `sudo` (plano §4.3.1b). Só `ls`, `id` e `printf`:
 * não cria, não move, não apaga. Devolve um único quadro `estado812` de lista fechada. Ausência só vale como
 * `limpo-comprovado` quando o root **listou** o diretório pai; qualquer leitura que falhe é `ilegivel`. Confere os quatro
 * caminhos, o sudoers próprio, as unidades `donnabot*` (`/etc` e `/usr/lib` do systemd) e a conta do Hub.
 *
 * LIMITE DA F0: o inventário do #812 (objetos, donos, modos e hashes; portão V3) ainda não existe em `main`. Por isso aqui
 * qualquer vestígio vira `incompleto` (intervenção manual, fail-closed), **nada vira `limpo-comprovado`** (sem inventário
 * completo, `inventario=0` bloqueia como `ilegivel`) e `concluido-integro` só nasce na F1.
 */
export const SCRIPT_LEITURA_ROOT = sh(String.raw`PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
LC_ALL=C
export LC_ALL
umask 077
exec 2>/dev/null
inventario=0
emite() { printf "MEX1 D estado812 %s\n%s\nMEX1 FIM estado812\n" "@{#1}" "$1"; }
[ "$(id -u)" = 0 ] || { emite ilegivel; exit 0; }

presenca() {
  [ "$1" = / ] && return 2
  pai=@{1%/*}
  [ -n "$pai" ] || pai=/
  if [ -d "$pai" ]; then
    lista=$(ls -A "$pai") || return 2
    case "
$lista
" in *"
@{1##*/}
"*) return 1 ;; *) return 0 ;; esac
  fi
  [ -e "$pai" ] && return 2
  presenca "$pai"
  [ $? -eq 0 ] && return 0
  return 2
}

# 0 = nenhum nome com o prefixo donnabot na pasta, 1 = ha vestigio, 2 = pasta ausente ou ilegivel.
prefixo() {
  [ -d "$1" ] || return 2
  lista=$(ls -A "$1") || return 2
  case "
$lista
" in *"
donnabot"*) return 1 ;; esac
  return 0
}

vestigio=0
for caminho in /var/lib/donnabot/atualizacao /opt/donnabot/sbin /opt/donnabot/releases /opt/donnabot/bin; do
  presenca "$caminho"
  case $? in 0) ;; 1) vestigio=1 ;; *) emite ilegivel; exit 0 ;; esac
done
for pasta in /etc/sudoers.d /etc/systemd/system /usr/lib/systemd/system; do
  prefixo "$pasta"
  case $? in 0) ;; 1) vestigio=1 ;; *) emite ilegivel; exit 0 ;; esac
done
getent passwd ${CONTA_HUB} >/dev/null
case $? in 0) vestigio=1 ;; 2) ;; *) emite ilegivel; exit 0 ;; esac
[ "$vestigio" -eq 0 ] || { emite incompleto; exit 0; }
# Inventario do #812 (V3) incompleto: sem a lista fechada de objetos, ausencia dos caminhos conhecidos nao prova limpo.
[ "$inventario" = 1 ] || { emite ilegivel; exit 0; }
emite limpo-comprovado
`)



/**
 * Hashes **literais** dos textos fixos (revisão consciente: mudar o texto exige mudar o hash aqui). `textosFixosIntegros`
 * compara o texto embutido com estes literais; não é uma comparação do texto consigo mesmo.
 */
export const HASH_SONDAGEM = '49c4bf734bf7c294068426222ee9adf2d800fa283558698411ef044d11f75de1'
export const HASH_LEITURA_ROOT = '9566ab96318f3c9fcba45dadb90cee00d17cd8a36da45990b3e40d14d1ced521'
