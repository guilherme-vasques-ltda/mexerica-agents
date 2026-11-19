//! M3-D: pedido inicial, sem atestar escopos concedidos ou perfis curados.
use anyhow::{Result, ensure};
use reqwest::{StatusCode, header::WWW_AUTHENTICATE};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) const ERRO: &str = "O serviço informou permissões inválidas ou conflitantes; login recusado.";

/// Sonda da própria tentativa; sem cookies, headers configurados ou credenciais.
/// O chamador revalida o snapshot/Origin depois deste await, inclusive no erro.
pub(crate) async fn inicial(url: &str) -> Result<Option<reqwest::Response>> {
    let resposta = super::oauth_urls::cliente()?
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":crate::mcp::PROTOCOL_VERSION,"capabilities":{},
            "clientInfo":{"name":"codync","version":env!("CARGO_PKG_VERSION")}
        }}))
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await;
    Ok(resposta.ok())
}

pub(crate) fn capturar(resposta: Option<reqwest::Response>) -> Result<Option<Vec<String>>> {
    let Some(resposta) = resposta else { return Ok(None) };
    if resposta.status() != StatusCode::UNAUTHORIZED {
        return Ok(None);
    }
    let headers = resposta.headers().get_all(WWW_AUTHENTICATE);
    let valores: Vec<_> = headers.iter().map(reqwest::header::HeaderValue::as_bytes).collect();
    desafio(&valores)
}

fn token(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)
}
fn scope_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c == 0x21 || (0x23..=0x5b).contains(&c) || (0x5d..=0x7e).contains(&c))
}
pub(super) fn conjunto(s: &str) -> Result<Vec<String>> {
    let mut scopes = Vec::new();
    let mut seen = BTreeSet::new();
    for s in s.split(' ') {
        ensure!(scope_token(s), ERRO);
        if seen.insert(s) {
            scopes.push(s.to_owned());
        }
    }
    Ok(scopes)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Parser<'a> {
    fn ows(&mut self) {
        while self.bytes.get(self.pos).is_some_and(|c| matches!(c, b' ' | b'\t')) {
            self.pos += 1;
        }
    }
    /// RFC 9110 §5.6.1.2: ignora até 32 separadores vazios por conjunto de headers.
    fn vazios(&mut self, count: &mut usize) -> Result<()> {
        self.ows();
        while self.bytes.get(self.pos) == Some(&b',') {
            *count += 1;
            ensure!(*count <= 32, ERRO);
            self.pos += 1;
            self.ows();
        }
        Ok(())
    }
    fn nome(&mut self) -> Result<&'a [u8]> {
        let start = self.pos;
        while self.bytes.get(self.pos).is_some_and(|c| token(*c)) {
            self.pos += 1;
        }
        ensure!(self.pos > start, ERRO);
        Ok(&self.bytes[start..self.pos])
    }
    fn valor(&mut self) -> Result<Vec<u8>> {
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Ok(self.nome()?.to_vec());
        }
        self.pos += 1;
        let mut value = Vec::new();
        while let Some(&c) = self.bytes.get(self.pos) {
            self.pos += 1;
            if c == b'"' {
                return Ok(value);
            }
            let c = if c == b'\\' {
                let c = *self.bytes.get(self.pos).ok_or_else(|| anyhow::anyhow!(ERRO))?;
                self.pos += 1;
                c
            } else {
                c
            };
            // qdtext/quoted-pair: obs-text é opaco; CTL/DEL continuam inválidos.
            ensure!(c == b'\t' || (c >= 0x20 && c != 0x7f), ERRO);
            value.push(c);
        }
        anyhow::bail!(ERRO)
    }
    /// Depois de uma vírgula: auth-param tem '='; caso contrário começa outro challenge.
    fn parametro(&mut self) -> bool {
        let old = self.pos;
        let named = self.nome().is_ok();
        self.ows();
        let yes = named && self.bytes.get(self.pos) == Some(&b'=');
        self.pos = old;
        yes
    }
}

