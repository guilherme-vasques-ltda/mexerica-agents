use super::{
    estado::{Estado, Fase},
    manifesto::{self, Artefato, Manifesto, Politica},
};
use ed25519_dalek::{Signer as _, SigningKey};

fn manifesto() -> Manifesto {
    let artefato = |nome: &str| Artefato { nome: nome.into(), tamanho: 10, sha256: "a".repeat(64) };
    Manifesto {
        produto: "mexerica".into(),
        canal: "estavel".into(),
        arquitetura: "x86_64".into(),
        versao: "1.2.3".into(),
        sequencia: 8,
        inicio: 100,
        expiracao: 200,
        hub: artefato("donnabot-host"),
        ajudante: artefato("donnabot-motor-exec"),
        sequencia_minima_reversivel: 4,
        wrapper_minimo: "1.0.0".into(),
    }
}

#[test]
fn assinatura_antes_do_parse_e_dominio_separado() {
    let mut seed = [0; 32];
    getrandom::fill(&mut seed).unwrap();
    let key = SigningKey::from_bytes(&seed);
    let bytes = serde_json::to_vec(&manifesto()).unwrap();
    let mut message = manifesto::DOMINIO.to_vec();
    message.extend_from_slice(&bytes);
    let assinatura = key.sign(&message).to_bytes();
    assert!(manifesto::verificar(&bytes, &assinatura, &key.verifying_key().to_bytes()).is_ok());
    let mut adulterado = bytes.clone();
    adulterado[0] ^= 1;
    assert!(manifesto::verificar(&adulterado, &assinatura, &key.verifying_key().to_bytes()).is_err());
    let assinatura_outro_dominio = key.sign(&bytes).to_bytes();
    assert!(manifesto::verificar(&bytes, &assinatura_outro_dominio, &key.verifying_key().to_bytes()).is_err());
    message.extend_from_slice(b" ");
    let nao_canonico = [bytes.as_slice(), b" "].concat();
    let assinatura_nao_canonica = key.sign(&message).to_bytes();
    assert!(manifesto::verificar(&nao_canonico, &assinatura_nao_canonica, &key.verifying_key().to_bytes()).is_err());
}

#[test]
fn validade_janela_sequencia_e_piso() {
    let versao = semver::Version::parse("1.0.0").unwrap();
    let politica = Politica {
        canal: "estavel",
        arquitetura: "x86_64",
        wrapper: &versao,
        tamanho_maximo: 100,
        janela_minima: 20,
        janela_maxima: 40,
    };
    let m = manifesto();
    assert_eq!(manifesto::validar(&m, &politica, 150, 7, 3).unwrap(), 190);
    assert!(manifesto::validar(&m, &politica, 200, 7, 3).is_err());
    assert!(manifesto::validar(&m, &politica, 181, 7, 3).is_err());
    assert!(manifesto::validar(&m, &politica, 150, 8, 3).is_err());
    assert!(manifesto::validar(&m, &politica, 150, 7, 5).is_err());
}

#[test]
fn aceite_reversao_e_expiracao() {
    let inicial = Estado { marca: 7, piso: 3, fase: Fase::Livre };
    let mut estado = inicial.clone();
    assert!(estado.iniciar(8, 4, Some(0), 200, 100).is_err());
    assert!(estado.iniciar(8, 4, Some(8), 200, 100).is_err());
    assert_eq!(estado, inicial);
    estado.iniciar(8, 4, Some(7), 200, 100).unwrap();
    assert!(estado.iniciar(9, 4, Some(8), 200, 100).is_err());
    estado.reverter(199).unwrap();
    estado.reverter(199).unwrap();
    assert_eq!((estado.marca, estado.piso), (8, 4));
    let mut estado = inicial.clone();
    estado.iniciar(8, 4, Some(7), 200, 100).unwrap();
    estado.aceitar(199).unwrap();
    assert!(estado.reverter(199).is_err());
    let mut estado = inicial;
    estado.iniciar(8, 4, Some(3), 200, 100).unwrap();
    assert!(estado.reverter(199).is_err());
    assert!(estado.aceitar(200).is_err());
    assert_eq!(estado.fase, Fase::Expirada);
    assert!(estado.iniciar(9, 4, Some(8), 300, 201).is_err());
    assert!(estado.reverter(201).is_err());
}
