use super::Connector;
use crate::{
    fork::conectores::catalogo_servicos::{self, PLANETSCALE_CHAVE},
    store::Store,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::collections::BTreeSet;

pub(super) const KEY: &str = "market.planetscale.preflight.v1";

pub(crate) fn gravar(db: &Connection, contas: &[Connector]) -> Result<()> {
    let ids: BTreeSet<_> = contas
        .iter()
        .filter(|c| c.servico.as_deref() == Some("planetscale") && c.tipo.as_deref() == Some(PLANETSCALE_CHAVE))
        .map(|c| c.id.as_str())
        .collect();
    db.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![KEY, serde_json::to_string(&ids)?],
    )?;
    Ok(())
}

pub(crate) fn nome_recusado(store: &Store, conta: &str, nome: Option<&str>) -> Result<bool> {
    let restrita = if let Some(raw) = store.kv_read(KEY)? {
        serde_json::from_str::<BTreeSet<String>>(&raw)?.contains(conta)
    } else {
        // Antes da primeira leitura do catálogo após upgrade, não abrir o cofre com um nome proibido
        // para um ID que o cadastro anterior poderia ter emitido para o PlanetScale.
        conta == "planetscale"
            || conta.strip_prefix("planetscale-").is_some_and(|s| {
                !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<u64>().is_ok_and(|n| n >= 2)
            })
    };
    if !restrita {
        return Ok(false);
    }
    let tipo = catalogo_servicos::servico("planetscale")
        .and_then(|s| s.tipo(PLANETSCALE_CHAVE))
        .context("Ficha do PlanetScale indisponível")?;
    Ok(nome.is_none_or(|nome| !tipo.grupos.iter().any(|g| g.disponivel && g.ferramentas.contains(&nome))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antes_da_migracao_recusa_o_id_legado_sem_reservar_o_nome_para_outros_conectores() -> Result<()> {
        let store = Store::open(std::path::Path::new(":memory:"))?;
        let nome = Some("fixture_other_tool");
        assert!(nome_recusado(&store, "planetscale", nome)?);
        assert!(nome_recusado(&store, "planetscale-2", nome)?);
        assert!(!nome_recusado(&store, "planetscale-principal", nome)?);
        assert!(!nome_recusado(&store, "planetscale", Some("planetscale_get_insights"))?);
        let conta = serde_json::from_value(serde_json::json!({
            "id":"planetscale","name":"Fixture","url":"https://mcp.example.invalid/mcp"
        }))?;
        crate::market::save_connectors(&store, &[conta])?;
        assert!(!nome_recusado(&store, "planetscale", nome)?);
        Ok(())
    }
}