/// Todos os headers/challenges são percorridos; nunca herda scope de outro esquema.
fn desafio(headers: &[&[u8]]) -> Result<Option<Vec<String>>> {
    let mut scopes = None;
    let mut bearers = 0;
    let mut empty = 0;
    // RFC 9110 §§5.2/5.3: linhas do mesmo campo formam uma lista em ordem.
    let header = headers.join(b", ".as_slice());
    let mut p = Parser { bytes: &header, pos: 0 };
    p.ows();
    while p.pos < p.bytes.len() {
        p.vazios(&mut empty)?;
        if p.pos == p.bytes.len() {
            break;
        }
        let bearer = p.nome()?.eq_ignore_ascii_case(b"Bearer");
        if bearer {
            bearers += 1;
        }
        if p.pos == p.bytes.len() {
            break;
        }
        if p.bytes[p.pos] == b',' {
            p.pos += 1;
            p.ows();
            continue;
        }
        ensure!(p.bytes[p.pos] == b' ', ERRO);
        p.ows();
        p.vazios(&mut empty)?;
        if p.pos == p.bytes.len() {
            break;
        }
        // token68 (ex.: Negotiate/Basic) não tem auth-params nem scope.
        let end = p.bytes[p.pos..].iter().position(|c| *c == b',').map_or(p.bytes.len(), |n| p.pos + n);
        let mut credential = &header[p.pos..end];
        while credential.last().is_some_and(|c| matches!(c, b' ' | b'\t')) {
            credential = &credential[..credential.len() - 1];
        }
        let end_stem = credential.iter().rposition(|c| *c != b'=').map_or(0, |n| n + 1);
        let stem = &credential[..end_stem];
        let token68 = !stem.is_empty() && stem.iter().all(|c| c.is_ascii_alphanumeric() || b"-._~+/".contains(c));
        if token68 {
            ensure!(!bearer, ERRO);
            p.pos = end;
            if p.pos < p.bytes.len() {
                p.pos += 1;
                p.ows();
            }
            continue;
        }
        let mut names = BTreeSet::new();
        loop {
            let name = p.nome()?;
            ensure!(names.insert(name.to_ascii_lowercase()), ERRO);
            p.ows();
            ensure!(p.bytes.get(p.pos) == Some(&b'='), ERRO);
            p.pos += 1;
            p.ows();
            let value = p.valor()?;
            if bearer && name.eq_ignore_ascii_case(b"scope") {
                ensure!(scopes.is_none(), ERRO);
                let value = std::str::from_utf8(&value).map_err(|_| anyhow::anyhow!(ERRO))?;
                scopes = Some(conjunto(value)?);
            }
            p.ows();
            if p.pos == p.bytes.len() {
                break;
            }
            ensure!(p.bytes[p.pos] == b',', ERRO);
            p.pos += 1;
            p.vazios(&mut empty)?;
            if p.pos == p.bytes.len() {
                break;
            }
            if !p.parametro() {
                break;
            }
        }
    }
    ensure!(scopes.is_none() || bearers == 1, ERRO);
    Ok(scopes)
}

/// O fallback só recebe PRM validado por M3-C; []/ausente continuam UNKNOWN.
pub(crate) fn selecionar(desafio: Option<Vec<String>>, documento: &Value) -> Result<Vec<String>> {
    if let Some(scopes) = desafio {
        return Ok(scopes);
    }
    let Some(value) = documento.get("scopes_supported") else { return Ok(Vec::new()) };
    let values = value.as_array().ok_or_else(|| anyhow::anyhow!(ERRO))?;
    let mut scopes = Vec::new();
    let mut seen = BTreeSet::new();
    for value in values {
        let value = value.as_str().filter(|s| scope_token(s)).ok_or_else(|| anyhow::anyhow!(ERRO))?;
        if seen.insert(value) {
            scopes.push(value.to_owned());
        }
    }
    Ok(scopes)
}

/// Conta de ficha: pede EXATAMENTE os escopos dos grupos ligados, nunca o desafio do servidor nem a lista inteira do
/// fornecedor. Se o servidor declara os escopos que aceita e algum pedido da ficha não está lá, o login é recusado.
pub(crate) fn da_ficha(pedidos: &[String], documento: &Value) -> Result<Vec<String>> {
    let mut vistos = BTreeSet::new();
    let mut escopos = Vec::new();
    for escopo in pedidos {
        ensure!(scope_token(escopo), ERRO);
        if vistos.insert(escopo.as_str()) {
            escopos.push(escopo.clone());
        }
    }
    if let Some(declarados) = documento.get("scopes_supported") {
        let declarados: BTreeSet<&str> = declarados
            .as_array()
            .ok_or_else(|| anyhow::anyhow!(ERRO))?
            .iter()
            .map(|v| v.as_str().ok_or_else(|| anyhow::anyhow!(ERRO)))
            .collect::<Result<_>>()?;
        ensure!(escopos.iter().all(|e| declarados.contains(e.as_str())), ERRO);
    }
    Ok(escopos)
}

