#![allow(clippy::unwrap_used)]
use super::{
    contrato::{
        Confianca, Contencao, Estado, Execucao, Fim, Limites, Passo, Preparo, Provedor as _, Sandbox as _, admitir,
        avancar,
    },
    local::{PATH_PADRAO, ProcessoLocal},
    montagem::SandboxMontagem,
};
use std::{
    ffi::OsString,
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
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
        let base = std::env::temp_dir().join(format!("mx175-{}", uuid::Uuid::new_v4().simple()));
        let (copia, raiz) = (base.join("copia"), base.join("raiz"));
        std::fs::create_dir_all(&copia).unwrap();
        std::fs::create_dir_all(&raiz).unwrap();
        // A checagem de ancestrais recusa pasta gravável pelo grupo; não depender do umask.
        for pasta in [&base, &copia, &raiz] {
            std::fs::set_permissions(pasta, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self { base, copia, raiz }
    }
    async fn sandbox(&self) -> SandboxMontagem {
        let provedor = ProcessoLocal::new(self.raiz.clone(), PATH_PADRAO.into()).unwrap();
        provedor.preparar(&Preparo { copia: self.copia.clone(), confianca: Confianca::DoDono }).await.unwrap()
    }
}
impl Drop for Lugar {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn limites(tempo_ms: u64) -> Limites {
    Limites { tempo: Duration::from_millis(tempo_ms), saida_bytes: 64 * 1024, carencia: Duration::from_millis(200) }
}
fn exec(argv: &[&str]) -> Execucao {
    Execucao {
        argv: argv.iter().map(OsString::from).collect(),
        env: Vec::new(),
        pasta: PathBuf::new(),
        limites: limites(10_000),
    }
}
fn sh(script: &str) -> Execucao {
    exec(&["/bin/sh", "-c", script])
}
fn vivo(pid: i32) -> bool {
    // SAFETY: sinal 0 só consulta existência.
    unsafe { libc::kill(pid, 0) == 0 }
}
async fn esperar_morte(pid: i32) -> bool {
    for _ in 0..50 {
        if !vivo(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[test]
fn desconhecido_so_em_maquina_descartavel() {
    assert!(admitir(Confianca::Desconhecida, Contencao::Nenhuma).is_err());
    assert!(admitir(Confianca::Desconhecida, Contencao::ArquivosEAmbiente).is_err());
    assert!(admitir(Confianca::Desconhecida, Contencao::MaquinaDescartavel).is_ok());
    for contencao in [Contencao::Nenhuma, Contencao::ArquivosEAmbiente, Contencao::MaquinaDescartavel] {
        assert!(admitir(Confianca::DoDono, contencao).is_ok());
    }
}

#[tokio::test]
async fn provedor_local_recusa_agente_desconhecido_antes_de_criar_areas() {
    let lugar = Lugar::novo();
    let provedor = ProcessoLocal::new(lugar.raiz.clone(), PATH_PADRAO.into()).unwrap();
    let erro = provedor
        .preparar(&Preparo { copia: lugar.copia.clone(), confianca: Confianca::Desconhecida })
        .await
        .err()
        .unwrap();
    assert!(erro.to_string().contains("agente_nao_confiavel"));
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}

#[test]
fn execucao_recusa_env_reservada_caminho_fora_e_limites_fora_da_faixa() {
    let mut e = exec(&["/bin/true"]);
    assert!(e.validar().is_ok());
    for chave in ["HOME", "PATH", "TMPDIR", "LD_PRELOAD", "DYLD_INSERT_LIBRARIES", "SHELL"] {
        e.env = vec![(chave.into(), "x".into())];
        assert!(e.validar().is_err(), "{chave} deveria ser reservada");
    }
    for chave in ["", "minuscula", "1X", "A-B", "A B"] {
        e.env = vec![(chave.into(), "x".into())];
        assert!(e.validar().is_err(), "{chave:?} deveria ser recusada");
    }
    e.env = vec![("A".into(), "1".into()), ("A".into(), "2".into())];
    assert!(e.validar().is_err());
    e.env.clear();
    for pasta in ["/etc", "..", "a/../..", "./a"] {
        e.pasta = pasta.into();
        assert!(e.validar().is_err(), "{pasta} deveria ser recusada");
    }
    e.pasta = PathBuf::new();
    e.argv = Vec::new();
    assert!(e.validar().is_err());
    e.argv = vec!["/bin/true".into()];
    for l in [
        Limites { tempo: Duration::ZERO, ..limites(1000) },
        Limites { tempo: Limites::TEMPO_MAX + Duration::from_secs(1), ..limites(1000) },
        Limites { saida_bytes: 0, ..limites(1000) },
        Limites { saida_bytes: Limites::SAIDA_MAX + 1, ..limites(1000) },
        Limites { carencia: Limites::CARENCIA_MAX + Duration::from_secs(1), ..limites(1000) },
    ] {
        e.limites = l;
        assert!(e.validar().is_err(), "{l:?}");
    }
}

#[test]
fn ciclo_de_vida_nunca_repete_execucao_incerta_nem_reabre_encerrado() {
    use {Estado as E, Passo as P};
    assert_eq!(avancar(E::Preparado, P::Executar).unwrap(), E::Executando);
    assert_eq!(avancar(E::Executando, P::FimExecucao).unwrap(), E::Preparado);
    assert_eq!(avancar(E::Executando, P::Falha).unwrap(), E::Desconhecido);
    assert!(avancar(E::Executando, P::Executar).is_err());
    assert!(avancar(E::Executando, P::Copiar).is_err());
    assert!(avancar(E::Desconhecido, P::Executar).is_err());
    assert!(avancar(E::Desconhecido, P::Copiar).is_err());
    assert_eq!(avancar(E::Desconhecido, P::Encerrar).unwrap(), E::Encerrado);
    for passo in [P::Executar, P::FimExecucao, P::Copiar, P::Falha, P::Encerrar] {
        assert!(avancar(E::Encerrado, passo).is_err());
    }
}

#[tokio::test]
async fn ambiente_e_so_o_do_provedor_mais_o_pedido_com_home_e_tmp_privados() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let mut e = exec(&["/usr/bin/env"]);
    e.env = vec![("PEDIDO".into(), "1".into())];
    let saida = sb.executar(&e).await.unwrap();
    assert_eq!(saida.fim, Fim::Saiu(0));
    let texto = String::from_utf8(saida.stdout).unwrap();
    let mut chaves: Vec<_> = texto.lines().filter_map(|l| l.split_once('=')).map(|(k, _)| k).collect();
    chaves.sort_unstable();
    assert_eq!(chaves, ["HOME", "LANG", "PATH", "PEDIDO", "TMPDIR"], "{texto}");
    let home = texto.lines().find_map(|l| l.strip_prefix("HOME=")).unwrap();
    let tmp = texto.lines().find_map(|l| l.strip_prefix("TMPDIR=")).unwrap();
    for pasta in [home, tmp] {
        assert!(Path::new(pasta).starts_with(&lugar.raiz), "{pasta}");
        assert_eq!(std::fs::metadata(pasta).unwrap().permissions().mode() & 0o777, 0o700);
    }
    assert_ne!(Some(home), std::env::var("HOME").ok().as_deref());
    assert_eq!(sb.estado(), Estado::Preparado);
}

#[tokio::test]
async fn prazo_para_o_grupo_inteiro() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let mut e = sh("sleep 30 & echo $! > neto; sleep 30");
    e.limites = limites(300);
    let saida = sb.executar(&e).await.unwrap();
    assert_eq!(saida.fim, Fim::TempoEsgotado);
    assert!(saida.duracao < Duration::from_secs(5));
    let neto: i32 = std::fs::read_to_string(lugar.copia.join("neto")).unwrap().trim().parse().unwrap();
    assert!(esperar_morte(neto).await, "neto {neto} sobreviveu ao prazo");
}

#[tokio::test]
async fn watchdog_mata_filho_deixado_no_grupo_e_saida_nao_trava() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    // O neto herda stdout: sem o watchdog, a leitura esperaria 30 s.
    let saida = sb.executar(&sh("sleep 30 & echo $! > neto; echo pronto")).await.unwrap();
    assert_eq!(saida.fim, Fim::Saiu(0));
    assert_eq!(saida.stdout, b"pronto\n");
    assert!(!saida.incompleta);
    assert!(saida.duracao < Duration::from_secs(5));
    let neto: i32 = std::fs::read_to_string(lugar.copia.join("neto")).unwrap().trim().parse().unwrap();
    assert!(esperar_morte(neto).await, "neto {neto} sobreviveu ao fim");
}

#[tokio::test]
async fn saida_respeita_teto_somado() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let mut e = sh("head -c 100000 /dev/zero; head -c 100000 /dev/zero >&2");
    e.limites.saida_bytes = 1024;
    let saida = sb.executar(&e).await.unwrap();
    assert_eq!(saida.fim, Fim::Saiu(0));
    assert!(saida.truncada);
    assert_eq!(saida.stdout.len() + saida.stderr.len(), 1024);
}

#[tokio::test]
async fn copiar_para_fora_recusa_link_hardlink_e_sobrescrita() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let segredo = lugar.base.join("segredo");
    std::fs::write(&segredo, "nao sai").unwrap();
    std::os::unix::fs::symlink(&segredo, lugar.copia.join("link")).unwrap();
    std::fs::hard_link(&segredo, lugar.copia.join("duro")).unwrap();
    std::fs::create_dir(lugar.copia.join("sub")).unwrap();
    std::os::unix::fs::symlink(&lugar.base, lugar.copia.join("sub/pasta")).unwrap();
    std::fs::write(lugar.base.join("fora"), "x").unwrap();
    std::fs::write(lugar.copia.join("ok"), "resultado").unwrap();
    let destino = lugar.base.join("exportado");
    for origem in ["link", "duro", "sub/pasta/segredo", "../segredo", "/etc/passwd"] {
        assert!(sb.copiar_para_fora(Path::new(origem), &destino).await.is_err(), "{origem}");
        assert!(!destino.exists(), "{origem} vazou");
    }
    assert!(sb.copiar_para_fora(Path::new("ok"), &lugar.base.join("fora")).await.is_err());
    assert_eq!(std::fs::read_to_string(lugar.base.join("fora")).unwrap(), "x");
    sb.copiar_para_fora(Path::new("ok"), &destino).await.unwrap();
    assert_eq!(std::fs::read_to_string(&destino).unwrap(), "resultado");
}

