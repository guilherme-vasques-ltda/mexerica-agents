use super::{
    open_command_settings,
    open_tool_actions::{approve_command, check_card},
    open_tools::LOCAL_COMMAND,
    open_turn::LocalTurn,
    rules::{
        avaliacao::{Decisao, Origem, Validade},
        rule::ApprovalLevel,
        store,
        terminal::{
            self, Action,
            executor::{Consent, Gate},
        },
    },
    sandbox::contrato::Fim,
};
use crate::{LockExt, hub::Hub};
use anyhow::{Context, Result, ensure};
use rusqlite::TransactionBehavior;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use zeroize::Zeroizing;

pub(super) fn catalog() -> Value {
    json!({
        "type":"function", "name":LOCAL_COMMAND,
        "description":"Pede autorização do dono no app para executar um comando local sem isolamento. Use caminhos absolutos e argumentos separados, sem credenciais. Só retorna estado e código de saída. Se o resultado for desconhecido, não repita a chamada.",
        "inputSchema":{
            "type":"object", "additionalProperties":false, "required":["argv","cwd"],
            "properties":{
                "argv":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"string"}},
                "cwd":{"type":"string"}
            }
        }
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    argv: Vec<String>,
    cwd: String,
}

pub(super) async fn call(hub: &Arc<Hub>, proof: &LocalTurn, args: &Value) -> Result<Value> {
    let cfg = proof.check(hub)?;
    let activation =
        open_command_settings::revision(&hub.store.db.locked(), &cfg.id)?.context("open_tools_command_disabled")?;
    let Arguments { argv, cwd } = serde_json::from_value(args.clone())?;
    ensure!(
        argv.first().is_some_and(|program| std::path::Path::new(program).is_absolute())
            && std::path::Path::new(&cwd).is_absolute()
            && argv.len() <= 64
            && !cwd.chars().any(char::is_control),
        "open_tools_invalid_arguments"
    );
    let snapshot = hub.store.donna_rules()?;
    let decision = terminal::decide(
        &cfg.id,
        Action::Opaque,
        &argv[0],
        Origem::Derivada,
        Validade::Atual,
        ApprovalLevel::Ask,
        &snapshot.rules,
    );
    ensure!(decision == Decisao::Perguntar, "open_tools_command_denied");
    let mut detail = String::new();
    for c in serde_json::to_string_pretty(&json!({"argv":argv,"cwd":cwd}))?.chars() {
        if c.is_ascii() {
            detail.push(c);
        } else {
            detail.extend(c.escape_unicode());
        }
    }
    ensure!(detail.encode_utf16().count() <= 2000, "open_tools_invalid_approval");
    let consent = approve_command(
        hub, proof.lane(), proof.turn(), activation, "Pedir aprovação para comando local",
        "Se você aprovar o próximo cartão, o comando vai rodar com as suas permissões, sem isolamento. Ele pode ler ou modificar seus arquivos e usar a rede. Este cartão só autoriza mostrar o pedido; não roda nada.",
    ).await?;
    proof.check(hub)?;
    for login in crate::market::logins::all(&hub.store)? {
        let secret = Zeroizing::new(login.password);
        ensure!(
            secret.is_empty()
                || (!cwd.contains(secret.as_str())
                    && !argv.iter().any(|part| part.contains(secret.as_str()))
                    && !detail.contains(secret.as_str())),
            "open_tools_command_credentials"
        );
    }
    let gate = Gate::opaque(cfg.id, proof.turn(), decision, Consent::OwnerAcceptedExecutionWithoutIsolation)?;
    let prepared = gate.prepare(argv, PathBuf::from(cwd))?;
    let card =
        approve_command(hub, proof.lane(), proof.turn(), activation, "Executar este comando uma vez", &detail).await?;
    gate.approve(prepared.request())?;
    let mut started = false;
    let result = prepared
        .execute_guarded(|spawn| {
            let _reservation = proof.reserve(hub)?;
            let mut db = hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            proof.check_db(&tx)?;
            check_card(&tx, &consent)?;
            check_card(&tx, &card)?;
            ensure!(store::revision(&tx)? == snapshot.revision, "open_tools_command_policy_changed");
            open_command_settings::require(&tx, &proof.lane().chat, activation)?;
            let child = spawn()?;
            started = true;
            Ok(child)
        })
        .await;
    proof.check(hub)?;
    let fim = match result {
        Ok(result) => result.fim,
        Err(_) if started => return Ok(json!({"status":"unknown"})),
        Err(error) => return Err(error),
    };
    Ok(match fim {
        Fim::Saiu(code) => json!({"status":"exited","exitCode":code}),
        Fim::Sinal(signal) => json!({"status":"signaled","signal":signal}),
        Fim::TempoEsgotado => json!({"status":"timed_out"}),
    })
}
