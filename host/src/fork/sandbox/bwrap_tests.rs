#![allow(clippy::unwrap_used)]
//! Testes puros rodam sempre. Os reais rodam onde o bwrap funciona; com
//! `MEXERICA_EXIGIR_BWRAP=1`, bwrap indisponível reprova em vez de pular.
use super::{
    bwrap::{
        BINARIO_PADRAO, Bwrap, COPIA_DENTRO, Entrada, Fds, HOME_DENTRO, Plano, abrir_ferramenta, conferir_montagens,
        diagnosticar, montagens_fixas, resolver_fixas, validar_ferramenta,
    },
    contrato::{Confianca, Estado, Execucao, Fim, Limites, Preparo, Provedor as _, Sandbox as _},
    local::PATH_PADRAO,
    montagem::{Areas, SandboxMontagem},
};
use std::{
    ffi::OsString,
    os::{fd::AsRawFd as _, unix::fs::PermissionsExt as _},
    path::{Path, PathBuf},
    time::Duration,
};

struct Lugar {
    base: PathBuf,
    copia: PathBuf,
    raiz: PathBuf,
}
impl Lugar {
    fn novo() -> Self {
        let base = std::env::temp_dir().join(format!("mx177-{}", uuid::Uuid::new_v4().simple()));
        let (copia, raiz) = (base.join("copia"), base.join("raiz"));
        std::fs::create_dir_all(&copia).unwrap();
        std::fs::create_dir_all(&raiz).unwrap();
        // A checagem de ancestrais recusa pasta gravável pelo grupo; não depender do umask.
        for pasta in [&base, &copia, &raiz] {
            std::fs::set_permissions(pasta, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self { base, copia, raiz }
    }
    fn provedor(&self) -> Bwrap {
        Bwrap::new(self.raiz.clone(), BINARIO_PADRAO.into(), PATH_PADRAO.into(), Vec::new()).unwrap()
    }
}
impl Drop for Lugar {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn exec(argv: &[&str]) -> Execucao {
    Execucao {
        argv: argv.iter().map(OsString::from).collect(),
        env: Vec::new(),
        pasta: PathBuf::new(),
        limites: Limites {
            tempo: Duration::from_secs(10),
            saida_bytes: 64 * 1024,
            carencia: Duration::from_millis(200),
        },
    }
}
fn sh(script: &str) -> Execucao {
    exec(&["/bin/sh", "-c", script])
}

/// Sandbox real ou `None` (com motivo) quando o bwrap não funciona nesta máquina.
async fn real(lugar: &Lugar) -> Option<SandboxMontagem> {
    let provedor = lugar.provedor();
    if let Err(motivo) = provedor.verificar() {
        assert!(std::env::var_os("MEXERICA_EXIGIR_BWRAP").is_none(), "bwrap exigido e indisponível: {motivo}");
        eprintln!("bwrap indisponível, teste real pulado: {motivo}");
        return None;
    }
    Some(provedor.preparar(&Preparo { copia: lugar.copia.clone(), confianca: Confianca::DoDono }).await.unwrap())
}

#[tokio::test]
async fn bwrap_recusa_agente_desconhecido_mesmo_disponivel() {
    let lugar = Lugar::novo();
    let erro = lugar
        .provedor()
        .preparar(&Preparo { copia: lugar.copia.clone(), confianca: Confianca::Desconhecida })
        .await
        .err()
        .unwrap();
    assert!(erro.to_string().contains("agente_nao_confiavel"));
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}

#[test]
fn argumentos_montam_so_o_necessario_isolam_processos_e_ancoram_em_descritores() {
    let lugar = Lugar::novo();
    let ferramenta = PathBuf::from("/usr/local/mexerica-ferramenta");
    Bwrap::new(lugar.raiz.clone(), BINARIO_PADRAO.into(), PATH_PADRAO.into(), vec![ferramenta.clone()]).unwrap();
    let raiz = [("bin", Entrada::Link("usr/bin".into())), ("lib64", Entrada::Pasta), ("lib32", Entrada::Ausente)];
    let fds = Fds { copia: 10, home: 11, tmp: 12, ferramentas: vec![(ferramenta, 13)] };
    let fixas = vec![
        ("/usr".into(), "/usr".into()),
        ("/lib64".into(), "/lib64".into()),
        ("/etc/ssl/certs".into(), "/etc/ssl/certs".into()),
        ("/usr/share/zoneinfo/UTC".into(), "/etc/localtime".into()),
    ];
    let mut e = exec(&["git", "status"]);
    e.pasta = "sub".into();
    let a: Vec<String> =
        Plano::argumentos(&fds, &e, &raiz, &fixas).iter().map(|s| s.to_string_lossy().into_owned()).collect();
    let tem = |trecho: &[&str]| a.windows(trecho.len()).any(|w| w == trecho);
    for flag in ["--die-with-parent", "--unshare-pid", "--unshare-user", "--disable-userns", "--new-session"] {
        assert!(tem(&[flag]), "{flag} ausente: {a:?}");
    }
    assert!(tem(&["--cap-drop", "ALL"]));
    assert!(tem(&["--symlink", "usr/bin", "/bin"]));
    assert!(tem(&["--ro-bind", "/usr", "/usr"]));
    assert!(tem(&["--ro-bind", "/lib64", "/lib64"]));
    // Origem resolvida montada no destino de sempre.
    assert!(tem(&["--ro-bind", "/usr/share/zoneinfo/UTC", "/etc/localtime"]));
    assert!(!a.iter().any(|x| x == "/lib32"));
    // Cópia, home, tmp e ferramentas entram por descritor, nunca pelo nome.
    assert!(tem(&["--bind-fd", "10", COPIA_DENTRO]));
    assert!(tem(&["--bind-fd", "11", HOME_DENTRO]));
    assert!(tem(&["--bind-fd", "12", "/tmp"]));
    assert!(tem(&["--ro-bind-fd", "13", "/usr/local/mexerica-ferramenta"]));
    assert!(!a.iter().any(|x| x == "--info-fd"), "sem término gracioso no bwrap");
    assert!(!a.iter().any(|x| x.contains("mx177-") || x == "--bind"), "origem por nome: {a:?}");
    // /etc só por certificados públicos e arquivos de configuração; /opt fora do padrão.
    assert!(tem(&["--ro-bind", "/etc/ssl/certs", "/etc/ssl/certs"]));
    assert!(!a.iter().any(|x| x == "/etc"
        || x == "/etc/ssl"
        || x == "/etc/pki"
        || x == "/etc/ca-certificates"
        || x == "/opt"
        || x == "/home"
        || x == "/root"
        || x == "/var"
        || x == "/run"));
    // `--remount-ro /` depois de TODAS as montagens (inclusive por descritor) e antes do chdir.
    let pos = |trecho: &[&str]| a.windows(trecho.len()).position(|w| w == trecho).unwrap();
    let remount = pos(&["--remount-ro", "/"]);
    for ultima in [
        pos(&["--bind-fd", "10", COPIA_DENTRO]),
        pos(&["--bind-fd", "12", "/tmp"]),
        pos(&["--ro-bind-fd", "13", "/usr/local/mexerica-ferramenta"]),
        pos(&["--ro-bind", "/usr", "/usr"]),
        pos(&["--proc", "/proc"]),
        pos(&["--dev", "/dev"]),
    ] {
        assert!(ultima < remount, "montagem depois do remount-ro: {a:?}");
    }
    assert!(remount < pos(&["--chdir", "/trabalho/sub"]));
    assert_eq!(a.iter().filter(|x| *x == "--remount-ro").count(), 1);
    assert!(tem(&["--chdir", "/trabalho/sub", "--", "git", "status"]));
    assert!(!tem(&["--share-net"]));
    // Só há um `--`, e o argv do agente vem inteiro depois dele.
    assert_eq!(a.iter().filter(|x| *x == "--").count(), 1);
    assert_eq!(a.last().map(String::as_str), Some("status"));
}

#[test]
fn ferramenta_nunca_expoe_home_login_dados_ou_areas_internas() {
    let home = Path::new("/home/usuario");
    let dados = Path::new("/home/usuario/.local/share/mexerica");
    let raiz = Path::new("/var/lib/sandbox");
    for f in [
        "relativa",
        "/",
        "/home",
        "/home/usuario",
        "/home/usuario/.cargo",
        "/home/usuario/.ssh/bin",
        "/home/usuario/.config/x/bin",
        "/opt/.aws",
        "/home/usuario/.local/share",
        "/home/usuario/.local/share/mexerica/bin",
        "/var/lib",
        "/var/lib/sandbox/sb_x",
        "/tmp/x",
        "/proc/self",
        "/trabalho",
        "/home/user/bin",
        "/usr/../home/usuario",
        // Fora da lista de raízes (SEG-3): sockets, estado do sistema, mídia e homes alheias.
        "/run/user/1000",
        "/run",
        "/var/lib/docker",
        "/var/run/docker",
        "/srv/toolchain",
        "/mnt/x",
        "/media/x",
        "/root/bin",
        "/sys/kernel",
        "/dev/shm",
        "/etc/x",
        "/home/shared/bin",
        "/usr",
        "/opt",
        "/nix/store",
        "/nix/storex/a",
    ] {
        assert!(validar_ferramenta(Path::new(f), Some(home), dados, raiz).is_err(), "{f} deveria ser recusada");
    }
    for f in [
        "/usr/local/mexerica-go",
        "/opt/mexerica-node/bin",
        "/nix/store/abc-tool",
        "/snap/mexerica/123",
        "/home/usuario/.cargo/bin",
        "/home/usuario/.rustup/toolchains",
    ] {
        assert!(validar_ferramenta(Path::new(f), Some(home), dados, raiz).is_ok(), "{f} deveria ser aceita");
    }
}

#[test]
fn etc_so_monta_certificados_publicos_e_configuracao() {
    let fixas = montagens_fixas(&[]);
    let tem = |p: &str| fixas.iter().any(|f| f == Path::new(p));
    for ok in [
        "/usr",
        "/etc/ssl/certs",
        "/etc/pki/tls/certs",
        "/etc/pki/ca-trust/extracted",
        "/etc/ca-certificates.conf",
        "/etc/resolv.conf",
        "/etc/hosts",
        "/etc/nsswitch.conf",
        "/etc/passwd",
        "/etc/group",
        "/etc/localtime",
        "/etc/ld.so.cache",
        "/etc/ld.so.conf",
        "/etc/ld.so.conf.d",
        "/etc/alternatives",
        "/etc/gitconfig",
    ] {
        assert!(tem(ok), "{ok} deveria ser montado");
    }
    // Árvores que podem guardar chaves privadas nunca entram inteiras.
    for nao in ["/etc", "/etc/ssl", "/etc/ssl/private", "/etc/pki", "/etc/ca-certificates", "/opt"] {
        assert!(!tem(nao), "{nao} não pode ser montado");
    }
}

#[test]
fn origem_fixa_que_e_link_para_dados_home_ou_areas_privadas_e_recusada() {
    let lugar = Lugar::novo();
    let (dados, home) = (lugar.base.join("dados"), lugar.base.join("home-do-hub"));
    for pasta in [&dados, &home] {
        std::fs::create_dir(pasta).unwrap();
    }
    let outros = Path::new("/nao/existe/dados");
    // `ssl` aponta para o alvo protegido; o texto da origem não revela nada.
    for (alvo, esperado_dados, esperado_home, esperada_raiz) in [
        (&dados, dados.as_path(), Some(Path::new("/nao/existe/home")), Path::new("/nao/existe/raiz")),
        (&home, outros, Some(home.as_path()), Path::new("/nao/existe/raiz")),
        (&lugar.raiz, outros, Some(Path::new("/nao/existe/home")), lugar.raiz.as_path()),
    ] {
        let link = lugar.base.join(format!("ssl-{}", uuid::Uuid::new_v4().simple()));
        std::os::unix::fs::symlink(alvo, &link).unwrap();
        let erro = resolver_fixas(&[link], esperado_home, esperado_dados, esperada_raiz).unwrap_err();
        assert!(erro.to_string().contains("montagem_expoe_dados"), "{alvo:?}: {erro}");
    }
}

#[test]
fn origem_fixa_so_passa_se_origem_e_ancestrais_nao_sao_do_hub_nem_gravaveis() {
    if crate::fork::private_fs::uid() == 0 {
        return; // "não é do usuário do Hub" não distingue nada para root; o Hub recusa rodar assim
    }
    let lugar = Lugar::novo();
    let (dados, raiz) = (Path::new("/nao/existe/dados"), Path::new("/nao/existe/raiz"));
    // Pasta do próprio usuário: um concorrente com o mesmo UID poderia trocá-la; recusada.
    let propria = lugar.base.join("propria");
    std::fs::create_dir(&propria).unwrap();
    let erro = resolver_fixas(std::slice::from_ref(&propria), None, dados, raiz).unwrap_err();
    assert!(erro.to_string().contains("montagem_insegura"), "{erro}");
    // Link de origem para pasta do usuário também (é o resolvido que vale).
    let atalho = lugar.base.join("atalho");
    std::os::unix::fs::symlink(&propria, &atalho).unwrap();
    assert!(resolver_fixas(&[atalho], None, dados, raiz).unwrap_err().to_string().contains("montagem_insegura"));
    // Origem do sistema passa e vem resolvida; ausente é ignorada.
    let fixas = resolver_fixas(&[PathBuf::from("/usr"), lugar.base.join("ausente")], None, dados, raiz).unwrap();
    assert_eq!(fixas, [(PathBuf::from("/usr").canonicalize().unwrap(), PathBuf::from("/usr"))]);
}

#[test]
fn ferramenta_e_montagens_fixas_recusam_dados_sob_opt_e_usr_local() {
    let home = Path::new("/home/usuario");
    let host = [("lib", Entrada::Pasta), ("bin", Entrada::Link("usr/bin".into()))];
    let fixas = montagens_fixas(&host);
    // Fora do padrão: /opt não é mais montado, então dados sob /opt são seguros.
    let (dados, raiz) = (Path::new("/opt/mexerica-dados"), Path::new("/opt/sandboxes"));
    assert!(conferir_montagens(&fixas, Some(home), dados, raiz).is_ok());
    // Mas uma ferramenta não cobre nem entra neles.
    for f in ["/opt/mexerica-dados", "/opt/mexerica-dados/bin", "/opt/sandboxes/x", "/opt"] {
        assert!(validar_ferramenta(Path::new(f), Some(home), dados, raiz).is_err(), "{f}");
    }
    assert!(validar_ferramenta(Path::new("/opt/mexerica-node"), Some(home), dados, raiz).is_ok());
    // Montagem fixa que cobre dados, raiz ou home é erro (não é pulada).
    for (dados, raiz, home) in [
        ("/usr/local/mexerica-dados", "/var/lib/sb", "/home/usuario"),
        ("/home/usuario/.local/share/mexerica", "/usr/local/sandboxes", "/home/usuario"),
        ("/etc/ssl/certs/mexerica", "/var/lib/sb", "/home/usuario"),
        ("/etc/ssl", "/var/lib/sb", "/home/usuario"),
        ("/usr", "/var/lib/sb", "/home/usuario"),
        ("/home/usuario/.local/share/mexerica", "/var/lib/sb", "/usr/local/home/usuario"),
        ("/lib/mexerica", "/var/lib/sb", "/home/usuario"),
        ("/home/usuario/d", "/etc", "/home/usuario"),
    ] {
        let erro = conferir_montagens(&fixas, Some(Path::new(home)), Path::new(dados), Path::new(raiz)).unwrap_err();
        assert!(erro.to_string().contains("montagem_expoe_dados"), "{dados} {raiz} {home}: {erro}");
    }
    for f in ["/usr/local", "/usr/local/mexerica-dados/bin"] {
        let (dados, raiz) = (Path::new("/usr/local/mexerica-dados"), Path::new("/var/lib/sb"));
        assert!(validar_ferramenta(Path::new(f), Some(home), dados, raiz).is_err(), "{f}");
    }
}

#[test]
fn ferramenta_com_link_e_recusada_na_abertura_e_na_validacao() {
    let lugar = Lugar::novo();
    let real = lugar.base.join("ferr");
    std::fs::create_dir_all(real.join("bin")).unwrap();
    std::fs::write(real.join("bin/segredo"), "x").unwrap();
    std::fs::set_permissions(real.join("bin"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(abrir_ferramenta(&real.join("bin")).is_ok());
    // Link no próprio caminho e link num ancestral (ex.: ~/.cargo/bin -> ~/.ssh).
    std::os::unix::fs::symlink(real.join("bin"), lugar.base.join("atalho")).unwrap();
    std::os::unix::fs::symlink(&real, lugar.base.join("ancestral")).unwrap();
    for f in [lugar.base.join("atalho"), lugar.base.join("ancestral/bin")] {
        let erro = abrir_ferramenta(&f).unwrap_err();
        assert!(erro.to_string().contains("ferramenta_recusada"), "{f:?}: {erro}");
        // A validação de configuração também recusa (a home do Hub aqui é o próprio `base`).
        let erro = validar_ferramenta(&f, Some(&lugar.base), Path::new("/x/dados"), Path::new("/x/raiz")).unwrap_err();
        assert!(erro.to_string().contains("ferramenta_com_link"), "{f:?}: {erro}");
    }
    // Pasta inexistente: recusada.
    assert!(abrir_ferramenta(&lugar.base.join("nao-existe")).is_err());
}

#[test]
fn areas_reabertas_recusam_copia_trocada_por_link_ou_por_outra_pasta() {
    let lugar = Lugar::novo();
    let areas = Areas::criar(&lugar.raiz, &lugar.copia).unwrap();
    assert!(areas.abrir().is_ok());
    // Home do Hub (alvo do ataque) no lugar da cópia, depois de preparar.
    let alvo = lugar.base.join("home-do-hub");
    std::fs::create_dir(&alvo).unwrap();
    std::fs::set_permissions(&alvo, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::rename(&lugar.copia, lugar.base.join("copia-original")).unwrap();
    std::os::unix::fs::symlink(&alvo, &lugar.copia).unwrap();
    let erro = areas.abrir().err().unwrap();
    assert!(erro.to_string().contains("copia_trocada"), "{erro}");
    // Outra pasta real no mesmo caminho também não é o objeto conferido.
    std::fs::remove_file(&lugar.copia).unwrap();
    std::fs::create_dir(&lugar.copia).unwrap();
    std::fs::set_permissions(&lugar.copia, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(areas.abrir().err().unwrap().to_string().contains("copia_trocada"));
    // O mesmo vale para home e tmp privados.
    std::fs::remove_dir(&lugar.copia).unwrap();
    std::fs::rename(lugar.base.join("copia-original"), &lugar.copia).unwrap();
    assert!(areas.abrir().is_ok());
    std::fs::rename(&areas.home, lugar.base.join("home-original")).unwrap();
    std::os::unix::fs::symlink(&alvo, &areas.home).unwrap();
    assert!(areas.abrir().err().unwrap().to_string().contains("home_trocada"));
}

/// bwrap de mentira: não isola nada, mas confere o contrato com o host (descritores herdados e
/// capacidades da sonda) em qualquer máquina com /proc. `REJEITA` é uma opção que esta versão
/// "antiga" não conhece (vazio = aceita tudo).
const BWRAP_FALSO: &str = r#"#!/bin/sh
REJEITA="@REJEITA@"
if [ -n "$REJEITA" ]; then
  case " $* " in
    *" $REJEITA "*) echo "bwrap: Unknown option $REJEITA" >&2; exit 1;;
  esac
fi
# Como o bwrap real, cada descritor é consumido (fechado) ao montar: reusar falha.
USADOS=" "
consumir() {
  case "$USADOS" in *" $1 "*) echo "bwrap: Can't stat fd $1: Bad file descriptor" >&2; exit 1;; esac
  [ -e "/proc/self/fd/$1" ] || { echo "bwrap: Can't stat fd $1: Bad file descriptor" >&2; exit 1; }
  USADOS="$USADOS$1 "
}
while [ $# -gt 0 ]; do
  case "$1" in
    --bind-fd) consumir "$2"; echo "bind $(readlink /proc/self/fd/$2) $3"; shift 3;;
    --ro-bind-fd) consumir "$2"; echo "robind $(readlink /proc/self/fd/$2) $3"; shift 3;;
    --) shift; break;;
    *) shift;;
  esac
done
"#;

fn bwrap_falso_que_rejeita(lugar: &Lugar, rejeita: &str) -> Bwrap {
    let binario = lugar.base.join("bwrap-falso");
    std::fs::write(&binario, BWRAP_FALSO.replace("@REJEITA@", rejeita)).unwrap();
    std::fs::set_permissions(&binario, std::fs::Permissions::from_mode(0o755)).unwrap();
    Bwrap::new(lugar.raiz.clone(), binario, PATH_PADRAO.into(), Vec::new()).unwrap()
}

fn bwrap_falso(lugar: &Lugar) -> Bwrap {
    bwrap_falso_que_rejeita(lugar, "")
}

/// Root é recusado por `verificar`; os testes com bwrap falso só fazem sentido para usuário comum.
fn root() -> bool {
    crate::fork::private_fs::uid() == 0
}

#[test]
fn sonda_exige_as_opcoes_por_descritor_que_o_lancamento_usa() {
    if root() {
        return;
    }
    let lugar = Lugar::novo();
    assert!(bwrap_falso(&lugar).verificar().is_ok());
    // Versão antiga: passaria na sonda de antes (sem descritores), mas não executa o plano final.
    for opcao in ["--bind-fd", "--ro-bind-fd"] {
        let erro = bwrap_falso_que_rejeita(&lugar, opcao).verificar().unwrap_err();
        assert!(erro.motivo.contains(opcao), "{erro}");
        assert!(erro.correcao.as_deref().unwrap().contains("0.10"), "{erro}");
    }
    // A pasta temporária da sonda não fica para trás.
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
    // E o preparo falha como Indisponivel, sem criar áreas.
    let erro = futures_lite_block(
        bwrap_falso_que_rejeita(&lugar, "--bind-fd")
            .preparar(&Preparo { copia: lugar.copia.clone(), confianca: Confianca::DoDono }),
    );
    assert!(erro.err().unwrap().to_string().contains("indisponível"));
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}

fn futures_lite_block<T>(futuro: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(futuro)
}

#[tokio::test]
async fn descritores_chegam_ao_bwrap_e_o_pai_nao_deixa_fd_herdavel() {
    if root() {
        return;
    }
    let lugar = Lugar::novo();
    let mut sb = bwrap_falso(&lugar)
        .preparar(&Preparo { copia: lugar.copia.clone(), confianca: Confianca::DoDono })
        .await
        .unwrap();
    let saida = sb.executar(&exec(&["nada"])).await.unwrap();
    assert_eq!(saida.fim, Fim::Saiu(0));
    let texto = String::from_utf8(saida.stdout).unwrap();
    let canonico = |p: &Path| p.canonicalize().unwrap();
    assert!(texto.contains(&format!("bind {} /trabalho", canonico(&lugar.copia).display())), "{texto}");
    assert!(texto.lines().any(|l| l.starts_with("bind ") && l.ends_with(" /home/user")), "{texto}");
    assert!(texto.lines().any(|l| l.starts_with("bind ") && l.ends_with(" /tmp")), "{texto}");
    // Nenhum descritor da montagem fica sem CLOEXEC no pai (outros spawns do Hub os herdariam).
    let areas = Areas::criar(&lugar.raiz, &lugar.copia).unwrap();
    let plano = bwrap_falso(&lugar);
    let lancamento = plano.plano().comando(areas.abrir().unwrap(), &exec(&["nada"])).unwrap();
    for arquivo in &lancamento.manter {
        // SAFETY: consulta de flags de um descritor vivo.
        let flags = unsafe { libc::fcntl(arquivo.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0, "fd {} herdável no pai", arquivo.as_raw_fd());
    }
    assert_eq!(lancamento.manter.len(), 3);
}

#[tokio::test]
async fn bwrap_recusa_pasta_que_e_link_para_fora_da_copia() {
    if root() {
        return;
    }
    let lugar = Lugar::novo();
    let mut sb = bwrap_falso(&lugar)
        .preparar(&Preparo { copia: lugar.copia.clone(), confianca: Confianca::DoDono })
        .await
        .unwrap();
    std::os::unix::fs::symlink(&lugar.base, lugar.copia.join("atalho")).unwrap();
    let mut e = exec(&["nada"]);
    e.pasta = "atalho".into();
    let erro = sb.executar(&e).await.unwrap_err();
    assert!(erro.to_string().contains("pasta_fora_da_copia"), "{erro}");
    assert_eq!(sb.estado(), Estado::Preparado, "nada foi lançado");
}

#[test]
fn indisponivel_diz_como_corrigir_sem_trocar_de_executor() {
    let caso = diagnosticar("bwrap: Can't read /proc/sys/kernel/overflowuid: No such file or directory\n");
    assert!(caso.motivo.contains("overflowuid"));
    assert!(caso.correcao.unwrap().contains("/proc completo"));
    assert!(diagnosticar("bwrap: Unknown option --bind-fd").correcao.unwrap().contains("0.10"));
    assert!(diagnosticar("bwrap: setting up uid map: Permission denied").correcao.unwrap().contains("namespaces"));
    assert!(diagnosticar("").correcao.is_some());
}

/// Arquivo criado no home real do Hub; removido ao sair do escopo, mesmo se o teste falhar.
struct Marcador(PathBuf);
impl Drop for Marcador {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[tokio::test]
async fn real_home_do_hub_e_dados_invisiveis_e_sistema_somente_leitura() {
    let lugar = Lugar::novo();
    let Some(mut sb) = real(&lugar).await else { return };
    let home = dirs::home_dir().unwrap();
    let dados = crate::service::data_dir();
    // O home real pode ter o mesmo caminho do home privado (HOME_DENTRO, ex.: runner com usuário
    // "user"): o caminho existe nos dois, então a prova é o CONTEÚDO: um marcador criado no home
    // real nunca pode aparecer dentro, e nada escrito dentro pode chegar ao home real.
    let nome = format!(".mx-marcador-{}", uuid::Uuid::new_v4().simple());
    let marcador = Marcador(home.join(&nome));
    std::fs::write(&marcador.0, "segredo-do-home-real").unwrap();
    let mut e = sh(r#"for p in "$2" "$3"; do test ! -e "$p" || { echo "visivel $p"; exit 1; }; done
        test ! -e "$1/$4" || { echo "marcador-do-home-real-visivel"; exit 1; }
        test "$1" = "$HOME" || test ! -e "$1" || { echo "visivel $1"; exit 1; }
        test -z "$(ls -A "$HOME")" || { echo "home-privado-nao-vazio"; exit 1; }
        touch /usr/proibido 2>/dev/null && { echo escreveu-usr; exit 1; }
        touch /etc/proibido 2>/dev/null && { echo escreveu-etc; exit 1; }
        touch /proibido 2>/dev/null && { echo escreveu-raiz; exit 1; }
        mkdir /novo 2>/dev/null && { echo criou-na-raiz; exit 1; }
        echo ok > /trabalho/resultado && echo h > "$HOME/$4.dentro" && echo t > "$TMPDIR/t""#);
    e.argv.extend([
        "sh".into(),
        home.clone().into(),
        dados.into(),
        lugar.base.join("raiz").into(),
        nome.clone().into(),
    ]);
    let saida = sb.executar(&e).await.unwrap();
    assert_eq!(saida.fim, Fim::Saiu(0), "{}", String::from_utf8_lossy(&saida.stdout));
    assert_eq!(std::fs::read_to_string(lugar.copia.join("resultado")).unwrap(), "ok\n");
    assert_eq!(std::fs::read_to_string(&marcador.0).unwrap(), "segredo-do-home-real");
    assert!(!home.join(format!("{nome}.dentro")).exists(), "escrita dentro do sandbox chegou ao home real");
}

#[tokio::test]
async fn real_ambiente_exato_sem_privilegio_novo() {
    let lugar = Lugar::novo();
    let Some(mut sb) = real(&lugar).await else { return };
    let mut e = sh("env; grep NoNewPrivs /proc/self/status; sudo -n true 2>/dev/null && echo SUDO_OK");
    e.env = vec![("PEDIDO".into(), "1".into())];
    let saida = sb.executar(&e).await.unwrap();
    let texto = String::from_utf8(saida.stdout).unwrap();
    let mut chaves: Vec<_> = texto
        .lines()
        .filter(|l| !l.starts_with("NoNewPrivs"))
        .filter_map(|l| l.split_once('='))
        .map(|(k, _)| k)
        .collect();
    chaves.sort_unstable();
    chaves.retain(|k| *k != "PWD" && *k != "SHLVL" && *k != "_");
    assert_eq!(chaves, ["HOME", "LANG", "PATH", "PEDIDO", "TMPDIR"], "{texto}");
    assert!(texto.contains(&format!("HOME={HOME_DENTRO}")));
    assert!(texto.lines().any(|l| l.starts_with("NoNewPrivs:") && l.ends_with('1')), "{texto}");
    assert!(!texto.contains("SUDO_OK"));
}

#[tokio::test]
async fn real_processo_escapado_por_setsid_morre_com_a_execucao() {
    let lugar = Lugar::novo();
    let Some(mut sb) = real(&lugar).await else { return };
    let saida = sb
        .executar(&sh(
            "setsid sh -c 'while :; do echo x >> /trabalho/vivo; sleep 0.05; done' </dev/null >/dev/null 2>&1 & sleep 0.3; echo fim",
        ))
        .await
        .unwrap();
    assert_eq!(saida.fim, Fim::Saiu(0));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let antes = std::fs::metadata(lugar.copia.join("vivo")).unwrap().len();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(std::fs::metadata(lugar.copia.join("vivo")).unwrap().len(), antes, "escapado continuou escrevendo");
}

#[tokio::test]
async fn real_prazo_para_tudo_dentro_do_bwrap() {
    let lugar = Lugar::novo();
    let Some(mut sb) = real(&lugar).await else { return };
    let mut e = sh("sleep 30 & sleep 30");
    e.limites.tempo = Duration::from_millis(300);
    let saida = sb.executar(&e).await.unwrap();
    assert_eq!(saida.fim, Fim::TempoEsgotado);
    assert!(saida.duracao < Duration::from_secs(5));
    sb.encerrar().await.unwrap();
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}

#[tokio::test]
async fn real_prazo_mata_tudo_na_hora_inclusive_worker_com_trap_term() {
    let lugar = Lugar::novo();
    let Some(mut sb) = real(&lugar).await else { return };
    // No bwrap o prazo é SIGKILL imediato: o handler de TERM do worker nunca termina o trabalho.
    let mut e = sh("(trap 'sleep 2; echo salvo > /trabalho/marca' TERM; while :; do sleep 0.05; done) & wait");
    e.limites = Limites { tempo: Duration::from_millis(300), saida_bytes: 4096, carencia: Duration::from_secs(5) };
    let saida = sb.executar(&e).await.unwrap();
    assert_eq!(saida.fim, Fim::TempoEsgotado);
    assert!(saida.duracao < Duration::from_secs(2), "sem carência no bwrap: {:?}", saida.duracao);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!lugar.copia.join("marca").exists(), "worker sobreviveu ao prazo");
}

#[tokio::test]
async fn real_ferramenta_com_link_e_pasta_com_link_sao_recusadas_na_execucao() {
    let lugar = Lugar::novo();
    let Some(mut sb) = real(&lugar).await else { return };
    std::os::unix::fs::symlink(&lugar.base, lugar.copia.join("atalho")).unwrap();
    let mut e = sh("true");
    e.pasta = "atalho".into();
    assert!(sb.executar(&e).await.unwrap_err().to_string().contains("pasta_fora_da_copia"));
}