#[tokio::test]
async fn copiar_para_dentro_nao_segue_pasta_trocada_por_link() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let origem = lugar.base.join("entrada");
    std::fs::write(&origem, "dado").unwrap();
    let fora = lugar.base.join("fora");
    std::fs::create_dir(&fora).unwrap();
    std::os::unix::fs::symlink(&fora, lugar.copia.join("atalho")).unwrap();
    assert!(sb.copiar_para_dentro(&origem, Path::new("atalho/x")).await.is_err());
    assert!(sb.copiar_para_dentro(&origem, Path::new("../x")).await.is_err());
    assert!(!fora.join("x").exists() && !lugar.base.join("x").exists());
    sb.copiar_para_dentro(&origem, Path::new("novo/dir/x")).await.unwrap();
    assert_eq!(std::fs::read_to_string(lugar.copia.join("novo/dir/x")).unwrap(), "dado");
}

#[tokio::test]
async fn encerrar_apaga_areas_privadas_preserva_copia_e_fecha_o_sandbox() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    sb.executar(&sh("echo feito > resultado; echo x > \"$HOME/h\"")).await.unwrap();
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 1);
    sb.encerrar().await.unwrap();
    assert_eq!(sb.estado(), Estado::Encerrado);
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
    assert_eq!(std::fs::read_to_string(lugar.copia.join("resultado")).unwrap(), "feito\n");
    assert!(sb.executar(&exec(&["/bin/true"])).await.is_err());
    assert!(sb.copiar_para_fora(Path::new("resultado"), &lugar.base.join("r")).await.is_err());
    assert!(sb.encerrar().await.is_err());
}

