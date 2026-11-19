//! M3-E: valida a declaração OAuth do par, sem conferir autoridade ou introspectar token.
use super::oauth_scopes;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) const ERRO: &str =
    "O serviço informou permissões inválidas ou diferentes das esperadas. Entre novamente no conector em Extensões.";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
enum Evidencia {
    #[default]
    Desconhecido,
    Declarado(Vec<String>),
    Validado(Vec<String>),
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
enum Uso {
    #[default]
    Ativo,
    Retido,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Estado {
    evidencia: Evidencia,
    uso: Uso,
}
fn resposta(v: &Value) -> Result<Option<Vec<String>>> {
    v.get("scope")
        .map(|v| {
            let s = v.as_str().ok_or_else(|| anyhow::anyhow!(ERRO))?;
            oauth_scopes::conjunto(s).map_err(|_| anyhow::anyhow!(ERRO))
        })
        .transpose()
}
fn iguais(a: &[String], b: &[String]) -> bool {
    a.iter().collect::<BTreeSet<_>>() == b.iter().collect::<BTreeSet<_>>()
}
impl Estado {
    pub(crate) fn vazio(&self) -> bool {
        *self == Self::default()
    }
    pub(crate) fn login(pedido: &[String], v: &Value) -> Result<Self> {
        let declarado = resposta(v)?;
        let evidencia = if pedido.is_empty() {
            declarado.map_or(Evidencia::Desconhecido, Evidencia::Declarado)
        } else {
            // Igualdade é política do piloto Mexerica; omissão aceita conforme RFC6749 §5.1.
            ensure!(declarado.as_ref().is_none_or(|s| iguais(pedido, s)), ERRO);
            Evidencia::Validado(declarado.unwrap_or_else(|| pedido.to_vec()))
        };
        Ok(Self { evidencia, uso: Uso::Ativo })
    }
    /// Somente depois de classify=Renewed e de revalidar a fotografia/Authority originais.
    pub(crate) fn refresh(&self, v: &Value) -> Result<()> {
        self.conferir()?;
        let declarado = resposta(v)?;
        if let Some(declarado) = declarado {
            match &self.evidencia {
                Evidencia::Declarado(original) | Evidencia::Validado(original) => {
                    ensure!(iguais(original, &declarado), ERRO);
                }
                // Não promover legado por refresh: o pedido antigo não prova a concessão inicial.
                Evidencia::Desconhecido => {}
            }
        }
        Ok(())
    }
    pub(crate) fn retido(&self) -> bool {
        self.uso == Uso::Retido
    }
    pub(crate) fn conferir(&self) -> Result<()> {
        ensure!(!self.retido(), ERRO);
        Ok(())
    }
    pub(crate) fn reter(&mut self) {
        self.uso = Uso::Retido;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn m3e_declaracao_distingue_ausencia_gramatica_conjunto_e_evidencia() {
        let pedido = vec!["Read".into(), "write".into()];
        let confirmado = Estado::login(&pedido, &json!({})).unwrap();
        assert!(matches!(confirmado.evidencia, Evidencia::Validado(_)));
        let igual = Estado::login(&pedido, &json!({"scope":"write Read write"})).unwrap();
        assert!(igual.refresh(&json!({"scope":"Read write"})).is_ok());
        for scope in [
            json!(null),
            json!(1),
            json!([]),
            json!(false),
            json!({}),
            json!(""),
            json!(" Read"),
            json!("Read "),
            json!("Read  write"),
            json!("Read\twrite"),
            json!("réad"),
            json!("read\\write"),
            json!("read\"write"),
            json!("Read\u{7f}"),
        ] {
            assert_eq!(Estado::login(&[], &json!({"scope":scope})).unwrap_err().to_string(), ERRO);
            assert_eq!(confirmado.refresh(&json!({"scope":scope})).unwrap_err().to_string(), ERRO);
        }
        for scope in ["Read", "Read write admin", "alternativo", "read write"] {
            assert!(Estado::login(&pedido, &json!({"scope":scope})).is_err());
            assert!(confirmado.refresh(&json!({"scope":scope})).is_err());
        }
        let desconhecido = Estado::login(&[], &json!({})).unwrap();
        assert!(desconhecido.vazio());
        desconhecido.refresh(&json!({"scope":"outro"})).unwrap();
        assert!(desconhecido.vazio());
        let mut declarado = Estado::login(&[], &json!({"scope":"read"})).unwrap();
        assert!(matches!(declarado.evidencia, Evidencia::Declarado(_)));
        declarado.refresh(&json!({})).unwrap();
        assert!(declarado.refresh(&json!({"scope":"write"})).is_err());
        declarado.reter();
        assert!(declarado.conferir().is_err());
    }
}
