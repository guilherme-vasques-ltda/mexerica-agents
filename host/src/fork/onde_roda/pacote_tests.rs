use super::vinculo_git::{PacoteConferidoGit, exigir_vinculo_git};
use super::*;
use std::fmt::Write as _;

fn pacote_somente_manifesto(arquivos: &[(&str, &[u8])]) -> Pacote {
    let mut p = Pacote {
        manifesto: Manifesto {
            version: 1,
            repo: "repo-opaco".into(),
            commit: "a".repeat(40),
            tree: "b".repeat(40),
            arquivos: Vec::new(),
            digest_total: [0; 32],
        },
        blobs: Vec::new(),
    };
    for (path, bytes) in arquivos {
        adicionar(&mut p, path, bytes.to_vec());
    }
    selar_somente_manifesto(&mut p);
    p
}

fn adicionar(p: &mut Pacote, path: &str, bytes: Vec<u8>) {
    p.manifesto.arquivos.push(Entrada {
        path: path.into(),
        mode: 0o100_644,
        digest: Sha256::digest(&bytes).into(),
        size: bytes.len() as u64,
    });
    p.blobs.push(Blob { path: path.into(), bytes });
}

fn selar_somente_manifesto(p: &mut Pacote) {
    p.manifesto.digest_total = digest_manifesto(&p.manifesto).unwrap();
}

fn erro(p: &Pacote, codigo: &str) {
    let falha = conferir_manifesto(p).err().expect("pacote deveria ser recusado");
    assert_eq!(falha.to_string(), codigo);
}

#[test]
fn somente_manifesto_bytes_reais_retorno_imutavel_e_vetor_canonico() {
    let p = pacote_somente_manifesto(&[("a.txt", b"oi\n"), ("src/main.rs", b"")]);
    let somente_manifesto = conferir_manifesto(&p).unwrap();
    assert_eq!(somente_manifesto.manifesto().repo, "repo-opaco");
    assert_eq!(
        somente_manifesto.arquivos().map(|(_, b)| b).collect::<Vec<_>>(),
        vec![b"oi\n".as_slice(), b"".as_slice()]
    );
    // Vetor calculado independentemente com hashlib/struct, não pelo codec Rust.
    let mut hex = String::with_capacity(64);
    for byte in p.manifesto.digest_total {
        write!(&mut hex, "{byte:02x}").unwrap();
    }
    assert_eq!(hex, "c8f3de1ccf4985fa6186b8966246a517fa1e7769483b1be2e73566d483d41453");
    assert!(conferir_manifesto(&pacote_somente_manifesto(&[])).is_ok());
    let mut modo_executavel_somente_manifesto = pacote_somente_manifesto(&[("script", b"conteudo")]);
    modo_executavel_somente_manifesto.manifesto.arquivos[0].mode = 0o100_755;
    modo_executavel_somente_manifesto.manifesto.commit = "c".repeat(64);
    modo_executavel_somente_manifesto.manifesto.tree = "d".repeat(64);
    selar_somente_manifesto(&mut modo_executavel_somente_manifesto);
    assert!(conferir_manifesto(&modo_executavel_somente_manifesto).is_ok());
}

#[test]
fn somente_manifesto_recusa_paths_perigosos_e_segredos_conhecidos() {
    for path in [
        "",
        "/a",
        "../a",
        "a/../b",
        "./a",
        "a//b",
        "a/",
        "a\\b",
        "C:/a",
        "a\0b",
        "a\nb",
        "a\u{7f}b",
        "caf\u{e9}",
        ".git/config",
        "a/.GiT/config",
        "a.",
        "a/.env",
        "AUTH.json",
        "con.txt",
        "LPT1",
        "a/aux",
    ] {
        assert!(conferir_manifesto(&pacote_somente_manifesto(&[(path, b"x")])).is_err(), "{path:?}");
    }
    assert!(conferir_manifesto(&pacote_somente_manifesto(&[(&"a".repeat(MAX_PATH_BYTES), b"x")])).is_ok());
    assert!(conferir_manifesto(&pacote_somente_manifesto(&[(&"a".repeat(MAX_PATH_BYTES + 1), b"x")])).is_err());
}

#[test]
fn somente_manifesto_recusa_colisoes_ordem_e_arquivo_pai() {
    for paths in [["a", "a"], ["A", "a"], ["a", "a/b"], ["A", "a/b"], ["Dir/a", "dir/b"], ["z", "a"], ["a/b", "a/b/c"]]
    {
        assert!(
            conferir_manifesto(&pacote_somente_manifesto(&[(paths[0], b""), (paths[1], b"")])).is_err(),
            "{paths:?}"
        );
    }
    assert!(conferir_manifesto(&pacote_somente_manifesto(&[("dir/a", b""), ("dir/b", b"")])).is_ok());
}

#[test]
fn somente_manifesto_recusa_tipos_nao_regulares_sem_omitir() {
    // Symlink, gitlink/submódulo, diretório, dispositivos, FIFO e socket.
    for mode in [0o120_000, 0o160_000, 0o040_000, 0o020_000, 0o060_000, 0o010_000, 0o140_000, 0, 0o100_600] {
        let mut p = pacote_somente_manifesto(&[("arquivo", b"destino")]);
        p.manifesto.arquivos[0].mode = mode;
        selar_somente_manifesto(&mut p);
        erro(&p, "snapshot_mode");
    }
    // Não existe variante de hardlink: bytes por caminho são obrigatórios.
    let mut p = pacote_somente_manifesto(&[("a", b"x"), ("b", b"x")]);
    p.blobs.pop();
    erro(&p, "snapshot_blob_count");
}