#[tokio::test]
async fn falha_de_execucao_deixa_estado_desconhecido_sem_reexecutar() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    assert!(sb.executar(&exec(&["/caminho/que/nao/existe"])).await.is_err());
    assert_eq!(sb.estado(), Estado::Desconhecido);
    assert!(sb.executar(&exec(&["/bin/true"])).await.is_err());
    sb.encerrar().await.unwrap();
}

#[tokio::test]
async fn sandbox_abandonado_nao_deixa_areas_privadas() {
    let lugar = Lugar::novo();
    drop(lugar.sandbox().await);
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}

#[test]
fn path_do_toolchain_so_aceita_pastas_absolutas() {
    let raiz = std::env::temp_dir();
    for path in ["", "bin", "/usr/bin:", "/usr/bin::/bin", "/usr/../tmp", "/usr/bin:."] {
        assert!(ProcessoLocal::new(raiz.clone(), path.into()).is_err(), "{path:?}");
    }
    assert!(ProcessoLocal::new(raiz, PATH_PADRAO.into()).is_ok());
}

#[tokio::test]
async fn cancelar_executar_mata_o_grupo_e_deixa_estado_desconhecido() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    // O neto herda os pipes: sem o guarda, sobreviveria e os leitores ficariam soltos.
    let e = sh("sleep 30 & echo $! > neto; wait");
    assert!(tokio::time::timeout(Duration::from_millis(800), sb.executar(&e)).await.is_err());
    let neto: i32 = std::fs::read_to_string(lugar.copia.join("neto")).unwrap().trim().parse().unwrap();
    assert!(esperar_morte(neto).await, "neto {neto} sobreviveu ao cancelamento");
    assert_eq!(sb.estado(), Estado::Desconhecido);
    assert!(sb.executar(&exec(&["/bin/true"])).await.is_err());
    sb.encerrar().await.unwrap();
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}