pub(crate) fn conferir_endpoint(url: &reqwest::Url) -> Result<()> {
    ensure!(!url.query_pairs().any(|(key, _)| key == "scope"), ERRO);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desafio(headers: &[&str]) -> Result<Option<Vec<String>>> {
        let valores: Vec<_> = headers.iter().map(|s| s.as_bytes()).collect();
        super::desafio(&valores)
    }

    #[test]
    fn parser_separa_schemes_headers_quoted_e_conjuntos() {
        let mut opaco = Parser { bytes: b"\"caf\xe9\\\xff\"", pos: 0 };
        assert_eq!(opaco.valor().unwrap().as_slice(), b"caf\xe9\xff");
        assert_eq!(super::desafio(&[b"Bearer realm=\"caf\xe9\", scope=read"]).unwrap(), Some(vec!["read".into()]));
        assert_eq!(super::desafio(&[b"Bearer scope=\"r\xe9ad\""]).unwrap_err().to_string(), ERRO);
        for byte in [0, 0x1f, 0x7f] {
            for escaped in [false, true] {
                let mut bytes = vec![b'"'];
                if escaped {
                    bytes.push(b'\\');
                }
                bytes.extend([byte, b'"']);
                let mut parser = Parser { bytes: &bytes, pos: 0 };
                assert_eq!(parser.valor().unwrap_err().to_string(), ERRO);
            }
        }
        for headers in [
            vec![r#"Basic realm="a,b", scope="ignorado", bEaReR realm="x", scope="read write""#],
            vec![r"Basic YWJjZA==", r#"Bearer scope="read write""#],
            vec![r#"Bearer realm="a,\"b", scope="read write read""#],
            vec![r#"Bearer scope="\read write""#],
            vec![r#", ,Bearer , realm="x",, Scope="read write", ,"#],
            vec![r#"Bearer realm="x""#, r#"scope="read write""#],
            vec![r#"Basic realm="x""#, r#"scope="ignorar", Bearer scope="read write""#],
        ] {
            assert_eq!(desafio(&headers).unwrap(), Some(vec!["read".into(), "write".into()]));
        }
        assert_eq!(desafio(&[r#"Bearer Scope="Read read Read""#]).unwrap(), Some(vec!["Read".into(), "read".into()]));
        assert_eq!(desafio(&[&format!("{}Bearer scope=read", ",".repeat(33))]).unwrap_err().to_string(), ERRO);
        for headers in [vec![], vec!["Bearer"], vec![r#"Basic scope="read", Bearer realm="x""#]] {
            assert_eq!(desafio(&headers).unwrap(), None);
        }
        for headers in [
            vec![r#"Bearer scope="""#],
            vec![r#"Bearer scope=" read""#],
            vec![r#"Bearer scope="read  write""#],
            vec!["Bearer scope=\"read\twrite\""],
            vec![r#"Bearer scope="réad""#],
            vec![r#"Bearer scope="read", scope="read""#],
            vec![r#"Bearer scope="read", Scope="write""#],
            vec![r#"Bearer realm="x", Realm="y", scope="read""#],
            vec![r#"Bearer scope="read""#, r#"Bearer scope="write""#],
            vec![r#"Bearer scope="read", Bearer realm="outro""#],
            vec![r#"Bearer scope="read" lixo"#],
            vec![r#"Bearer scope="read"#],
        ] {
            assert_eq!(desafio(&headers).unwrap_err().to_string(), ERRO, "{headers:?}");
        }
    }

    #[test]
    fn selecao_prioriza_desafio_sem_exigir_relacao_e_omite_desconhecido() {
        let metadata = json!({"scopes_supported":["read","write","read"]});
        for scope in ["read", "read write admin", "outro"] {
            assert_eq!(selecionar(Some(conjunto(scope).unwrap()), &metadata).unwrap().join(" "), scope);
        }
        assert_eq!(selecionar(None, &metadata).unwrap(), ["read", "write"]);
        for doc in [json!({}), json!({"scopes_supported":[]})] {
            assert_eq!(selecionar(None, &doc).unwrap(), [] as [String; 0]);
        }
        for value in [Value::Null, json!("read"), json!([1]), json!(["read write"]), json!([""])] {
            assert!(selecionar(None, &json!({"scopes_supported":value})).is_err());
        }
        for query in ["scope=read", "%73cope=x", "scop%65=x", "scope=a&scope=b"] {
            assert!(conferir_endpoint(&reqwest::Url::parse(&format!("https://x/token?{query}")).unwrap()).is_err());
        }
        assert!(conferir_endpoint(&reqwest::Url::parse("https://x/token?tenant=x&next=scope%3Da").unwrap()).is_ok());
    }
}
