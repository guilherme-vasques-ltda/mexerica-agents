use super::*;

#[test]
fn hash_incorreto_nao_publica_nem_substitui_instalacao() {
    let raiz = tempfile::tempdir().unwrap();
    let pasta = raiz.path().join("agente");
    fs::create_dir(&pasta).unwrap();
    let destino = pasta.join("1.0.0");
    assert!(publicar(&destino, "programa", b"conteudo", &"0".repeat(64), Path::new("programa")).is_err());
    assert_eq!(fs::read_dir(&pasta).unwrap().count(), 0);
    fs::create_dir(&destino).unwrap();
    fs::write(destino.join("programa"), b"anterior").unwrap();
    assert!(publicar(&destino, "programa", b"conteudo", &"0".repeat(64), Path::new("programa")).is_err());
    assert_eq!(fs::read(destino.join("programa")).unwrap(), b"anterior");
}

#[test]
fn publica_tar_gzip_com_hash_valido_e_executavel_regular() {
    let codificador = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(codificador);
    let mut entrada = tar::Header::new_gnu();
    entrada.set_size(8);
    entrada.set_mode(0o644);
    tar.append_data(&mut entrada, "bin/programa", &b"conteudo"[..]).unwrap();
    let bytes = tar.into_inner().unwrap().finish().unwrap();
    let raiz = tempfile::tempdir().unwrap();
    let destino = raiz.path().join("1.0.0");
    let cmd = Path::new("bin/programa");
    let hash = super::super::sha256_hex(&bytes).to_ascii_uppercase();
    publicar(&destino, "pacote.tar.gz", &bytes, &hash, cmd).unwrap();
    assert!(cache_verificado(&destino, &hash, cmd).unwrap());
    assert_eq!(fs::read(destino.join(cmd)).unwrap(), b"conteudo");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(fs::metadata(destino.join(cmd)).unwrap().permissions().mode() & 0o111, 0);
    }
}

#[test]
fn publica_zip_com_executavel_e_arquivos_auxiliares() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opcoes = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("bin/programa", opcoes).unwrap();
    zip.write_all(b"conteudo").unwrap();
    zip.start_file("dados/leia-me.txt", opcoes).unwrap();
    zip.write_all(b"leia-me").unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let raiz = tempfile::tempdir().unwrap();
    let destino = raiz.path().join("1.0.0");
    let cmd = Path::new("bin/programa");
    let hash = super::super::sha256_hex(&bytes);
    publicar(&destino, "pacote.zip", &bytes, &hash, cmd).unwrap();
    assert!(cache_verificado(&destino, &hash, cmd).unwrap());
    assert_eq!(fs::read(destino.join("dados/leia-me.txt")).unwrap(), b"leia-me");
}

#[test]
fn recusa_caminhos_de_arquivo_fora_do_destino() {
    for nome in ["../arquivo", "dir/../arquivo", "/arquivo", "C:/arquivo", r"dir\..\arquivo"] {
        assert!(caminho_relativo(Path::new(nome)).is_err(), "{nome}");
    }
    assert!(caminho_relativo(Path::new("./bin/programa")).is_ok());
}

#[test]
fn recusa_link_no_tar_sem_publicar() {
    let mut tar = tar::Builder::new(Vec::new());
    let mut entrada = tar::Header::new_gnu();
    entrada.set_entry_type(tar::EntryType::Symlink);
    entrada.set_size(0);
    entrada.set_mode(0o777);
    tar.append_link(&mut entrada, "bin/programa", "programa-real").unwrap();
    let bytes = tar.into_inner().unwrap();
    let raiz = tempfile::tempdir().unwrap();
    let destino = raiz.path().join("1.0.0");
    let resultado =
        publicar(&destino, "pacote.tar", &bytes, &super::super::sha256_hex(&bytes), Path::new("bin/programa"));
    assert!(resultado.unwrap_err().to_string().contains("link"));
    assert!(!destino.exists());
}

#[cfg(unix)]
#[test]
fn recusa_executavel_em_cache_que_virou_link_ou_diretorio() {
    let raiz = tempfile::tempdir().unwrap();
    let hash = "a".repeat(64);
    fs::write(raiz.path().join(".installed"), &hash).unwrap();
    fs::write(raiz.path().join("programa-real"), b"conteudo").unwrap();
    std::os::unix::fs::symlink("programa-real", raiz.path().join("link")).unwrap();
    fs::create_dir(raiz.path().join("pasta")).unwrap();
    for cmd in ["link", "pasta"] {
        assert!(cache_verificado(raiz.path(), &hash, Path::new(cmd)).is_err());
    }
}