#[tokio::test]
async fn carencia_chega_aos_processos_efetivos_mesmo_com_o_lider_fora() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    // O líder morre no SIGTERM; o worker precisa de 0,3 s para salvar. SIGKILL só após a carência.
    let mut e = sh(r"(trap 'sleep 0.3; echo salvo > marca; exit 0' TERM; while :; do sleep 0.05; done) & wait");
    e.limites = Limites { tempo: Duration::from_millis(300), saida_bytes: 4096, carencia: Duration::from_secs(2) };
    let saida = sb.executar(&e).await.unwrap();
    assert_eq!(saida.fim, Fim::TempoEsgotado);
    assert_eq!(std::fs::read_to_string(lugar.copia.join("marca")).unwrap(), "salvo\n");
    assert!(saida.duracao >= Duration::from_secs(2), "carência inteira: {:?}", saida.duracao);
}

// O líder só sai depois de o filho confirmar a sessão própria. Sem esse handshake,
// a limpeza do grupo pode matar o filho antes do escape e invalidar a fixture.
#[cfg(target_os = "linux")]
fn processo_escapado_com_pipes() -> Execucao {
    exec(&[
        "/usr/bin/python3",
        "-c",
        r#"
import os
import time

leitura, escrita = os.pipe()
if os.fork() == 0:
    os.close(leitura)
    os.setsid()
    with open("escapado", "w") as arquivo:
        arquivo.write(str(os.getpid()))
    os.write(escrita, b"1")
    os.close(escrita)
    time.sleep(20)
    os._exit(0)

os.close(escrita)
if os.read(leitura, 1) != b"1":
    raise RuntimeError("filho não confirmou a sessão própria")
os.close(leitura)
print("pronto", flush=True)
"#,
    ])
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn escapado_segurando_os_dois_pipes_limita_a_drenagem_a_carencia() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let mut e = processo_escapado_com_pipes();
    e.limites = Limites { tempo: Duration::from_secs(10), saida_bytes: 4096, carencia: Duration::from_millis(400) };
    let saida = sb.executar(&e).await.unwrap();
    let escapado: i32 = std::fs::read_to_string(lugar.copia.join("escapado")).unwrap().trim().parse().unwrap();
    // SAFETY: encerra o escapado deste teste.
    unsafe { libc::kill(escapado, libc::SIGKILL) };
    assert_eq!(saida.fim, Fim::Saiu(0));
    assert!(saida.incompleta);
    assert_eq!(saida.stdout, b"pronto\n");
    assert!(saida.stderr.is_empty(), "{:?}", String::from_utf8_lossy(&saida.stderr));
    // Um prazo só (a carência) para os dois pipes, nunca dois prazos em sequência.
    assert!(saida.duracao >= Duration::from_millis(400), "{:?}", saida.duracao);
    assert!(saida.duracao < Duration::from_millis(1200), "{:?}", saida.duracao);
}

