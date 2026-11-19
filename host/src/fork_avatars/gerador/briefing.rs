//! Entrada do gerador: só nome, descrição e atalhos do catálogo `variantes.json`.
//! Os atalhos viram dicas descritivas de aparência; nada aqui vira instrução de agente.
use super::{VARIANTES, falha};
use anyhow::Result;
use serde_json::{Value, json};

pub struct Briefing {
    pub nome: String,
    pub descricao: String,
    pub atalhos: serde_json::Map<String, Value>,
}

impl Briefing {
    /// O JSON descritivo que compõe o prompt da identidade (dados, não ferramentas).
    pub fn json(&self) -> String {
        json!({"nome": self.nome, "descricao": self.descricao, "atalhos": self.atalhos}).to_string()
    }
}

/// Mesmas fronteiras do protótipo: nome até 80, descrição até 2000, atalhos só do catálogo.
pub fn validar(body: &Value) -> Result<Briefing> {
    let Some(obj) = body.as_object() else { return Err(falha("BRIEFING_INVALIDO")) };
    if obj.keys().any(|k| !["nome", "descricao", "atalhos"].contains(&k.as_str())) {
        return Err(falha("BRIEFING_INVALIDO"));
    }
    let nome = body["nome"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.chars().count() <= 80);
    let descricao = body["descricao"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.chars().count() <= 2000);
    let (Some(nome), Some(descricao)) = (nome, descricao) else {
        return Err(falha("BRIEFING_INVALIDO"));
    };
    let mut atalhos = serde_json::Map::new();
    if let Some(mapa) = body.get("atalhos") {
        let Some(entradas) = mapa.as_object() else { return Err(falha("ATALHO_INVALIDO")) };
        for (campo, valor) in entradas {
            let dica =
                valor.as_str().and_then(|id| VARIANTES[campo][id].as_str()).filter(|_| VARIANTES[campo].is_object());
            let Some(dica) = dica else { return Err(falha("ATALHO_INVALIDO")) };
            atalhos.insert(campo.clone(), Value::String(dica.to_owned()));
        }
    }
    Ok(Briefing { nome: nome.to_owned(), descricao: descricao.to_owned(), atalhos })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fronteiras_do_briefing() {
        assert!(validar(&json!({"nome":"Limoeiro","descricao":"cítrico"})).is_ok());
        assert!(validar(&json!({"nome":" x ","descricao":"d","extra":1})).is_err());
        assert!(validar(&json!({"nome":"","descricao":"d"})).is_err());
        assert!(validar(&json!({"nome":"n","descricao":"d","atalhos":{"cor":"laranja"}})).is_ok());
        assert!(validar(&json!({"nome":"n","descricao":"d","atalhos":{"cor":"inventada"}})).is_err());
        assert!(validar(&json!({"nome":"n","descricao":"d","atalhos":{"campo":"laranja"}})).is_err());
        assert!(validar(&json!({"nome":"n".repeat(81),"descricao":"d"})).is_err());
        assert!(validar(&json!({"nome":"n","descricao":"d".repeat(2001)})).is_err());
        let b = validar(&json!({"nome":"n","descricao":"d","atalhos":{"cor":"laranja"}})).unwrap();
        assert!(b.json().contains("casca laranja natural"));
    }
}
