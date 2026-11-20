use super::*;
use serde_json::json;
use std::path::Path;

const URL: &str = "https://example.test/agente";
const VERSAO: &str = "1.2.3";
const PROGRAMA: &[u8] = b"programa de teste";

fn entrada(versao: &str, url: &str) -> Value {
    json!({
        "version": versao,
        "distribution": { "binary": { "linux-x86_64": { "archive": url, "sha256": "00", "cmd": "mutavel" } } }
    })
}

fn fixacao() -> Agente {
    let artefato = Artefato {
        url: URL.into(),
        sha256: "c61b59e443b3ca710dcbef6861c3e19d3643819085a8bf467639e1eeddf21c56".into(),
        cmd: "bin/agente".into(),
        bytes: PROGRAMA.len(),
        bytes_extraidos: PROGRAMA.len() as u64,
    };
    Agente { versao: VERSAO.into(), plataformas: BTreeMap::from([("linux-x86_64".into(), artefato)]) }
}

#[test]
fn publica_somente_bytes_que_conferem_com_o_hash_fixado_no_hub() {
    let fixacao = fixacao();
    let pacote = fixacao.selecionar(&entrada(VERSAO, URL), Some("linux-x86_64")).unwrap();
    let raiz = tempfile::tempdir().unwrap();
    let destino = raiz.path().canonicalize().unwrap().join("instalacao");
    let cmd = Path::new(pacote.cmd);
    assert!(cmd.ends_with("bin/agente"));
    assert_eq!(pacote.limite_download, PROGRAMA.len());
    let publicar =
        |bytes| super::super::arquivo::publicar(&destino, "agente", bytes, pacote.hash, cmd, pacote.limite_extraido);
    let erro = publicar(b"programa alterado").unwrap_err();
    assert!(erro.to_string().contains("SHA-256"));
    assert!(!destino.exists());
    publicar(PROGRAMA).unwrap();
    assert_eq!(std::fs::read(destino.join(cmd)).unwrap(), PROGRAMA);
}

#[test]
fn recusa_versao_ou_url_diferente_e_plataforma_sem_pin() {
    let fixacao = fixacao();
    for (versao, url, plataforma, mensagem) in [
        ("1.2.4", URL, "linux-x86_64", "versão verificada"),
        (VERSAO, "https://example.test/outro", "linux-x86_64", "endereço do binário mudou"),
        (VERSAO, URL, "darwin-aarch64", "Não há um binário verificado"),
    ] {
        let erro = fixacao.selecionar(&entrada(versao, url), Some(plataforma)).err().unwrap();
        assert!(erro.to_string().contains(mensagem), "{erro}");
    }
}

#[test]
fn os_dois_agentes_tem_os_dados_das_seis_plataformas_validados() {
    let plataformas =
        ["darwin-aarch64", "darwin-x86_64", "linux-aarch64", "linux-x86_64", "windows-aarch64", "windows-x86_64"];
    for id in ["cursor", "devin"] {
        let fixado = agente(&json!({"id": id})).unwrap();
        assert_eq!(fixado.plataformas.len(), plataformas.len());
        for plataforma in plataformas {
            let artefato = &fixado.plataformas[plataforma];
            let entrada = json!({
                "version": fixado.versao,
                "distribution": {"binary": {(plataforma): {"archive": artefato.url}}}
            });
            let pacote = fixado.selecionar(&entrada, Some(plataforma)).unwrap();
            assert!(pacote.url.starts_with("https://"));
            assert!(pacote.limite_extraido >= pacote.limite_download as u64);
            assert!(pacote.limite_download > 0);
            assert!(super::super::arquivo::caminho_relativo(Path::new(pacote.cmd)).is_ok());
        }
    }
}