#[tokio::test]
async fn carencia_zero_retorna_sem_esperar_escapado_e_sem_perder_saida_normal() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let mut e = sh("echo oi; echo erro >&2");
    e.limites.carencia = Duration::ZERO;
    let saida = sb.executar(&e).await.unwrap();
    assert_eq!((saida.stdout.as_slice(), saida.stderr.as_slice()), (&b"oi\n"[..], &b"erro\n"[..]));
    assert!(!saida.incompleta);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn carencia_zero_nao_espera_escapado() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let mut e = processo_escapado_com_pipes();
    e.limites.carencia = Duration::ZERO;
    let saida = sb.executar(&e).await.unwrap();
    let escapado: i32 = std::fs::read_to_string(lugar.copia.join("escapado")).unwrap().trim().parse().unwrap();
    // SAFETY: encerra o escapado deste teste.
    unsafe { libc::kill(escapado, libc::SIGKILL) };
    assert_eq!(saida.fim, Fim::Saiu(0));
    assert_eq!(saida.stdout, b"pronto\n");
    assert!(saida.stderr.is_empty(), "{:?}", String::from_utf8_lossy(&saida.stderr));
    assert!(saida.incompleta);
    assert!(saida.duracao < Duration::from_millis(800), "{:?}", saida.duracao);
}