#[test]
fn somente_manifesto_recusa_bytes_digests_tamanhos_e_blobs_inconsistentes() {
    let mut p = pacote_somente_manifesto(&[("a", b"abc")]);
    p.blobs[0].bytes[0] ^= 1;
    erro(&p, "snapshot_blob_digest");
    let mut p = pacote_somente_manifesto(&[("a", b"abc")]);
    p.manifesto.arquivos[0].size += 1;
    erro(&p, "snapshot_size");
    p.manifesto.arquivos[0].size = u64::MAX;
    erro(&p, "snapshot_file_bytes");
    let mut p = pacote_somente_manifesto(&[("a", b"abc")]);
    p.manifesto.arquivos[0].digest[0] ^= 1;
    selar_somente_manifesto(&mut p);
    erro(&p, "snapshot_blob_digest");
    let mut p = pacote_somente_manifesto(&[("a", b"abc")]);
    p.manifesto.repo = "outro-repo".into();
    erro(&p, "snapshot_total_digest");
    let mut p = pacote_somente_manifesto(&[("a", b"abc")]);
    p.manifesto.digest_total[0] ^= 1;
    erro(&p, "snapshot_total_digest");
    let mut p = pacote_somente_manifesto(&[("a", b"abc")]);
    p.blobs[0].path = "b".into();
    erro(&p, "snapshot_blob_path");
    p.blobs.clear();
    erro(&p, "snapshot_blob_count");
    let mut p = pacote_somente_manifesto(&[]);
    p.blobs.push(Blob { path: "extra".into(), bytes: vec![] });
    erro(&p, "snapshot_blob_count");
}

#[test]
fn somente_manifesto_recusa_lfs_pendente_inclusive_ponteiro_invalido() {
    for bytes in [
        b"version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 9\n".as_slice(),
        b"version https://git-lfs.github.com/spec/v1\nINVALIDO".as_slice(),
        b"version https://git-lfs.github.com/spec/v999".as_slice(),
    ] {
        erro(&pacote_somente_manifesto(&[("asset", bytes)]), "snapshot_lfs_pending");
    }
}

#[test]
fn somente_manifesto_limites_de_arquivos_inclusivos() {
    let mut p = pacote_somente_manifesto(&[]);
    for i in 0..MAX_ARQUIVOS {
        adicionar(&mut p, &format!("f{i:04}"), vec![]);
    }
    selar_somente_manifesto(&mut p);
    assert!(conferir_manifesto(&p).is_ok());
    adicionar(&mut p, "ultimo", vec![]);
    erro(&p, "snapshot_file_limit");
}

#[test]
fn somente_manifesto_limites_de_bytes_e_expansao_um_para_um() {
    let mut p = pacote_somente_manifesto(&[]);
    for i in 0..4 {
        adicionar(&mut p, &format!("f{i}"), vec![b'x'; usize::try_from(MAX_ARQUIVO_BYTES).unwrap()]);
    }
    selar_somente_manifesto(&mut p);
    assert_eq!(p.manifesto.arquivos.iter().map(|a| a.size).sum::<u64>(), MAX_TOTAL_BYTES);
    assert!(conferir_manifesto(&p).is_ok());
    adicionar(&mut p, "f4", vec![0]);
    erro(&p, "snapshot_total_bytes");
    p.blobs[0].bytes.push(0);
    p.manifesto.arquivos[0].size += 1;
    erro(&p, "snapshot_file_bytes");
}

#[test]
fn somente_manifesto_referencia_e_versao_nao_sao_prova_git() {
    let mut p = pacote_somente_manifesto(&[]);
    p.manifesto.version = 2;
    erro(&p, "snapshot_version");
    p.manifesto.version = 1;
    for repo in ["", "com espaco", "x\n", &"x".repeat(129)] {
        p.manifesto.repo = repo.into();
        erro(&p, "snapshot_repo");
    }
    p.manifesto.repo = "x".repeat(128);
    selar_somente_manifesto(&mut p);
    assert!(conferir_manifesto(&p).is_ok());
    for commit in ["a".repeat(39), "A".repeat(40), "g".repeat(40), "c".repeat(64)] {
        p.manifesto.commit = commit;
        erro(&p, "snapshot_oid");
    }
}

#[test]
fn somente_manifesto_oid_ficticio_nao_atende_limite_git() {
    let p = pacote_somente_manifesto(&[("arquivo", b"conteudo arbitrario")]);
    let somente_manifesto = conferir_manifesto(&p).unwrap();
    assert_eq!(somente_manifesto.manifesto().commit, "a".repeat(40));
    assert_eq!(somente_manifesto.manifesto().tree, "b".repeat(40));
    // Não existe conversão deste resultado para PacoteConferidoGit.
    let sem_prova_git: Option<PacoteConferidoGit<'_>> = None;
    let erro = exigir_vinculo_git(sem_prova_git).err().expect("ausência de prova Git deve falhar");
    assert_eq!(erro.to_string(), "snapshot_git_nao_conferido");
}
