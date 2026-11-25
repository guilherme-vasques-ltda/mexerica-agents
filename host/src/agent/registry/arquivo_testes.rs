use super::*;

fn publicar(destino: &Path, nome: &str, bytes: &[u8], hash: &str, cmd: &Path) -> Result<()> {
    super::publicar(destino, nome, bytes, hash, cmd, LIMITE_EXTRAIDO)
}

fn pasta_temporaria() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}

fn tar_com_programa(prefixo: impl FnOnce(&mut tar::Builder<Vec<u8>>)) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    prefixo(&mut tar);
    let mut cabecalho = tar::Header::new_gnu();
    cabecalho.set_size(8);
    cabecalho.set_mode(0o644);
    tar.append_data(&mut cabecalho, "bin/programa", &b"conteudo"[..]).unwrap();
    tar.into_inner().unwrap()
}

#[test]
fn ignora_cabecalho_pax_global_sem_criar_arquivo() {
    let dados = b"15 comment=abc\n";
    let bytes = tar_com_programa(|tar| {
        let mut cabecalho = tar::Header::new_ustar();
        cabecalho.set_entry_type(tar::EntryType::XGlobalHeader);
        cabecalho.set_size(dados.len() as u64);
        cabecalho.set_mode(0o644);
        tar.append_data(&mut cabecalho, "pax_global_header", &dados[..]).unwrap();
    });
    let raiz = pasta_temporaria();
    let destino = raiz.path().join("agente");
    publicar(&destino, "pacote.tar", &bytes, &super::super::sha256_hex(&bytes), Path::new("bin/programa")).unwrap();
    assert_eq!(fs::read(destino.join("bin/programa")).unwrap(), b"conteudo");
    assert!(!destino.join("pax_global_header").exists());
}

#[cfg(unix)]
#[test]
fn cache_recusa_programa_adulterado_mesmo_com_marcador_e_permissao_originais() {
    use std::os::unix::fs::PermissionsExt as _;
    let raiz = pasta_temporaria();
    let destino = pasta_instalacao(raiz.path(), "agente", "1.0.0").unwrap();
    let cmd = Path::new("bin/programa");
    let bytes = b"programa verificado";
    let hash = super::super::sha256_hex(bytes);
    publicar(&destino, "programa", bytes, &hash, cmd).unwrap();
    assert!(cache_verificado(&destino, &hash, cmd).unwrap());
    let exe = destino.join(cmd);
    for dir in [raiz.path().join("agents"), raiz.path().join("agents/agente"), destino.clone()] {
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700, "{dir:?}");
    }
    assert_eq!(fs::metadata(destino.join("bin")).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o700);
    fs::write(&exe, b"programa adulterado").unwrap();
    assert_ne!(fs::metadata(exe).unwrap().permissions().mode() & 0o100, 0);
    assert!(!cache_verificado(&destino, &hash, cmd).unwrap());
    publicar(&destino, "programa", bytes, &hash, cmd).unwrap();
    assert!(cache_verificado(&destino, &hash, cmd).unwrap());
    assert_eq!(fs::read(destino.join(cmd)).unwrap(), bytes);
}

