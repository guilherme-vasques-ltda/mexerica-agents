//! D320: o bot lê só a própria cadeia de Times, um nível acima e um abaixo, no próprio Space.
//! Mesma leitura e projeção do quadro (`store::snapshot`), em Tx descartada; nada é gravado.
//! Não expõe outras áreas, rotas, avô/neto, configuração, conta, memória, pasta ou sessão.
use super::model::{ReportsTo, Role};
use crate::LockExt;
use crate::store::Store;
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};

pub(crate) const FERRAMENTA: &str = "my_team_chain";

const DONO: &str = "Você (o dono)";

pub(crate) const NOTA: &str = "Só leitura do quadro de Equipes. Reporta a não dá poder nem permissão; mandar mensagem a outro bot continua pelas regras de ask_bot/message_bot e do limite D220.";

pub(crate) fn tool() -> Value {
    json!({
        "name": FERRAMENTA,
        "description": "Mostra, só leitura, a sua própria cadeia no quadro de Equipes deste Space: a quem você reporta (o dono, um bot ou um bot oculto), os bots visíveis que reportam direto a você (ocultos só contam em hiddenDirectReports), sua função (manager/executor) e o nome da sua área. Um nível acima e um abaixo; não mostra outras áreas, rotas nem o quadro inteiro. Reporta a não concede poder: pedidos a outros bots seguem as regras de ask_bot/message_bot e do limite D220. Sem argumentos.",
        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
        "annotations": {"readOnlyHint": true, "idempotentHint": true},
    })
}

/// `bot_id` já foi conferido como visível pelo chamador; `space` é o Space do próprio bot.
pub(crate) fn consultar(store: &Store, bot_id: &str, space: &str, args: &Value) -> Result<Value> {
    ensure!(
        args.is_null() || args.as_object().is_some_and(serde_json::Map::is_empty),
        "invalid_field: {FERRAMENTA} não recebe argumentos."
    );
    let snapshot = {
        let c = store.db.locked();
        // Tx só de leitura: o drop sem commit desfaz qualquer efeito.
        let tx = c.unchecked_transaction()?;
        super::store::snapshot(&tx, space)?
    };
    let visible = |id: &str| snapshot.bots.iter().find(|b| b.id == id && !b.hidden);
    let me = snapshot
        .graph
        .members
        .iter()
        .find(|m| m.bot_id == bot_id)
        .ok_or_else(|| anyhow!("bot_not_found: este bot não está no quadro deste Space."))?;
    let reports_to = match &me.reports_to {
        ReportsTo::Owner {} => json!({"kind": "owner", "label": DONO}),
        ReportsTo::Bot { bot_id } => match visible(bot_id) {
            Some(b) => json!({"kind": "bot", "id": b.id, "name": b.name}),
            None => json!({"kind": "hidden"}),
        },
    };
    let mut direct = Vec::new();
    let mut hidden = 0;
    for m in &snapshot.graph.members {
        if !matches!(&m.reports_to, ReportsTo::Bot { bot_id: up } if up == bot_id) {
            continue;
        }
        match visible(&m.bot_id) {
            Some(b) => direct.push(json!({"id": b.id, "name": b.name})),
            None => hidden += 1,
        }
    }
    let team =
        me.team_id.as_deref().and_then(|id| snapshot.graph.nodes.iter().find(|n| n.id == id)).map(|n| n.name.clone());
    Ok(json!({
        "role": match me.role { Role::Manager => "manager", Role::Executor => "executor" },
        "team": team,
        "reportsTo": reports_to,
        "directReports": direct,
        "hiddenDirectReports": hidden,
        "note": NOTA,
    }))
}
