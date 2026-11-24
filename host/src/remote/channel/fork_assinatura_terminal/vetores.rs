#![allow(clippy::unwrap_used)]

use super::{corte::*, *};
use ed25519_dalek::SigningKey;
use serde_json::Value;
use x25519_dalek::StaticSecret;

fn fixo<const N: usize>(v: &Value) -> [u8; N] {
    crypto::unb64_n(v.as_str().unwrap()).unwrap()
}

fn bytes(v: &Value) -> Vec<u8> {
    crypto::unb64(v.as_str().unwrap()).unwrap()
}

#[test]
fn vetores_de_assinatura_terminal() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../../docs/reference/fixtures/remote-relay-vectors.json")).unwrap();
    let identidade = Identity {
        sign: SigningKey::from_bytes(&fixo(&fixture["keys"]["hostSignSeed"])),
        box_secret: StaticSecret::from(fixo::<32>(&fixture["keys"]["hostBoxPriv"])),
    };
    let pubkey = fixo(&fixture["keys"]["hostSignPub"]);
    assert_eq!(identidade.sign_pub(), pubkey);
    let th = fixo(&fixture["handshake"]["transcriptHash"]);
    let vetores = &fixture["terminalProofsV1"];
    let workspace_epoch = vetores["workspaceEpoch"].as_u64().unwrap();
    let aberto_em = vetores["abertoEm"].as_u64().unwrap();
    let corte = |estavel| Corte { th, estavel, workspace_epoch, aberto_em };
    let recusa = |estavel| Recusa {
        cid: fixo(&fixture["keys"]["computerId"]),
        dk: fixo(&fixture["keys"]["deviceSignPub"]),
        ek_d: fixo(&fixture["handshake"]["deviceEphPub"]),
        n: fixo(&fixture["handshake"]["n"]),
        estavel,
    };

    for (label, estavel) in [
        ("device_revoked", Estavel::DeviceRevoked),
        ("workspace_revoked", Estavel::WorkspaceRevoked),
        ("role_changed", Estavel::RoleChanged),
    ] {
        let v = &vetores["corte"][label];
        let corte = corte(estavel);
        assert_eq!(corte.bytes().unwrap().as_slice(), bytes(&v["bytes"]), "{label}");
        assert_eq!(corte.assinar(&identidade).unwrap(), fixo(&v["sig"]), "{label}");
        assert!(corte.verificar(&pubkey, &bytes(&v["bytes"]), &bytes(&v["sig"])), "{label}");
    }
    for (label, estavel) in
        [("device_revoked", RecusaEstavel::DeviceRevoked), ("device_unpaired", RecusaEstavel::DeviceUnpaired)]
    {
        let v = &vetores["recusa"][label];
        let recusa = recusa(estavel);
        assert_eq!(recusa.bytes().as_slice(), bytes(&v["bytes"]), "{label}");
        assert_eq!(recusa.assinar(&identidade), fixo(&v["sig"]), "{label}");
        assert!(recusa.verificar(&pubkey, &bytes(&v["bytes"]), &bytes(&v["sig"])), "{label}");
    }

    let c = corte(Estavel::DeviceRevoked);
    let r = recusa(RecusaEstavel::DeviceRevoked);
    let negativos = &vetores["negativos"];
    let verificar = |tipo: &str, nome: &str| {
        let v = &negativos[tipo][nome];
        let msg = bytes(&v["bytes"]);
        let sig = bytes(&v["sig"]);
        match tipo {
            "corte" => c.verificar(&pubkey, &msg, &sig),
            "recusa" => r.verificar(&pubkey, &msg, &sig),
            _ => unreachable!(),
        }
    };
    for nome in [
        "outroTh",
        "outroAbertoEm",
        "outroStable",
        "stableInvalido",
        "prefixoClaim",
        "prefixoRecusa",
        "recusaComoCorte",
        "sigWelcome",
        "byteAMenos",
        "byteAMais",
        "sigAMenos",
        "sigAMais",
    ] {
        assert!(!verificar("corte", nome), "{nome}");
    }
    for nome in [
        "outroN",
        "outroEkD",
        "outroDk",
        "outroCid",
        "outroStable",
        "stableInvalido",
        "prefixoCorte",
        "prefixoClaim",
        "byteAMenos",
        "byteAMais",
        "sigAMenos",
        "sigAMais",
    ] {
        assert!(!verificar("recusa", nome), "{nome}");
    }
    for (nome, epoch_aberto) in [("epochIgual", workspace_epoch), ("epochMaior", workspace_epoch + 1)] {
        let v = &negativos["corte"][nome];
        let raw = bytes(&v["bytes"]);
        let sig = fixo(&v["sig"]);
        assert_eq!(raw[56..], epoch_aberto.to_be_bytes());
        crypto::verify(&pubkey, &raw, &sig).unwrap();
        let invalido = Corte { aberto_em: epoch_aberto, ..corte(Estavel::DeviceRevoked) };
        assert!(invalido.assinar(&identidade).is_none());
        assert!(!invalido.verificar(&pubkey, &raw, &sig));
    }
    let outro_th = fixo(&negativos["corte"]["outroTh"]["th"]);
    let outro_n = fixo(&negativos["recusa"]["outroN"]["n"]);
    assert!(!Corte { th: outro_th, ..c }.verificar(&pubkey, &c.bytes().unwrap(), &c.assinar(&identidade).unwrap()));
    assert!(!Recusa { n: outro_n, ..r }.verificar(&pubkey, &r.bytes(), &r.assinar(&identidade)));

    let welcome = fixo(&fixture["handshake"]["hs2Sig"]);
    assert_eq!(identidade.sign(&th), welcome);
    assert!(!c.verificar(&pubkey, &c.bytes().unwrap(), &welcome));
    assert!(!c.verificar(&pubkey, &th, &welcome));
}