#[test]
fn falha_ao_publicar_a_nova_arvore_restaura_a_anterior() {
    let raiz = pasta_temporaria();
    let destino = raiz.path().join("agente");
    fs::create_dir(&destino).unwrap();
    fs::write(destino.join("programa"), b"anterior").unwrap();
    let erro = substituir(&raiz.path().join("pasta-ausente"), &destino, raiz.path()).unwrap_err();
    assert!(erro.to_string().contains("anterior foi restaurada"));
    assert_eq!(fs::read(destino.join("programa")).unwrap(), b"anterior");
    assert_eq!(fs::read_dir(raiz.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn publicacao_revalida_pai_e_destino_contra_links() {
    use std::os::unix::fs::symlink;
    let raiz = pasta_temporaria();
    let nova = raiz.path().join("nova");
    fs::create_dir(&nova).unwrap();
    let fora = raiz.path().join("fora");
    fs::create_dir(&fora).unwrap();
    let pai = raiz.path().join("pai");
    symlink(&fora, &pai).unwrap();
    assert!(substituir(&nova, &pai.join("agente"), &pai).is_err());
    assert_eq!(fs::read_dir(fora).unwrap().count(), 0);
    let destino = raiz.path().join("agente");
    symlink(&nova, &destino).unwrap();
    assert!(substituir(&nova, &destino, raiz.path()).is_err());
    assert!(fs::symlink_metadata(destino).unwrap().is_symlink());
}

#[test]
fn pacote_com_recibo_legado_nao_planta_a_prova_nova() {
    let bytes = tar_com_programa(|tar| {
        let mut cabecalho = tar::Header::new_gnu();
        cabecalho.set_size(5);
        cabecalho.set_mode(0o644);
        tar.append_data(&mut cabecalho, ".publicada", &b"velho"[..]).unwrap();
    });
    let raiz = pasta_temporaria();
    let destino = raiz.path().join("agente");
    let cmd = Path::new("bin/programa");
    let hash = super::super::sha256_hex(&bytes);
    publicar(&destino, "pacote.tar", &bytes, &hash, cmd).unwrap();
    assert!(publicada_por_este_protocolo(&destino), "a prova é a do protocolo, não a do pacote");
    assert_ne!(fs::read(destino.join(RECIBO)).unwrap(), b"velho");
    // Recibo legado solto, sem o vínculo com o marcador, não vira prova.
    fs::write(destino.join(RECIBO), b"velho").unwrap();
    assert!(!publicada_por_este_protocolo(&destino));
    fs::remove_file(destino.join(RECIBO)).unwrap();
    assert!(!publicada_por_este_protocolo(&destino));
}

#[test]
fn troca_interrompida_entre_renames_recupera_a_anterior() {
    let raiz = pasta_temporaria();
    let destino = raiz.path().join("1.0.0");
    // Interrupção depois do primeiro rename: destino ausente, anterior esperando.
    let copia = raiz.path().join(".agente-anterior:1.0.0");
    let anterior = copia.join("instalacao");
    fs::create_dir_all(&anterior).unwrap();
    fs::write(anterior.join("programa"), b"anterior").unwrap();
    recuperar_anterior(raiz.path(), &destino).unwrap();
    assert_eq!(fs::read(destino.join("programa")).unwrap(), b"anterior");
    assert!(!copia.exists());
    // Destino presente: o resto é cópia velha e sai sem tocar no destino.
    fs::create_dir_all(&anterior).unwrap();
    fs::write(anterior.join("programa"), b"velha").unwrap();
    recuperar_anterior(raiz.path(), &destino).unwrap();
    assert_eq!(fs::read(destino.join("programa")).unwrap(), b"anterior");
    assert!(!copia.exists());
}

#[test]
fn erro_ao_consultar_o_destino_preserva_a_copia_anterior() {
    let raiz = pasta_temporaria();
    // Destino sob um arquivo comum: consultar dá erro que não é ausência.
    let bloqueio = raiz.path().join("bloqueio");
    fs::write(&bloqueio, b"arquivo").unwrap();
    let destino = bloqueio.join("1.0.0");
    let copia = raiz.path().join(".agente-anterior:1.0.0");
    let anterior = copia.join("instalacao");
    fs::create_dir_all(&anterior).unwrap();
    fs::write(anterior.join("programa"), b"anterior").unwrap();
    assert!(recuperar_anterior(raiz.path(), &destino).is_err());
    assert_eq!(fs::read(anterior.join("programa")).unwrap(), b"anterior", "a única cópia fica intacta");
}

#[test]
fn recusa_expansao_acima_do_teto_no_zip_e_no_tar() {
    let raiz = pasta_temporaria();
    let dados = vec![0_u8; 8192];
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opcoes = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for nome in ["programa", "dados"] {
        zip.start_file(nome, opcoes).unwrap();
        zip.write_all(&dados[..2048]).unwrap();
    }
    let zip = zip.finish().unwrap().into_inner();
    let tar = tar_com_programa(|tar| {
        let mut cabecalho = tar::Header::new_ustar();
        cabecalho.set_size(dados.len() as u64);
        cabecalho.set_mode(0o644);
        tar.append_data(&mut cabecalho, "dados", &dados[..]).unwrap();
    });
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar).unwrap();
    for (nome, bytes) in [("pacote.zip", zip), ("pacote.tar.gz", gz.finish().unwrap())] {
        assert!(bytes.len() < 3072);
        let pasta = pasta_temporaria();
        let erro = extrair(nome, &bytes, pasta.path(), 3072, 10, Path::new("programa")).unwrap_err();
        assert!(erro.to_string().contains("limite descomprimido"), "{nome}: {erro}");
        let pasta = raiz.path().join(nome);
        fs::create_dir(&pasta).unwrap();
        let erro = extrair(nome, &bytes, &pasta, 16_384, 1, Path::new("programa")).unwrap_err();
        assert!(erro.to_string().contains("limite de entradas"), "{nome}: {erro}");
    }
}

#[test]
fn recusa_entrada_zip_com_metodo_lzma_ou_xz() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opcoes = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("programa", opcoes).unwrap();
    zip.write_all(b"conteudo").unwrap();
    let zip = zip.finish().unwrap().into_inner();
    // O método de compressão fica a 8 bytes do início do cabeçalho local e a 10 do cabeçalho do diretório central.
    for metodo in [14_u16, 95] {
        let mut bytes = zip.clone();
        for (assinatura, deslocamento) in [([b'P', b'K', 3, 4], 8), ([b'P', b'K', 1, 2], 10)] {
            let inicio = bytes.windows(4).position(|janela| janela == assinatura).unwrap();
            bytes[inicio + deslocamento..inicio + deslocamento + 2].copy_from_slice(&metodo.to_le_bytes());
        }
        let pasta = pasta_temporaria();
        let erro = extrair("pacote.zip", &bytes, pasta.path(), 4096, 10, Path::new("programa")).unwrap_err();
        assert!(erro.to_string().contains("not supported"), "método {metodo}: {erro}");
    }
}

#[test]
fn recusa_entrada_zip_fora_do_destino_e_link_simbolico() {
    for link in [false, true] {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opcoes = zip::write::SimpleFileOptions::default();
        if link {
            zip.add_symlink("programa", "arquivo", opcoes).unwrap();
        } else {
            zip.start_file("../fora", opcoes).unwrap();
            zip.write_all(b"conteudo").unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let raiz = pasta_temporaria();
        let destino = raiz.path().join("agente");
        let hash = super::super::sha256_hex(&bytes);
        let erro = publicar(&destino, "pacote.zip", &bytes, &hash, Path::new("programa")).unwrap_err();
        assert!(erro.to_string().contains(if link { "link" } else { "fora da pasta" }), "{erro}");
        assert_eq!(fs::read_dir(raiz.path()).unwrap().count(), 0);
    }
}

#[test]
fn recusa_hardlink_e_fifo_no_tar() {
    for tipo in [tar::EntryType::Link, tar::EntryType::Fifo] {
        let bytes = tar_com_programa(|tar| {
            let mut cabecalho = tar::Header::new_gnu();
            cabecalho.set_entry_type(tipo);
            cabecalho.set_size(0);
            cabecalho.set_mode(0o644);
            if tipo.is_hard_link() {
                tar.append_link(&mut cabecalho, "extra", "bin/programa").unwrap();
            } else {
                tar.append_data(&mut cabecalho, "extra", io::empty()).unwrap();
            }
        });
        let raiz = pasta_temporaria();
        let destino = raiz.path().join("agente");
        let hash = super::super::sha256_hex(&bytes);
        let erro = publicar(&destino, "pacote.tar", &bytes, &hash, Path::new("bin/programa")).unwrap_err();
        assert!(erro.to_string().contains("link ou arquivo especial"));
        assert!(!destino.exists());
    }
}

#[test]
fn publica_tar_bzip2_e_tar_xz_minimos() {
    let bytes = tar_com_programa(|_| ());
    let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    bz.write_all(&bytes).unwrap();
    let mut xz = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(0)).unwrap();
    xz.write_all(&bytes).unwrap();
    for (nome, bytes) in [("pacote.tar.bz2", bz.finish().unwrap()), ("pacote.tar.xz", xz.finish().unwrap())] {
        let raiz = pasta_temporaria();
        let destino = raiz.path().join("agente");
        let cmd = Path::new("bin/programa");
        let hash = super::super::sha256_hex(&bytes);
        publicar(&destino, nome, &bytes, &hash, cmd).unwrap();
        assert!(cache_verificado(&destino, &hash, cmd).unwrap(), "{nome}");
        assert_eq!(fs::read(destino.join(cmd)).unwrap(), b"conteudo");
    }
}

#[test]
fn hash_incorreto_nao_publica_nem_substitui_instalacao() {
    let raiz = pasta_temporaria();
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
    let raiz = pasta_temporaria();
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
        assert_eq!(fs::metadata(destino.join(MARCADOR)).unwrap().permissions().mode() & 0o777, 0o600);
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
    let raiz = pasta_temporaria();
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
    let raiz = pasta_temporaria();
    let destino = raiz.path().join("1.0.0");
    let resultado =
        publicar(&destino, "pacote.tar", &bytes, &super::super::sha256_hex(&bytes), Path::new("bin/programa"));
    assert!(resultado.unwrap_err().to_string().contains("link"));
    assert!(!destino.exists());
}

#[cfg(unix)]
#[test]
fn recusa_executavel_em_cache_que_virou_link_ou_diretorio() {
    let raiz = pasta_temporaria();
    let hash = "a".repeat(64);
    fs::write(raiz.path().join(".installed"), format!("{hash}{hash}")).unwrap();
    fs::write(raiz.path().join("programa-real"), b"conteudo").unwrap();
    std::os::unix::fs::symlink("programa-real", raiz.path().join("link")).unwrap();
    fs::create_dir(raiz.path().join("pasta")).unwrap();
    for cmd in ["link", "pasta"] {
        let cmd = Path::new(cmd);
        assert!(!cache_verificado(raiz.path(), &hash, cmd).unwrap());
        assert!(executavel_regular(raiz.path(), cmd).is_err());
    }
}

#[test]
fn cache_sem_executavel_permite_reinstalar_a_mesma_versao() {
    let raiz = pasta_temporaria();
    let destino = raiz.path().join("agente");
    let cmd = Path::new("bin/agente");
    let bytes = b"agente ficticio";
    let hash = super::super::sha256_hex(bytes);
    publicar(&destino, "agente", bytes, &hash, cmd).unwrap();
    fs::remove_file(destino.join(cmd)).unwrap();
    assert!(destino.join(MARCADOR).is_file());
    assert!(!cache_verificado(&destino, &hash, cmd).unwrap());
    publicar(&destino, "agente", bytes, &hash, cmd).unwrap();
    assert!(cache_verificado(&destino, &hash, cmd).unwrap());
    assert_eq!(fs::read(destino.join(cmd)).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn cache_trata_links_e_diretorio_como_ausentes_sem_tocar_no_alvo_externo() {
    use std::os::unix::fs::symlink;

    let raiz = pasta_temporaria();
    let destino = raiz.path().join("agente");
    let fora = raiz.path().join("fora");
    fs::create_dir(&fora).unwrap();
    let cmd = Path::new("bin/agente");
    let bytes = b"agente ficticio";
    let hash = super::super::sha256_hex(bytes);
    publicar(&fora.join("referencia"), "agente", bytes, &hash, cmd).unwrap();

    for tipo in ["link", "diretorio", "link-no-pai"] {
        publicar(&destino, "agente", bytes, &hash, cmd).unwrap();
        fs::remove_file(destino.join(cmd)).unwrap();
        match tipo {
            "link" => symlink(fora.join("referencia").join(cmd), destino.join(cmd)).unwrap(),
            "diretorio" => fs::create_dir(destino.join(cmd)).unwrap(),
            _ => {
                fs::remove_dir(destino.join("bin")).unwrap();
                symlink(fora.join("referencia/bin"), destino.join("bin")).unwrap();
            }
        }
        assert!(!cache_verificado(&destino, &hash, cmd).unwrap(), "{tipo}");
        publicar(&destino, "agente", bytes, &hash, cmd).unwrap();
        assert!(cache_verificado(&destino, &hash, cmd).unwrap());
        assert_eq!(fs::read(fora.join("referencia").join(cmd)).unwrap(), bytes);
    }
}
