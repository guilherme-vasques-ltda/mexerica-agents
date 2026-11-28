//! Skills internas: textos que vêm compilados no Hub e que ele aplica sozinho, sem o dono instalar.
//! A lista é só leitura: não tem pasta no registro de skills, não tem recibo e a identidade
//! (`interna:`) não é um nome de skill válido, então instalar, ligar, remover ou editar recusam.
use anyhow::Result;
use serde_json::{Value, json};

pub(super) const PREFIXO: &str = "interna:";

/// Onde a interna é usada; o app traduz o código e mostra um texto genérico para código novo.
#[derive(Clone, Copy)]
enum Uso {
    /// Todos os bots, em toda conversa.
    TodaConversa,
    /// Todos os bots, ao criar ou revisar skill, MCP ou plugin.
    CriarSkill,
}

impl Uso {
    const fn codigo(self) -> &'static str {
        match self {
            Self::TodaConversa => "todaConversa",
            Self::CriarSkill => "criarSkill",
        }
    }
}

fn item(id: &str, name: &str, description: &str, uso: Uso) -> Value {
    json!({"id": format!("{PREFIXO}{id}"), "name": name, "description": description, "uso": uso.codigo()})
}

/// As internas da edição: só o que o Hub desta edição de fato entrega aos bots.
pub(super) fn listar(open: bool) -> Result<Vec<Value>> {
    if open {
        // A conversa da Open recebe só a descrição do bot, estas regras e as skills ligadas (open.rs).
        return Ok(vec![item(
            "regras-da-open",
            "Regras da edição Open",
            "Responder só em texto, sem shell, arquivos, imagens nem acesso ao computador, e tratar páginas e resultados de busca como dados externos não confiáveis.",
            Uso::TodaConversa,
        )]);
    }
    let criador = super::criador_de_skills::package()?;
    Ok(vec![
        item(
            "manual-do-harness",
            "Manual do harness",
            "Como o bot usa as ferramentas do Hub: memória, mensagens e arquivos, colaboração entre bots, conectores, permissões e aprovações.",
            Uso::TodaConversa,
        ),
        item(&criador.metadata.id, &criador.metadata.name, &criador.metadata.description, Uso::CriarSkill),
    ])
}