#[tokio::test]
async fn pasta_que_e_link_para_fora_da_copia_e_recusada_e_pasta_real_funciona() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let fora = lugar.base.join("fora");
    std::fs::create_dir(&fora).unwrap();
    std::os::unix::fs::symlink(&fora, lugar.copia.join("atalho")).unwrap();
    std::fs::create_dir(lugar.copia.join("real")).unwrap();
    // Sem depender do umask (0002 deixaria a pasta gravável pelo grupo, recusada de propósito).
    std::fs::set_permissions(lugar.copia.join("real"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink("real", lugar.copia.join("relativo")).unwrap();
    for pasta in ["atalho", "relativo", "atalho/x", "ausente"] {
        let mut e = sh("echo escreveu > marca");
        e.pasta = pasta.into();
        let erro = sb.executar(&e).await.unwrap_err();
        assert!(erro.to_string().contains("pasta_fora_da_copia"), "{pasta}: {erro}");
        assert_eq!(sb.estado(), Estado::Preparado, "{pasta}: nada rodou");
    }
    assert!(!fora.join("marca").exists());
    let mut e = sh("echo dentro > marca");
    e.pasta = "real".into();
    assert_eq!(sb.executar(&e).await.unwrap().fim, Fim::Saiu(0));
    assert_eq!(std::fs::read_to_string(lugar.copia.join("real/marca")).unwrap(), "dentro\n");
}

#[tokio::test]
async fn copia_trocada_por_link_depois_de_preparar_nao_executa() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let alvo = lugar.base.join("home-do-hub");
    std::fs::create_dir(&alvo).unwrap();
    std::fs::set_permissions(&alvo, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::rename(&lugar.copia, lugar.base.join("copia-original")).unwrap();
    std::os::unix::fs::symlink(&alvo, &lugar.copia).unwrap();
    let erro = sb.executar(&sh("echo invadiu > marca")).await.unwrap_err();
    assert!(erro.to_string().contains("copia_trocada"), "{erro}");
    assert!(!alvo.join("marca").exists());
    assert_eq!(sb.estado(), Estado::Preparado);
    assert!(sb.copiar_para_dentro(&lugar.base.join("copia-original/x"), Path::new("y")).await.is_err());
}

// Pasta 000 só se abre por O_PATH, que é do Linux; em outros sistemas a limpeza recusa.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn encerrar_limpa_subpasta_sem_permissao_e_nao_segue_link() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let fora = lugar.base.join("fora");
    std::fs::create_dir(&fora).unwrap();
    std::fs::set_permissions(&fora, std::fs::Permissions::from_mode(0o500)).unwrap();
    let mut e = sh(r#"mkdir -p "$HOME/a/b" "$TMPDIR/t"; echo x > "$HOME/a/b/arq"; echo y > "$TMPDIR/t/arq"
        ln -s "$1" "$HOME/link"; chmod 000 "$HOME/a/b" "$HOME/a" "$TMPDIR/t""#);
    e.argv.extend(["sh".into(), fora.clone().into()]);
    assert_eq!(sb.executar(&e).await.unwrap().fim, Fim::Saiu(0));
    sb.encerrar().await.unwrap();
    assert_eq!(sb.estado(), Estado::Encerrado);
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
    // O alvo do link nunca recebeu chmod.
    assert_eq!(std::fs::metadata(&fora).unwrap().mode() & 0o777, 0o500);
    std::fs::set_permissions(&fora, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[tokio::test]
async fn raiz_sem_permissao_de_busca_nao_vira_limpeza_concluida() {
    if crate::fork::private_fs::uid() == 0 {
        return; // root ignora permissões; o Hub recusa rodar assim
    }
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    // Abre por leitura, mas consultar o filho dá EACCES: isso não pode virar "já apagado".
    std::fs::set_permissions(&lugar.raiz, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(sb.encerrar().await.is_err());
    assert_eq!(sb.estado(), Estado::Desconhecido);
    std::fs::set_permissions(&lugar.raiz, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 1, "nada some antes da limpeza real");
    sb.encerrar().await.unwrap();
    assert_eq!(sb.estado(), Estado::Encerrado);
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}

#[tokio::test]
async fn copias_recusam_pasta_intermediaria_gravavel_pelo_grupo() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let sub = lugar.copia.join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("x"), "dado").unwrap();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o770)).unwrap();
    let origem = lugar.base.join("entrada");
    std::fs::write(&origem, "novo").unwrap();
    let destino = lugar.base.join("exportado");
    assert!(sb.copiar_para_fora(Path::new("sub/x"), &destino).await.is_err());
    assert!(!destino.exists());
    assert!(sb.copiar_para_dentro(&origem, Path::new("sub/y")).await.is_err());
    assert!(!sub.join("y").exists());
    // Execução também não entra numa pasta gravável por terceiros.
    let mut e = exec(&["/bin/true"]);
    e.pasta = "sub".into();
    assert!(sb.executar(&e).await.is_err());
    // Com a pasta privada de novo, tudo volta a funcionar.
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut sb = lugar.sandbox().await;
    sb.copiar_para_fora(Path::new("sub/x"), &destino).await.unwrap();
    assert_eq!(std::fs::read_to_string(&destino).unwrap(), "dado");
}

#[tokio::test]
async fn limpeza_que_falha_nao_marca_encerrado_e_pode_repetir() {
    if crate::fork::private_fs::uid() == 0 {
        return; // root ignora permissões; o Hub recusa rodar assim
    }
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    // Raiz sem escrita: remover a pasta do sandbox falha.
    std::fs::set_permissions(&lugar.raiz, std::fs::Permissions::from_mode(0o500)).unwrap();
    assert!(sb.encerrar().await.is_err());
    assert_eq!(sb.estado(), Estado::Desconhecido);
    assert!(sb.executar(&exec(&["/bin/true"])).await.is_err());
    std::fs::set_permissions(&lugar.raiz, std::fs::Permissions::from_mode(0o700)).unwrap();
    sb.encerrar().await.unwrap();
    assert_eq!(sb.estado(), Estado::Encerrado);
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
    assert!(sb.encerrar().await.is_err());
    // O Drop também tenta de novo.
    let sb = lugar.sandbox().await;
    drop(sb);
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}

