//! Fichas do Marketplace: quase todas em arquivo JSON em `fichas/` (lidas na compilação, `catalogo_json::FICHAS`); as do
//! Google moram em `catalogo_google.rs` (MEX-1447). Dado puro, sem segredo; cada ficha cita as fontes
//! oficiais lidas e a data da conferência (`conferida_em`), que o teste do catálogo reprova depois de 90 dias. Ficha
//! JSON inválida fica de fora e o motivo vai para o log (fail-closed).
use super::catalogo_json::{FICHA_DONO_A, FICHAS, ler};
use super::catalogo_servicos::{Links, Servico};

/// O catálogo inteiro: as fichas JSON na ordem de `FICHAS` (a do dono A vai primeiro) e depois as do Google.
pub(super) fn servicos() -> &'static [Servico] {
    static TODOS: std::sync::LazyLock<Vec<Servico>> = std::sync::LazyLock::new(|| {
        let mut todos = Vec::new();
        for json in FICHAS {
            match ler(json) {
                Ok(mut ficha) => {
                    // O nome do produto do dono só pode aparecer no JSON dele: a chave da logo é a do arquivo, não o id.
                    if *json == FICHA_DONO_A {
                        ficha.marca = "dono-a";
                    }
                    todos.push(ficha);
                }
                Err(error) => tracing::error!(%error, "ficha inválida; serviço fora do catálogo"),
            }
        }
        // O Google direto (MEX-1447) mora em `catalogo_google`: o app OAuth é da própria pessoa.
        todos.extend(super::catalogo_google::FICHAS.iter().map(|s| Servico { links: Links { ..s.links }, ..*s }));
        todos
    });
    &TODOS
}
