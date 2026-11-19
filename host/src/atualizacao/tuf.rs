//! Primitivas para verificar metadados TUF. Não habilitam atualização sozinhas.
use super::Hex as _;
use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_METADADOS: usize = 1024 * 1024;
const MAX_ASSINATURAS: usize = 64;

// JSON com nomes duplicados é ambíguo entre implementações; recusar antes de assinar.
struct Unico(Value);
impl<'de> Deserialize<'de> for Unico {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Visita;
        impl<'de> Visitor<'de> for Visita {
            type Value = Unico;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON sem chaves duplicadas")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Unico, E> {
                Ok(Unico(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Unico, E> {
                Ok(Unico(Value::Number(v.into())))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Unico, E> {
                Ok(Unico(Value::Number(v.into())))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Unico, E> {
                Ok(Unico(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Unico, E> {
                Ok(Unico(Value::String(v)))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Unico, E> {
                Ok(Unico(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Unico, E> {
                Ok(Unico(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Unico, A::Error> {
                let mut values = Vec::new();
                while let Some(v) = seq.next_element::<Unico>()? {
                    values.push(v.0);
                }
                Ok(Unico(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Unico, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, Unico>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(de::Error::custom("chave duplicada"));
                    }
                }
                Ok(Unico(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visita)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    signed: Unico,
    signatures: Vec<Assinatura>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assinatura {
    keyid: String,
    sig: String,
}
#[derive(Clone, Deserialize)]
pub struct Chave {
    pub keytype: String,
    pub scheme: String,
    pub keyval: ValorChave,
}
#[derive(Clone, Deserialize)]
pub struct ValorChave {
    pub public: String,
}
#[derive(Clone, Deserialize)]
pub struct Papel {
    pub keyids: Vec<String>,
    pub threshold: usize,
}

fn hex<const N: usize>(s: &str) -> Result<[u8; N]> {
    ensure!(s.len() == N * 2 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "hex inválido");
    let mut saida = [0u8; N];
    for (i, par) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digito = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
        saida[i] = digito(par[0]) * 16 + digito(par[1]);
    }
    Ok(saida)
}

/// Subconjunto sem caracteres de controle nem números reais. Recusar metadados
/// que não caibam no POUF reduzido até haver interoperabilidade comprovada.
pub(super) fn canonico(valor: &Value, out: &mut Vec<u8>) -> Result<()> {
    match valor {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            ensure!(n.is_i64() || n.is_u64(), "número real recusado");
            out.extend_from_slice(n.to_string().as_bytes());
        }
        Value::String(s) => {
            ensure!(s.chars().all(|c| !c.is_control()), "controle em metadados");
            out.extend_from_slice(&serde_json::to_vec(s)?);
        }
        Value::Array(a) => {
            out.push(b'[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                canonico(v, out)?;
            }
            out.push(b']');
        }
        Value::Object(m) => {
            out.push(b'{');
            let mut chaves: Vec<_> = m.keys().collect();
            chaves.sort_unstable();
            for (i, chave) in chaves.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                canonico(&Value::String(chave.clone()), out)?;
                out.push(b':');
                canonico(m.get(chave).context("chave ausente")?, out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

/// Verifica limiar de chaves públicas distintas antes de interpretar `signed`.
pub fn verificar(bytes: &[u8], papel: &Papel, chaves: &BTreeMap<String, Chave>) -> Result<Value> {
    ensure!(!bytes.is_empty() && bytes.len() <= MAX_METADADOS, "metadados grandes demais");
    ensure!(
        papel.threshold > 0 && papel.keyids.len() <= MAX_ASSINATURAS && papel.threshold <= papel.keyids.len(),
        "limiar inválido"
    );
    let envelope: Envelope = serde_json::from_slice(bytes).context("envelope TUF inválido")?;
    ensure!(envelope.signatures.len() <= MAX_ASSINATURAS, "assinaturas demais");
    let autorizadas: BTreeSet<_> = papel.keyids.iter().collect();
    ensure!(autorizadas.len() == papel.keyids.len(), "keyid duplicado");
    let mut mensagem = Vec::new();
    canonico(&envelope.signed.0, &mut mensagem)?;
    let mut confirmadas = BTreeSet::new();
    for assinatura in envelope.signatures {
        if !autorizadas.contains(&assinatura.keyid) {
            continue;
        }
        let Some(chave) = chaves.get(&assinatura.keyid) else { continue };
        if chave.keytype != "ed25519" || chave.scheme != "ed25519" {
            continue;
        }
        let Ok(publica) = hex::<32>(&chave.keyval.public) else { continue };
        let Ok(verificadora) = VerifyingKey::from_bytes(&publica) else { continue };
        let Ok(bytes_assinatura) = hex::<64>(&assinatura.sig) else { continue };
        if verificadora.verify_strict(&mensagem, &Signature::from_bytes(&bytes_assinatura)).is_ok() {
            confirmadas.insert(publica);
        }
    }
    ensure!(confirmadas.len() >= papel.threshold, "limiar TUF não atingido");
    Ok(envelope.signed.0)
}

#[derive(Deserialize)]
pub struct Raiz {
    #[serde(rename = "_type")]
    tipo: String,
    spec_version: String,
    pub version: u64,
    expires: String,
    pub keys: BTreeMap<String, Chave>,
    pub roles: BTreeMap<String, Papel>,
}

fn validade(tipo: &str, versao: u64, expiracao: &str, esperado: &str, agora: i64) -> Result<()> {
    ensure!(tipo == esperado && versao > 0, "tipo ou versão TUF inválidos");
    let data = chrono::DateTime::parse_from_rfc3339(expiracao).context("data TUF inválida")?;
    ensure!(data.timestamp() > agora, "metadados TUF expirados");
    Ok(())
}

impl Raiz {
    fn papel(&self, nome: &str) -> Result<&Papel> {
        self.roles.get(nome).context("papel TUF ausente")
    }
    fn validar(&self, agora: i64) -> Result<()> {
        validade(&self.tipo, self.version, &self.expires, "root", agora)?;
        self.validar_estrutura()
    }

    /// Estrutura, papéis e chaves; sem prazo. Raízes intermediárias de uma
    /// rotação podem estar vencidas, somente a final precisa estar válida.
    fn validar_estrutura(&self) -> Result<()> {
        ensure!(self.tipo == "root" && self.version > 0, "tipo ou versão TUF inválidos");
        chrono::DateTime::parse_from_rfc3339(&self.expires).context("data TUF inválida")?;
        let versao = semver::Version::parse(&self.spec_version)?;
        ensure!(versao.major == 1, "versão TUF incompatível");
        let mut todas = BTreeSet::new();
        for nome in ["root", "timestamp", "snapshot", "targets"] {
            let papel = self.papel(nome)?;
            ensure!(
                papel.threshold > 0 && papel.threshold <= papel.keyids.len() && papel.keyids.len() <= MAX_ASSINATURAS,
                "papel TUF inválido"
            );
            let mut publicas = BTreeSet::new();
            for id in &papel.keyids {
                let chave = self.keys.get(id).context("chave TUF ausente")?;
                ensure!(chave.keytype == "ed25519" && chave.scheme == "ed25519", "tipo de chave não suportado");
                let publica = hex::<32>(&chave.keyval.public)?;
                ensure!(publicas.insert(publica) && todas.insert(publica), "chave TUF repetida entre papéis");
            }
        }
        Ok(())
    }

    /// Chaves públicas efetivas de um papel, para detectar rotação de chaves online.
    pub fn chaves_do_papel(&self, nome: &str) -> Result<BTreeSet<[u8; 32]>> {
        self.papel(nome)?
            .keyids
            .iter()
            .map(|id| hex::<32>(&self.keys.get(id).context("chave TUF ausente")?.keyval.public))
            .collect()
    }
}

/// A instalação administrativa deve autenticar os bytes pinados antes desta chamada.
pub fn carregar_raiz_pinada(bytes: &[u8], agora: i64) -> Result<Raiz> {
    ensure!(!bytes.is_empty() && bytes.len() <= MAX_METADADOS, "raiz TUF grande demais");
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    let raiz: Raiz = serde_json::from_value(envelope.signed.0)?;
    raiz.validar(agora)?;
    verificar(bytes, raiz.papel("root")?, &raiz.keys)?;
    Ok(raiz)
}

/// Raiz já confiável persistida (pinada ou rotacionada), sem exigir prazo:
/// a rotação parte dela mesmo vencida; o uso final exige `carregar_raiz_pinada`.
pub fn carregar_raiz_confiavel(bytes: &[u8]) -> Result<Raiz> {
    ensure!(!bytes.is_empty() && bytes.len() <= MAX_METADADOS, "raiz TUF grande demais");
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    let raiz: Raiz = serde_json::from_value(envelope.signed.0)?;
    raiz.validar_estrutura()?;
    verificar(bytes, raiz.papel("root")?, &raiz.keys)?;
    Ok(raiz)
}

/// A rotação só aceita uma versão por vez e exige ambos os quóruns. O prazo
/// da nova raiz é conferido por quem a usa ao fim da cadeia (TUF 5.3.10).
pub fn rotacionar_raiz(anterior: &Raiz, bytes: &[u8]) -> Result<Raiz> {
    ensure!(!bytes.is_empty() && bytes.len() <= MAX_METADADOS, "raiz TUF grande demais");
    let valor = verificar(bytes, anterior.papel("root")?, &anterior.keys)?;
    let nova: Raiz = serde_json::from_value(valor)?;
    ensure!(anterior.version.checked_add(1) == Some(nova.version), "versão de raiz não sequencial");
    nova.validar_estrutura()?;
    verificar(bytes, nova.papel("root")?, &nova.keys)?;
    Ok(nova)
}

#[derive(Deserialize)]
pub struct Cabecalho {
    #[serde(rename = "_type")]
    tipo: String,
    pub version: u64,
    expires: String,
    spec_version: String,
}

/// Chamador deve persistir a maior versão de cada papel e vincular os hashes
/// de timestamp a snapshot, de snapshot a targets e de targets ao arquivo.
pub fn verificar_papel(bytes: &[u8], raiz: &Raiz, nome: &str, agora: i64, versao_minima: u64) -> Result<Value> {
    ensure!(matches!(nome, "timestamp" | "snapshot" | "targets"), "papel inválido");
    let valor = verificar(bytes, raiz.papel(nome)?, &raiz.keys)?;
    let cabecalho: Cabecalho = serde_json::from_value(valor.clone())?;
    validade(&cabecalho.tipo, cabecalho.version, &cabecalho.expires, nome, agora)?;
    ensure!(
        semver::Version::parse(&cabecalho.spec_version)?.major == 1 && cabecalho.version >= versao_minima,
        "versão TUF recusada"
    );
    Ok(valor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde_json::json;

    fn texto_hex(bytes: &[u8]) -> String {
        bytes.hex()
    }
    fn chave() -> SigningKey {
        let mut seed = [0; 32];
        getrandom::fill(&mut seed).unwrap();
        SigningKey::from_bytes(&seed)
    }
    fn envelope(valor: &Value, key: &SigningKey, id: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        canonico(valor, &mut bytes).unwrap();
        serde_json::to_vec(
            &json!({"signed": valor, "signatures": [{"keyid":id,"sig":texto_hex(&key.sign(&bytes).to_bytes())}]}),
        )
        .unwrap()
    }
    #[test]
    fn limiar_rejeita_chaves_faltantes_e_json_ambiguo() {
        let key = chave();
        let publica = texto_hex(key.verifying_key().as_bytes());
        let chaves: BTreeMap<String, Chave> = serde_json::from_value(json!({
            "a":{"keytype":"ed25519","scheme":"ed25519","keyval":{"public":publica}}
        }))
        .unwrap();
        let papel = Papel { keyids: vec!["a".into()], threshold: 1 };
        let bytes = envelope(&json!({"_type":"targets","version":1}), &key, "a");
        assert!(verificar(&bytes, &papel, &chaves).is_ok());
        assert!(verificar(&bytes, &Papel { keyids: vec!["a".into(), "b".into()], threshold: 2 }, &chaves).is_err());
        assert!(verificar(br#"{"signed":{"version":1,"version":2},"signatures":[]}"#, &papel, &chaves).is_err());
    }

    #[test]
    fn raiz_pinada_e_expiracao() {
        let key = chave();
        let outras = [chave(), chave(), chave()];
        let public = |k: &SigningKey| json!({"keytype":"ed25519","scheme":"ed25519","keyval":{"public":texto_hex(k.verifying_key().as_bytes())}});
        let raiz = json!({"_type":"root", "spec_version":"1.0.0", "version":1,
            "expires":"2030-01-01T00:00:00Z",
            "keys":{"a":public(&key),"b":public(&outras[0]),"c":public(&outras[1]),"d":public(&outras[2])},
            "roles":{"root":{"keyids":["a"],"threshold":1},"targets":{"keyids":["b"],"threshold":1},
            "timestamp":{"keyids":["c"],"threshold":1},"snapshot":{"keyids":["d"],"threshold":1}}});
        let bytes = envelope(&raiz, &key, "a");
        assert!(carregar_raiz_pinada(&bytes, 1_800_000_000).is_ok());
        assert!(carregar_raiz_pinada(&bytes, 2_000_000_000).is_err());
    }
    #[test]
    fn rotacao_exige_quorum_antigo_e_novo() {
        let antiga = chave();
        let nova = chave();
        let alvos = chave();
        let snapshot = chave();
        let timestamp = chave();
        let public = |k: &SigningKey| json!({"keytype":"ed25519","scheme":"ed25519","keyval":{"public":texto_hex(k.verifying_key().as_bytes())}});
        let roles = json!({"root":{"keyids":["a"],"threshold":1},"targets":{"keyids":["b"],"threshold":1},
            "snapshot":{"keyids":["c"],"threshold":1},"timestamp":{"keyids":["d"],"threshold":1}});
        let raiz = json!({"_type":"root","spec_version":"1.0.0","version":1,"expires":"2030-01-01T00:00:00Z",
            "keys":{"a":public(&antiga),"b":public(&alvos),"c":public(&snapshot),"d":public(&timestamp)},"roles":roles});
        let raiz_antiga = carregar_raiz_pinada(&envelope(&raiz.clone(), &antiga, "a"), 1_800_000_000).unwrap();
        let mut proxima = raiz;
        proxima["version"] = json!(2);
        proxima["keys"]["nova"] = public(&nova);
        proxima["roles"]["root"]["keyids"] = json!(["nova"]);
        let so_antiga = envelope(&proxima.clone(), &antiga, "a");
        assert!(rotacionar_raiz(&raiz_antiga, &so_antiga).is_err());
        let mut dados = Vec::new();
        canonico(&proxima, &mut dados).unwrap();
        let ambas = serde_json::to_vec(&json!({"signed":proxima,"signatures":[
            {"keyid":"a","sig":texto_hex(&antiga.sign(&dados).to_bytes())},
            {"keyid":"nova","sig":texto_hex(&nova.sign(&dados).to_bytes())}]}))
        .unwrap();
        assert_eq!(rotacionar_raiz(&raiz_antiga, &ambas).unwrap().version, 2);
    }

    #[test]
    fn cadeia_vincula_hashes_e_permite_repeticao_identica() {
        use sha2::{Digest as _, Sha256};
        let key = chave();
        let targets_key = chave();
        let snapshot_key = chave();
        let timestamp_key = chave();
        let public = |k: &SigningKey| json!({"keytype":"ed25519","scheme":"ed25519","keyval":{"public":texto_hex(k.verifying_key().as_bytes())}});
        let root = json!({"_type":"root", "spec_version":"1.0.0", "version":1,
            "expires":"2030-01-01T00:00:00Z", "keys":{"a":public(&key),"b":public(&targets_key),"c":public(&snapshot_key),"d":public(&timestamp_key)},
            "roles":{"root":{"keyids":["a"],"threshold":1},"targets":{"keyids":["b"],"threshold":1},
            "snapshot":{"keyids":["c"],"threshold":1},"timestamp":{"keyids":["d"],"threshold":1}}});
        let raiz = carregar_raiz_pinada(&envelope(&root, &key, "a"), 1_800_000_000).unwrap();
        let referencia = |bytes: &[u8]| json!({"version":1,"length":bytes.len(),"hashes":{"sha256":texto_hex(&Sha256::digest(bytes))}});
        let manifesto = b"manifesto de teste";
        let alvo = json!({"length":manifesto.len(),"hashes":{"sha256":texto_hex(&Sha256::digest(manifesto))}});
        let targets = envelope(
            &json!({"_type":"targets","spec_version":"1.0.0","version":1,
            "expires":"2030-01-01T00:00:00Z", "targets":{"manifesto.json":alvo}}),
            &targets_key,
            "b",
        );
        let snapshot = envelope(
            &json!({"_type":"snapshot","spec_version":"1.0.0","version":1,
            "expires":"2030-01-01T00:00:00Z", "meta":{"targets.json":referencia(&targets)}}),
            &snapshot_key,
            "c",
        );
        let timestamp = envelope(
            &json!({"_type":"timestamp","spec_version":"1.0.0","version":1,
            "expires":"2030-01-01T00:00:00Z", "meta":{"snapshot.json":referencia(&snapshot)}}),
            &timestamp_key,
            "d",
        );
        let delegados = BTreeMap::new();
        let anteriores = BTreeMap::new();
        let cadeia = Cadeia {
            timestamp: &timestamp,
            snapshot: &snapshot,
            targets: &targets,
            delegados: &delegados,
            anteriores: &anteriores,
        };
        assert!(listar_delegados(&raiz, &timestamp, &snapshot, &anteriores, 1_800_000_000).unwrap().is_empty());
        assert!(listar_delegados(&raiz, &timestamp, b"adulterado", &anteriores, 1_800_000_000).is_err());
        let snapshot_delegado = envelope(
            &json!({"_type":"snapshot","spec_version":"1.0.0","version":1,
            "expires":"2030-01-01T00:00:00Z", "meta":{"targets.json":referencia(&targets), "release-1.json":referencia(b"dados")}}),
            &snapshot_key,
            "c",
        );
        let timestamp_delegado = envelope(
            &json!({"_type":"timestamp","spec_version":"1.0.0","version":1,
            "expires":"2030-01-01T00:00:00Z", "meta":{"snapshot.json":referencia(&snapshot_delegado)}}),
            &timestamp_key,
            "d",
        );
        assert_eq!(
            listar_delegados(&raiz, &timestamp_delegado, &snapshot_delegado, &anteriores, 1_800_000_000)
                .unwrap()
                .get("release-1"),
            Some(&5)
        );
        assert!(listar_delegados(&raiz, &timestamp, &snapshot_delegado, &anteriores, 1_800_000_000).is_err());
        let resultado = verificar_cadeia(&raiz, &cadeia, 1_800_000_000, "manifesto.json").unwrap();
        assert!(verificar_alvo(manifesto, &resultado.alvo).is_ok());
        assert!(verificar_alvo(b"outro", &resultado.alvo).is_err());
        let replay = Cadeia { anteriores: &resultado.versoes, ..cadeia };
        assert!(verificar_cadeia(&raiz, &replay, 1_800_000_000, "manifesto.json").is_ok());
        let mut alterado = serde_json::from_slice::<Value>(&timestamp).unwrap();
        alterado["signed"]["expires"] = json!("2031-01-01T00:00:00Z");
        let adulterado = envelope(&alterado["signed"].clone(), &timestamp_key, "d");
        let equivocado = Cadeia { timestamp: &adulterado, ..replay };
        assert!(verificar_cadeia(&raiz, &equivocado, 1_800_000_000, "manifesto.json").is_err());
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VersaoConfiavel {
    pub versao: u64,
    pub sha256: String,
}

fn hash_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes).hex()
}

fn conferir_versao(anterior: Option<&VersaoConfiavel>, versao: u64, hash: &str) -> Result<()> {
    let _ = hex::<32>(hash)?;
    ensure!(versao > 0, "versão TUF inválida");
    if let Some(anterior) = anterior {
        ensure!(
            versao > anterior.versao || (versao == anterior.versao && hash == anterior.sha256),
            "rollback ou metadado alterado na mesma versão"
        );
    }
    Ok(())
}

#[derive(Clone, Deserialize)]
pub struct Referencia {
    pub version: u64,
    pub length: u64,
    pub hashes: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Metadados {
    meta: BTreeMap<String, Referencia>,
}
#[derive(Clone, Deserialize)]
pub struct Alvo {
    pub length: u64,
    pub hashes: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Alvos {
    targets: BTreeMap<String, Alvo>,
    #[serde(default)]
    delegations: Option<Delegacoes>,
}
#[derive(Deserialize)]
struct Delegacoes {
    keys: BTreeMap<String, Chave>,
    roles: Vec<Delegacao>,
}
#[derive(Deserialize)]
struct Delegacao {
    name: String,
    keyids: Vec<String>,
    threshold: usize,
    terminating: bool,
    paths: Vec<String>,
}

fn conferir_bytes(bytes: &[u8], tamanho: u64, hashes: &BTreeMap<String, String>) -> Result<()> {
    use sha2::{Digest as _, Sha256};
    ensure!(u64::try_from(bytes.len())? == tamanho, "tamanho TUF divergente");
    let esperado = hashes.get("sha256").context("SHA-256 obrigatório")?;
    let esperado = hex::<32>(esperado)?;
    let obtido = Sha256::digest(bytes);
    ensure!(obtido[..] == esperado[..], "hash TUF divergente");
    Ok(())
}

fn cabecalho(valor: &Value, tipo: &str, agora: i64, minimo: u64) -> Result<u64> {
    let cabecalho: Cabecalho = serde_json::from_value(valor.clone())?;
    validade(&cabecalho.tipo, cabecalho.version, &cabecalho.expires, tipo, agora)?;
    ensure!(
        semver::Version::parse(&cabecalho.spec_version)?.major == 1 && cabecalho.version >= minimo,
        "versão TUF recusada"
    );
    Ok(cabecalho.version)
}

/// Entrada de dados já baixados. As referências de versão e hash são exigidas,
/// mesmo quando opcionais no padrão TUF; o repositório deve usar este POUF.
pub struct Cadeia<'a> {
    pub timestamp: &'a [u8],
    pub snapshot: &'a [u8],
    pub targets: &'a [u8],
    pub delegados: &'a BTreeMap<String, Vec<u8>>,
    pub anteriores: &'a BTreeMap<String, VersaoConfiavel>,
}

pub struct ResultadoCadeia {
    pub alvo: Alvo,
    pub versoes: BTreeMap<String, VersaoConfiavel>,
}

/// Descobre apenas nomes presentes no snapshot autenticado pelo timestamp.
/// A autorização das delegações ainda é conferida pela cadeia completa.
pub fn listar_delegados(
    raiz: &Raiz,
    timestamp: &[u8],
    snapshot: &[u8],
    anteriores: &BTreeMap<String, VersaoConfiavel>,
    agora: i64,
) -> Result<BTreeMap<String, u64>> {
    raiz.validar(agora)?;
    let minimo = |nome: &str| anteriores.get(nome).map_or(1, |v| v.versao);
    let ts = verificar_papel(timestamp, raiz, "timestamp", agora, minimo("timestamp"))?;
    let ts_versao = cabecalho(&ts, "timestamp", agora, minimo("timestamp"))?;
    conferir_versao(anteriores.get("timestamp"), ts_versao, &hash_bytes(timestamp))?;
    let ts: Metadados = serde_json::from_value(ts)?;
    let referencia = ts.meta.get("snapshot.json").context("snapshot ausente do timestamp")?;
    conferir_bytes(snapshot, referencia.length, &referencia.hashes)?;
    let snap = verificar_papel(snapshot, raiz, "snapshot", agora, minimo("snapshot"))?;
    let versao = cabecalho(&snap, "snapshot", agora, minimo("snapshot"))?;
    ensure!(versao == referencia.version, "versão de snapshot divergente");
    conferir_versao(anteriores.get("snapshot"), versao, &hash_bytes(snapshot))?;
    let snap: Metadados = serde_json::from_value(snap)?;
    ensure!(snap.meta.contains_key("targets.json") && snap.meta.len() <= 65, "metadados de snapshot inválidos");
    for (nome, anterior) in anteriores {
        if let Some(papel) = nome.strip_prefix("meta:") {
            let referencia = snap.meta.get(papel).context("metadado anteriormente confiável removido")?;
            conferir_versao(
                Some(anterior),
                referencia.version,
                referencia.hashes.get("sha256").context("SHA-256 obrigatório")?,
            )?;
        }
    }
    let mut nomes = BTreeMap::new();
    for (nome, referencia) in snap.meta {
        if nome == "targets.json" {
            continue;
        }
        let papel = nome.strip_suffix(".json").context("nome de metadado inválido")?;
        ensure!(
            !papel.is_empty()
                && papel.len() <= 64
                && papel.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "nome de papel delegado inválido"
        );
        ensure!(referencia.length > 0 && referencia.length <= 1024 * 1024, "tamanho de metadado delegado inválido");
        conferir_versao(None, referencia.version, referencia.hashes.get("sha256").context("SHA-256 obrigatório")?)?;
        nomes.insert(papel.to_owned(), referencia.length);
    }
    Ok(nomes)
}

/// A chamada verifica a hierarquia inteira antes de devolver a descrição de um
/// único alvo fixo. Versões retornadas só são úteis após persistência durável.
pub fn verificar_cadeia(raiz: &Raiz, cadeia: &Cadeia<'_>, agora: i64, alvo: &str) -> Result<ResultadoCadeia> {
    raiz.validar(agora)?;
    ensure!(alvo == "manifesto.json", "alvo não permitido");
    let minimo = |nome: &str| -> u64 { cadeia.anteriores.get(nome).map_or(1, |anterior| anterior.versao) };
    let timestamp = verificar_papel(cadeia.timestamp, raiz, "timestamp", agora, minimo("timestamp"))?;
    let ts_versao = cabecalho(&timestamp, "timestamp", agora, minimo("timestamp"))?;
    let ts_hash = hash_bytes(cadeia.timestamp);
    conferir_versao(cadeia.anteriores.get("timestamp"), ts_versao, &ts_hash)?;
    let ts: Metadados = serde_json::from_value(timestamp)?;
    let snap_ref = ts.meta.get("snapshot.json").context("snapshot ausente do timestamp")?;
    conferir_bytes(cadeia.snapshot, snap_ref.length, &snap_ref.hashes)?;
    let snapshot = verificar_papel(cadeia.snapshot, raiz, "snapshot", agora, minimo("snapshot"))?;
    let snap_versao = cabecalho(&snapshot, "snapshot", agora, minimo("snapshot"))?;
    ensure!(snap_versao == snap_ref.version, "versão de snapshot divergente");
    let snap_hash = hash_bytes(cadeia.snapshot);
    conferir_versao(cadeia.anteriores.get("snapshot"), snap_versao, &snap_hash)?;
    let snap: Metadados = serde_json::from_value(snapshot)?;
    for (nome, anterior) in cadeia.anteriores {
        if let Some(papel) = nome.strip_prefix("meta:") {
            let referencia = snap.meta.get(papel).context("metadado anteriormente confiável removido")?;
            let hash = referencia.hashes.get("sha256").context("SHA-256 obrigatório")?;
            conferir_versao(Some(anterior), referencia.version, hash)?;
        }
    }
    let targets_ref = snap.meta.get("targets.json").context("targets ausente do snapshot")?;
    conferir_bytes(cadeia.targets, targets_ref.length, &targets_ref.hashes)?;
    let targets = verificar_papel(cadeia.targets, raiz, "targets", agora, minimo("meta:targets.json"))?;
    ensure!(
        cabecalho(&targets, "targets", agora, minimo("meta:targets.json"))? == targets_ref.version,
        "versão de targets divergente"
    );
    let mut proibidas = BTreeSet::new();
    for nome in ["root", "timestamp", "snapshot"] {
        for id in &raiz.papel(nome)?.keyids {
            proibidas.insert(hex::<32>(&raiz.keys.get(id).context("chave TUF ausente")?.keyval.public)?);
        }
    }
    let mut visitados = BTreeSet::new();
    let resultado = buscar_alvo(targets, alvo, &snap.meta, cadeia, agora, &proibidas, &mut visitados, 0)?
        .context("alvo TUF não autorizado")?;
    let mut versoes = BTreeMap::new();
    versoes.insert("timestamp".into(), VersaoConfiavel { versao: ts_versao, sha256: ts_hash });
    versoes.insert("snapshot".into(), VersaoConfiavel { versao: snap_versao, sha256: snap_hash });
    for (nome, referencia) in snap.meta {
        let hash = referencia.hashes.get("sha256").context("SHA-256 obrigatório")?;
        conferir_versao(None, referencia.version, hash)?;
        versoes.insert(format!("meta:{nome}"), VersaoConfiavel { versao: referencia.version, sha256: hash.clone() });
    }
    Ok(ResultadoCadeia { alvo: resultado, versoes })
}

#[expect(
    clippy::too_many_arguments,
    reason = "recursão sobre a cadeia autenticada; agrupar exigiria estado mutável compartilhado"
)]
fn buscar_alvo(
    valor: Value,
    alvo: &str,
    snapshot: &BTreeMap<String, Referencia>,
    cadeia: &Cadeia<'_>,
    agora: i64,
    proibidas: &BTreeSet<[u8; 32]>,
    visitados: &mut BTreeSet<String>,
    profundidade: usize,
) -> Result<Option<Alvo>> {
    ensure!(profundidade <= 16 && visitados.len() <= 64, "limite de delegações excedido");
    let alvos: Alvos = serde_json::from_value(valor)?;
    if let Some(alvo) = alvos.targets.get(alvo) {
        return Ok(Some(alvo.clone()));
    }
    let Some(delegacoes) = alvos.delegations else { return Ok(None) };
    ensure!(delegacoes.roles.len() <= 64, "delegações demais");
    for delegado in delegacoes.roles {
        // Este POUF admite somente paths exatos; padrões e hash-bin exigem
        // suporte adicional, em vez de serem interpretados incorretamente.
        ensure!(delegado.paths.iter().all(|p| p == "manifesto.json"), "caminho delegado não suportado");
        if !delegado.paths.iter().any(|p| p == alvo) {
            continue;
        }
        ensure!(
            !delegado.name.is_empty()
                && delegado.name.len() <= 64
                && delegado.name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "nome de papel delegado inválido"
        );
        ensure!(visitados.insert(delegado.name.clone()), "ciclo em delegações");
        let nome = format!("{}.json", delegado.name);
        let referencia = snapshot.get(&nome).context("delegação ausente do snapshot")?;
        let bytes = cadeia.delegados.get(&delegado.name).context("metadado delegado ausente")?;
        conferir_bytes(bytes, referencia.length, &referencia.hashes)?;
        let papel = Papel { keyids: delegado.keyids, threshold: delegado.threshold };
        for id in &papel.keyids {
            let chave = delegacoes.keys.get(id).context("chave delegada ausente")?;
            ensure!(!proibidas.contains(&hex::<32>(&chave.keyval.public)?), "chave online ou raiz em papel delegado");
        }
        let dados = verificar(bytes, &papel, &delegacoes.keys)?;
        let anterior = cadeia.anteriores.get(&format!("meta:{nome}")).map_or(1, |v| v.versao);
        ensure!(cabecalho(&dados, "targets", agora, anterior)? == referencia.version, "versão delegada divergente");
        if let Some(resultado) =
            buscar_alvo(dados, alvo, snapshot, cadeia, agora, proibidas, visitados, profundidade + 1)?
        {
            return Ok(Some(resultado));
        }
        if delegado.terminating {
            return Ok(None);
        }
    }
    Ok(None)
}

pub fn verificar_alvo(bytes: &[u8], alvo: &Alvo) -> Result<()> {
    conferir_bytes(bytes, alvo.length, &alvo.hashes)
}
