#![allow(clippy::unwrap_used)]
//! D70, sem rede: servidor HTTP local servindo pacotes de mentira.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Instala um pacote cujo hash confere (o pacote em si é "oficial") e devolve o erro;
/// confere que nada ficou instalado nem sobrou na raiz.
async fn refused(file: &str, archive: Vec<u8>, exe: &'static str, limit: u64) -> String {
    let (base, _) = serve(archive.clone(), false).await;
    let root = root();
    let error = install_with(&root, &base, target(file, &archive, exe), limit).await.unwrap_err();
    assert!(!installed_path(&root).exists(), "{file}: instalou");
    assert_eq!(leftovers(&root), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(root);
    error.to_string()
}
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const EXE: &[u8] = b"wacli de mentira";

fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn leak(text: String) -> &'static str {
    text.leak()
}

fn tar_gz() -> Vec<u8> {
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    for (path, data) in [("./LICENSE", &b"MIT"[..]), ("./wacli", EXE)] {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, path, data).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

/// Tar com nomes crus (sem a validação do `tar::Builder`), para montar pacotes maliciosos.
/// `link` = entrada é link simbólico para `/tmp/fora`.
fn raw_tar(entries: &[(&str, bool)]) -> Vec<u8> {
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    for &(name, link) in entries {
        let mut header = tar::Header::new_gnu();
        let old = header.as_old_mut();
        old.name[..name.len()].copy_from_slice(name.as_bytes());
        if link {
            old.linkname[..9].copy_from_slice(b"/tmp/fora");
        }
        header.set_entry_type(if link { tar::EntryType::Symlink } else { tar::EntryType::Regular });
        header.set_size(if link { 0 } else { EXE.len() as u64 });
        header.set_mode(0o755);
        header.set_cksum();
        tar.append(&header, if link { &b""[..] } else { EXE }).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

/// Zip mínimo: README guardado e o executável comprimido (deflate), como o oficial.
fn zip() -> Vec<u8> {
    zip_with(&[("README.md", false), ("wacli.exe", true)])
}

/// Cada entrada leva `EXE` (o README leva também; só o nome importa), `true` = deflate.
fn zip_with(entries: &[(&str, bool)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut deflate = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    deflate.write_all(EXE).unwrap();
    let packed = deflate.finish().unwrap();
    for &(name, compressed) in entries {
        let (method, data) = if compressed { (8_u16, &packed[..]) } else { (0, EXE) };
        out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0]);
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
    out
}

fn target(file: &str, archive: &[u8], exe: &'static str) -> Target {
    Target { file: leak(file.into()), sha256: leak(sha(archive)), exe, exe_sha256: leak(sha(EXE)) }
}

/// `cut` = anuncia o dobro do tamanho e fecha no meio (download interrompido).
async fn serve(body: Vec<u8>, cut: bool) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v0.25.1", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            let body = body.clone();
            tokio::spawn(async move {
                let mut request = [0_u8; 4096];
                let _ = socket.read(&mut request).await;
                let declared = if cut { body.len() * 2 } else { body.len() };
                let head = format!("HTTP/1.1 200 OK\r\ncontent-length: {declared}\r\nconnection: close\r\n\r\n");
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (base, hits)
}

fn root() -> PathBuf {
    std::env::temp_dir().join(format!("donna-wacli-{}", uuid::Uuid::new_v4()))
}

/// Nada além de `bin/` (e o que estiver nela) sobra na raiz.
fn leftovers(root: &Path) -> Vec<String> {
    std::fs::read_dir(root)
        .map(|d| d.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| n != "bin").collect())
        .unwrap_or_default()
}

async fn settled(this: &Arc<Downloader>) -> Value {
    for _ in 0..200 {
        let view = Downloader::follow(this, true, false, || {});
        if view["state"] != "downloading" {
            return view;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("download não terminou");
}

#[test]
fn tabela_oficial_fixada() {
    if let Some(t) = official() {
        assert!(t.file.starts_with("wacli_0.25.1_"));
        assert_eq!((t.sha256.len(), t.exe_sha256.len()), (64, 64));
    }
    assert!(BASE.starts_with("https://github.com/openclaw/wacli/releases/download/v"));
    assert!(BASE.ends_with(VERSION));
}

#[tokio::test]
async fn hash_certo_instala_no_caminho_padrao() {
    for (file, archive, exe) in
        [("wacli_0.25.1_linux_amd64.tar.gz", tar_gz(), "wacli"), ("wacli_0.25.1_windows_amd64.zip", zip(), "wacli.exe")]
    {
        let (base, _) = serve(archive.clone(), false).await;
        let root = root();
        install(&root, &base, target(file, &archive, exe)).await.unwrap();
        let path = installed_path(&root);
        assert_eq!(path, root.join("bin").join(if cfg!(windows) { "wacli.exe" } else { "wacli" }));
        assert_eq!(std::fs::read(&path).unwrap(), EXE);
        assert_eq!(leftovers(&root), Vec::<String>::new());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);
            assert_eq!(std::fs::metadata(root.join("bin")).unwrap().permissions().mode() & 0o777, 0o700);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn hash_errado_ou_download_interrompido_nao_deixam_nada() {
    let archive = tar_gz();
    let mut wrong = target("w.tar.gz", &archive, "wacli");
    wrong.sha256 = leak(sha(b"outro"));
    let mut wrong_exe = target("w.tar.gz", &archive, "wacli");
    wrong_exe.exe_sha256 = leak(sha(b"outro"));
    for (cut, t) in [(false, wrong), (false, wrong_exe), (true, target("w.tar.gz", &archive, "wacli"))] {
        let (base, _) = serve(archive.clone(), cut).await;
        let root = root();
        let error = install(&root, &base, t).await.unwrap_err();
        if !cut {
            assert!(error.to_string().starts_with("wacli_integrity"), "{error:#}");
        }
        assert!(!installed_path(&root).exists());
        assert_eq!(leftovers(&root), Vec::<String>::new());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn desligado_nao_baixa_e_sem_alvo_nao_tenta() {
    let archive = tar_gz();
    let (base, hits) = serve(archive.clone(), false).await;
    let root = root();
    let on = Arc::new(Downloader::new(root.clone(), base.clone(), Some(target("w.tar.gz", &archive, "wacli"))));
    assert_eq!(Downloader::follow(&on, false, true, || {})["state"], "off");
    let none = Arc::new(Downloader::new(root.clone(), base, None));
    assert_eq!(Downloader::follow(&none, true, true, || {})["state"], "unsupported");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(!root.exists());
}

#[tokio::test]
async fn estados_na_leitura_e_tentar_de_novo() {
    let archive = tar_gz();
    let (base, hits) = serve(archive.clone(), false).await;
    let root = root();
    let good = target("w.tar.gz", &archive, "wacli");
    let this = Arc::new(Downloader::new(root.clone(), base.clone(), Some(good)));
    assert_eq!(Downloader::follow(&this, true, false, || {})["state"], "downloading");
    assert_eq!(settled(&this).await, json!({"state": "ready", "version": VERSION}));
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    // Apagado com o plugin ligado: baixa de novo.
    std::fs::remove_file(installed_path(&root)).unwrap();
    assert_eq!(settled(&this).await["state"], "ready");
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    // Já certo num processo novo: confere o hash e não baixa. Trocado: volta o oficial.
    let fresh = Arc::new(Downloader::new(root.clone(), base.clone(), Some(good)));
    assert_eq!(settled(&fresh).await["state"], "ready");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    std::fs::write(installed_path(&root), b"adulterado").unwrap();
    let fresh = Arc::new(Downloader::new(root.clone(), base.clone(), Some(good)));
    assert_eq!(settled(&fresh).await["state"], "ready");
    assert_eq!(std::fs::read(installed_path(&root)).unwrap(), EXE);
    assert_eq!(hits.load(Ordering::SeqCst), 3);

    // Falha fica na tela e só tenta de novo quando o dono pede.
    let mut bad = good;
    bad.sha256 = leak(sha(b"outro"));
    let other = root.join("outro");
    let failing = Arc::new(Downloader::new(other.clone(), base, Some(bad)));
    let failed = settled(&failing).await;
    assert_eq!(failed["state"], "failed");
    assert!(failed["reason"].as_str().unwrap().starts_with("wacli_integrity"));
    assert_eq!(Downloader::follow(&failing, true, false, || {})["state"], "failed");
    assert_eq!(hits.load(Ordering::SeqCst), 4);
    assert_eq!(Downloader::follow(&failing, true, true, || {})["state"], "downloading");
    assert_eq!(settled(&failing).await["state"], "failed");
    assert_eq!(hits.load(Ordering::SeqCst), 5);
    assert!(!installed_path(&other).exists());
    std::fs::remove_dir_all(root).unwrap();
}

const MISSING: &str = "wacli_integrity: o pacote não tem o executável.";
const TOO_BIG: &str = "wacli_integrity: executável grande demais.";

/// (c) Só o arquivo comum `wacli` na raiz do tar: `..`, absoluto, subpasta e link são ignorados.
#[tokio::test]
async fn tar_recusa_fora_da_raiz_subpasta_e_link() {
    for (name, link) in [("../wacli", false), ("/wacli", false), ("bin/wacli", false), ("wacli", true)] {
        let archive = raw_tar(&[("./LICENSE", false), (name, link)]);
        assert_eq!(refused("w.tar.gz", archive, "wacli", EXE_LIMIT).await, MISSING, "{name} link={link}");
    }
}

/// (c) No zip, só o nome exato `wacli.exe`.
#[tokio::test]
async fn zip_recusa_fora_da_raiz_e_subpasta() {
    for name in ["../wacli.exe", "/wacli.exe", "C:\\wacli.exe", "pasta/wacli.exe"] {
        let archive = zip_with(&[("README.md", false), (name, true)]);
        assert_eq!(refused("w.zip", archive, "wacli.exe", EXE_LIMIT).await, MISSING, "{name}");
    }
}

/// (d) Acima do limite descompactado: recusa, sem cortar. No limite exato, instala.
#[tokio::test]
async fn executavel_acima_do_limite_recusa() {
    let small = EXE.len() as u64 - 1;
    assert_eq!(refused("w.tar.gz", tar_gz(), "wacli", small).await, TOO_BIG);
    assert_eq!(refused("w.zip", zip_with(&[("wacli.exe", false)]), "wacli.exe", small).await, TOO_BIG);
    assert_eq!(refused("w.zip", zip(), "wacli.exe", small).await, TOO_BIG);
    let archive = zip();
    let (base, _) = serve(archive.clone(), false).await;
    let root = root();
    install_with(&root, &base, target("w.zip", &archive, "wacli.exe"), EXE.len() as u64).await.unwrap();
    assert_eq!(std::fs::read(installed_path(&root)).unwrap(), EXE);
    std::fs::remove_dir_all(root).unwrap();
}
