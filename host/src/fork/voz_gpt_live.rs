pub(crate) const CATALOGO_HABILITADO: bool = false;

fn modelo_suportado(id: &str) -> bool {
    matches!(id, "gpt-live-1" | "gpt-live-1.1")
}

pub(crate) fn familia_live(id: &str) -> bool {
    id == "gpt-live" || id.starts_with("gpt-live-")
}

pub(crate) fn catalogo<'a>(habilitado: bool, ids: impl Iterator<Item = &'a str>) -> Option<Vec<&'a str>> {
    if !habilitado {
        return None;
    }
    let mut modelos: Vec<_> = ids.filter(|id| modelo_suportado(id)).collect();
    modelos.sort_unstable();
    modelos.dedup();
    Some(modelos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_catalog_does_not_expose_live_models() {
        assert_eq!(catalogo(CATALOGO_HABILITADO, ["gpt-live-1.1"].into_iter()), None);
    }

    #[test]
    fn enabled_catalog_only_lists_exact_live_models_from_the_provider() {
        let ids = [
            "gpt-live-1.1",
            "gpt-live-1",
            "gpt-live-1.1",
            "gpt-live-1.1-preview",
            "gpt-realtime-2.1",
            "other-gpt-live-1",
        ];
        assert_eq!(catalogo(true, ids.into_iter()), Some(vec!["gpt-live-1", "gpt-live-1.1"]));
        assert_eq!(catalogo(true, ["gpt-realtime-2.1"].into_iter()), Some(vec![]));
    }
}