#[tokio::test]
async fn copiar_para_dentro_recusa_origem_e_destino_no_mesmo_arquivo() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    let resultado = lugar.copia.join("resultado");
    std::fs::write(&resultado, "conteudo").unwrap();
    let alias = lugar.base.join("alias");
    std::os::unix::fs::symlink(&resultado, &alias).unwrap();
    for origem in [&resultado, &alias] {
        let erro = sb.copiar_para_dentro(origem, Path::new("resultado")).await.unwrap_err();
        assert!(erro.to_string().contains("copia_mesmo_arquivo"), "{origem:?}: {erro}");
        assert_eq!(std::fs::read_to_string(&resultado).unwrap(), "conteudo", "{origem:?} apagou a origem");
    }
    // Arquivo diferente continua copiando por cima.
    let outro = lugar.base.join("outro");
    std::fs::write(&outro, "novo").unwrap();
    sb.copiar_para_dentro(&outro, Path::new("resultado")).await.unwrap();
    assert_eq!(std::fs::read_to_string(&resultado).unwrap(), "novo");
}

#[tokio::test]
async fn copias_recusam_raiz_trocada_por_outra_pasta_real_depois_de_preparar() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    std::fs::write(lugar.copia.join("resultado"), "original").unwrap();
    let entrada = lugar.base.join("entrada");
    std::fs::write(&entrada, "dado").unwrap();
    // Concorrente com o mesmo UID troca a cópia por outro diretório real, com dono e modo válidos.
    std::fs::rename(&lugar.copia, lugar.base.join("copia-original")).unwrap();
    std::fs::create_dir(&lugar.copia).unwrap();
    std::fs::set_permissions(&lugar.copia, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(lugar.copia.join("resultado"), "substituta").unwrap();
    let destino = lugar.base.join("exportado");
    let erro = sb.copiar_para_fora(Path::new("resultado"), &destino).await.unwrap_err();
    assert!(erro.to_string().contains("copia_trocada"), "{erro}");
    assert!(!destino.exists(), "exportou da árvore substituta");
    let erro = sb.copiar_para_dentro(&entrada, Path::new("resultado")).await.unwrap_err();
    assert!(erro.to_string().contains("copia_trocada"), "{erro}");
    assert_eq!(std::fs::read_to_string(lugar.copia.join("resultado")).unwrap(), "substituta");
    // Devolvida a original, as cópias voltam a funcionar.
    std::fs::remove_dir_all(&lugar.copia).unwrap();
    std::fs::rename(lugar.base.join("copia-original"), &lugar.copia).unwrap();
    sb.copiar_para_fora(Path::new("resultado"), &destino).await.unwrap();
    assert_eq!(std::fs::read_to_string(&destino).unwrap(), "original");
}

#[tokio::test]
async fn limpeza_recusa_pasta_privada_substituida_e_nao_apaga_nada() {
    let lugar = Lugar::novo();
    let mut sb = lugar.sandbox().await;
    sb.executar(&sh("echo x > \"$HOME/h\"")).await.unwrap();
    let privada = std::fs::read_dir(&lugar.raiz).unwrap().next().unwrap().unwrap().path();
    let original = lugar.base.join("privada-original");
    std::fs::rename(&privada, &original).unwrap();
    // Outro diretório real, com arquivo e subpasta sem permissão, no lugar.
    std::fs::create_dir_all(privada.join("sub")).unwrap();
    std::fs::write(privada.join("sub/precioso"), "nao apagar").unwrap();
    std::fs::set_permissions(privada.join("sub"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let erro = sb.encerrar().await.unwrap_err();
    assert!(erro.to_string().contains("privada_trocada"), "{erro}");
    assert_eq!(sb.estado(), Estado::Desconhecido);
    assert_eq!(std::fs::read_to_string(privada.join("sub/precioso")).unwrap(), "nao apagar");
    assert_eq!(std::fs::metadata(privada.join("sub")).unwrap().mode() & 0o777, 0o500, "chmod na árvore substituta");
    assert!(original.join("home/h").exists(), "apagou a original");
    // Corrigida a troca, nova tentativa limpa.
    std::fs::set_permissions(privada.join("sub"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_dir_all(&privada).unwrap();
    std::fs::rename(&original, &privada).unwrap();
    sb.encerrar().await.unwrap();
    assert_eq!(sb.estado(), Estado::Encerrado);
    assert_eq!(std::fs::read_dir(&lugar.raiz).unwrap().count(), 0);
}
