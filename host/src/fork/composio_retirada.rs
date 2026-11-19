//! Retirada da integração dedicada do Composio (decisão do dono, 16/11/2026): migração única que apaga só o
//! registro do Composio do cofre (chave, configuração, cache de contas e apelidos moram nele), tira os
//! conectores de conta `composio-*` das listas dos bots e avisa o dono. O resto do cofre não é tocado e
//! nenhum valor secreto é lido, decifrado ou escrito em log.
use crate::{
    LockExt,
    store::{EntryKind, Lane, Store},
};
use anyhow::{Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use serde_json::{Value, json};

/// Registro do cofre usado pela integração antiga.
const SLOT: &str = "composio";
/// Prefixo dos ids de conta (`composio-<app>-<conta>`) que a integração antiga criava nas listas dos bots.
const PREFIX: &str = "composio-";
/// Id de conta da integração antiga: `composio-<app>-ca_<conta>`. Um MCP comum chamado `composio-mcp` não casa e
/// continua ligado. Só olha o texto do id: não abre o cofre, que pode estar trancado na partida.
fn is_account(id: &str) -> bool {
    id.strip_prefix(PREFIX)
        .and_then(|rest| rest.split_once("-ca_"))
        .is_some_and(|(app, account)| !app.is_empty() && !account.is_empty())
}

/// Marca de que a migração já rodou; impede apagar fichas novas do catálogo que usem o mesmo prefixo.
const DONE: &str = "migration.composio-retirado.v1";
const NOTICE_ID: &str = "composio-retirado-v1";
const NOTICE: &str = "O Composio agora é um conector comum; conecte de novo pelo catálogo se quiser.";

/// Roda uma vez por instalação. Segunda chamada não faz nada, mesmo que o estado mude depois.
///
/// Tudo acontece numa única transação: apagar o registro do cofre, limpar as listas dos bots, gravar o aviso e a
/// marca `DONE`. Se algo falhar ou o processo cair no meio, nada disso fica gravado e a próxima partida repete a
/// migração inteira, inclusive o aviso. A escrita nos bots segue o caminho do dono (`como_dono`), a única forma
/// de os gatilhos de proteção do bot principal aceitarem a mudança de `connectors`; os gatilhos continuam ligados.
pub(crate) fn run(store: &Store) -> Result<()> {
    apply(store, || Ok(()))
}

/// `before_notice` roda depois da limpeza e antes do aviso; os testes usam para simular uma queda nessa fronteira.
fn apply(store: &Store, before_notice: impl FnOnce() -> Result<()>) -> Result<()> {
    let _guard = store.connector_lock.locked();
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if tx.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [DONE], |r| r.get::<_, bool>(0))? {
        return Ok(());
    }
    // Só o registro do Composio sai do cofre; as demais credenciais não são tocadas nem abertas.
    let had_slot =
        tx.execute("DELETE FROM kv WHERE k=?1", [SLOT]).context("não foi possível apagar o registro do Composio")? > 0;
    let cleaned = crate::fork::donna_fixa::como_dono(&tx, clean_bots)?;
    before_notice()?;
    if had_slot || !cleaned.is_empty() {
        let target = notice_target(&tx, &cleaned)?;
        let exists = crate::store::entry_on(&tx, NOTICE_ID)?.is_some();
        if let (Some(bot), false) = (target, exists) {
            let data = json!({"text": NOTICE, "style": "info"});
            crate::store::insert_visible_on(&tx, NOTICE_ID, &Lane::main(&bot), EntryKind::Notice, 0, &data)?;
        }
    }
    tx.execute("INSERT INTO kv(k,v) VALUES(?1,'1')", [DONE])?;
    tx.commit()?;
    drop(db);
    if let Err(error) = store.guard_cleanup() {
        tracing::warn!(%error, "não foi possível limpar o estado do guarda depois da migração do Composio");
    }
    Ok(())
}

/// Tira os ids de conta da integração antiga de cada bot vivo, mexendo só em `$.connectors`: o resto do perfil
/// (inclusive campos legados) fica como está. Perfil ilegível ou sem lista de textos é deixado de lado.
fn clean_bots(c: &Connection) -> Result<Vec<String>> {
    let rows: Vec<(String, String)> = {
        let mut st = c.prepare("SELECT id, config FROM bots WHERE deleted=0 ORDER BY id")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let mut cleaned = Vec::new();
    for (id, raw) in rows {
        let Ok(profile) = serde_json::from_str::<Value>(&raw) else { continue };
        let Some(list) = profile.get("connectors").and_then(Value::as_array) else { continue };
        let kept: Vec<&Value> = list.iter().filter(|v| !v.as_str().is_some_and(is_account)).collect();
        if kept.len() == list.len() {
            continue;
        }
        c.execute(
            "UPDATE bots SET config=json_set(config,'$.connectors',json(?1)), rev=?2 WHERE id=?3",
            rusqlite::params![serde_json::to_string(&kept)?, crate::store::next_rev(c)?, id],
        )?;
        cleaned.push(id);
    }
    Ok(cleaned)
}

/// Quem recebe o aviso: o primeiro bot limpo que não é grupo; sem nenhum, o primeiro bot vivo que não é grupo.
fn notice_target(c: &Connection, cleaned: &[String]) -> Result<Option<String>> {
    let mut candidates: Vec<(String, bool)> = Vec::new();
    for id in cleaned {
        if let Some(row) = Store::bot_on(c, id)? {
            candidates.push((id.clone(), row.config.is_group()));
        }
    }
    if let Some((id, _)) = candidates.iter().find(|(_, group)| !group).or(candidates.first()) {
        return Ok(Some(id.clone()));
    }
    let ids: Vec<String> = {
        let mut st = c.prepare("SELECT id FROM bots WHERE deleted=0 ORDER BY id")?;
        st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for id in ids {
        if Store::bot_on(c, &id)?.is_some_and(|row| !row.config.is_group()) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::identity::Identity;
    use crate::store::BotConfig;
    use std::path::Path;

    fn store() -> Store {
        let store = Store::open(Path::new(":memory:")).unwrap();
        *store.secret_key.lock().unwrap() = Some(zeroize::Zeroizing::new(vec![42; 32]));
        store
    }

    fn bot(store: &Store, id: &str, connectors: &[&str]) {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":id,"name":id,"backend":"custom","cwd":"/","connectors":connectors}))
                .unwrap();
        store.save_bot(&cfg).unwrap();
    }

    fn connectors_of(store: &Store, id: &str) -> Vec<String> {
        store.bot(id).unwrap().unwrap().config.connectors
    }

    fn notices(store: &Store, bot: &str) -> usize {
        store.entries_since(0, 100).unwrap().iter().filter(|e| e.bot_id == bot && e.data["text"] == NOTICE).count()
    }

    fn raw_bot(store: &Store, id: &str, config: &str) {
        store
            .db
            .locked()
            .execute("INSERT INTO bots(id, rev, config, avatar_config) VALUES(?1, 1, ?2, NULL)", [id, config])
            .unwrap();
    }

    fn make_primary(store: &Store, id: &str) {
        store.kv_set("team.settings.v1", &json!({"primaryBotId": id}).to_string()).unwrap();
    }

    #[test]
    fn apaga_so_o_composio_limpa_os_bots_avisa_uma_vez_e_nao_repete() {
        let store = store();
        let outro =
            serde_json::from_value(json!({"id":"exemplo-mcp","name":"Exemplo","url":"https://example.test/mcp"}))
                .unwrap();
        crate::market::save_connectors(&store, &[outro]).unwrap();
        crate::market::vault::write(&store, SLOT, r#"{"key":"segredo-composio","connections":[]}"#).unwrap();
        crate::market::vault::write(&store, "outra-credencial", "valor-que-fica").unwrap();
        bot(&store, "b1", &["exemplo-mcp", "composio-gmail-ca_x", "ferramenta-mail", "composio-slack-ca_y"]);
        bot(&store, "b2", &["exemplo-mcp"]);

        run(&store).unwrap();

        assert_eq!(store.kv_read(SLOT).unwrap(), None, "só o registro do Composio some");
        assert_eq!(crate::market::vault::read(&store, "outra-credencial").unwrap().as_deref(), Some("valor-que-fica"));
        assert_eq!(crate::market::connectors(&store).unwrap().len(), 1, "conector comum intacto");
        assert_eq!(connectors_of(&store, "b1"), ["exemplo-mcp", "ferramenta-mail"]);
        assert_eq!(connectors_of(&store, "b2"), ["exemplo-mcp"]);
        assert_eq!(notices(&store, "b1"), 1);
        assert_eq!(notices(&store, "b2"), 0);

        // Segunda execução: nada muda, mesmo com ficha nova usando o mesmo prefixo.
        crate::market::vault::write(&store, SLOT, "{}").unwrap();
        bot(&store, "b1", &["composio-gmail-ca_novo"]);
        run(&store).unwrap();
        assert!(store.kv_read(SLOT).unwrap().is_some());
        assert_eq!(connectors_of(&store, "b1"), ["composio-gmail-ca_novo"]);
        assert_eq!(notices(&store, "b1"), 1);
    }

    #[test]
    fn bot_principal_vivo_com_os_gatilhos_ativos_e_limpo_pelo_caminho_do_dono() {
        let store = store();
        bot(&store, "principal", &["composio-gmail-ca_x", "exemplo-mcp"]);
        bot(&store, "outro", &["composio-slack-ca_y"]);
        make_primary(&store, "principal");
        crate::market::vault::write(&store, SLOT, "{}").unwrap();
        // Prova de que a proteção normal está ligada: o caminho comum não muda o principal.
        let mut cfg = store.bot("principal").unwrap().unwrap().config;
        cfg.connectors.clear();
        assert!(store.save_bot(&cfg).is_err(), "gatilho do principal precisa recusar a escrita sem a marca do dono");

        run(&store).unwrap();

        assert_eq!(connectors_of(&store, "principal"), ["exemplo-mcp"]);
        assert_eq!(connectors_of(&store, "outro"), Vec::<String>::new());
        assert_eq!(store.kv_read(SLOT).unwrap(), None);
        assert!(store.kv_read(DONE).unwrap().is_some());
        assert_eq!(notices(&store, "outro") + notices(&store, "principal"), 1);
        // A proteção continua valendo depois da migração: a marca do dono não ficou para trás.
        cfg.connectors = vec!["intruso".into()];
        assert!(store.save_bot(&cfg).is_err());
    }

    #[tokio::test]
    async fn hub_inicia_com_o_principal_ligado_a_conta_antiga() {
        let dir = std::env::temp_dir().join(format!("mexerica-composio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let store = store();
        bot(&store, "principal", &["composio-gmail-ca_x"]);
        make_primary(&store, "principal");
        let hub =
            crate::hub::Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
        hub.start().unwrap();
        assert_eq!(connectors_of(&hub.store, "principal"), Vec::<String>::new());
        assert_eq!(notices(&hub.store, "principal"), 1);
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn perfil_quebrado_ou_legado_nao_interrompe_o_lote_e_o_resto_do_perfil_fica_intacto() {
        let store = store();
        bot(&store, "primeiro", &["composio-gmail-ca_x"]);
        // Perfil que o Hub não consegue ler: fica de lado, sem erro nem mudança.
        raw_bot(&store, "ilegivel", "isto não é json");
        // Perfil legado: campo antigo desconhecido e pasta própria são preservados byte a byte.
        let legado = r#"{"id":"legado","name":"Legado","cwd":"/pasta-legada","campoAntigo":{"x":1},"connectors":["composio-notion-ca_z","mcp"]}"#;
        raw_bot(&store, "legado", legado);
        // Perfil sem lista de conectores (formato muito antigo).
        let sem_lista = r#"{"id":"sem-lista","name":"Sem lista","cwd":"/velha"}"#;
        raw_bot(&store, "sem-lista", sem_lista);
        bot(&store, "ultimo", &["composio-slack-ca_y"]);

        run(&store).unwrap();

        assert_eq!(connectors_of(&store, "primeiro"), Vec::<String>::new());
        assert_eq!(connectors_of(&store, "ultimo"), Vec::<String>::new());
        let db = store.db.locked();
        let config = |id: &str| -> Value {
            let raw: String = db.query_row("SELECT config FROM bots WHERE id=?1", [id], |r| r.get(0)).unwrap();
            serde_json::from_str(&raw).unwrap_or(Value::String(raw))
        };
        assert_eq!(config("ilegivel"), json!("isto não é json"));
        assert_eq!(config("sem-lista"), serde_json::from_str::<Value>(sem_lista).unwrap());
        let legado = config("legado");
        assert_eq!(legado["connectors"], json!(["mcp"]));
        assert_eq!(legado["cwd"], "/pasta-legada");
        assert_eq!(legado["campoAntigo"], json!({"x":1}));
    }

    #[test]
    fn queda_entre_a_limpeza_e_o_aviso_desfaz_tudo_e_a_proxima_partida_repete_com_o_aviso() {
        let store = store();
        bot(&store, "b1", &["composio-gmail-ca_x"]);
        crate::market::vault::write(&store, SLOT, "{}").unwrap();

        let falha = apply(&store, || anyhow::bail!("queda simulada"));
        assert!(falha.is_err());
        // Nada ficou pela metade: registro, lista, marca e aviso continuam como antes.
        assert!(store.kv_read(SLOT).unwrap().is_some());
        assert_eq!(connectors_of(&store, "b1"), ["composio-gmail-ca_x"]);
        assert_eq!(store.kv_read(DONE).unwrap(), None);
        assert_eq!(notices(&store, "b1"), 0);

        run(&store).unwrap();
        assert_eq!(store.kv_read(SLOT).unwrap(), None);
        assert_eq!(connectors_of(&store, "b1"), Vec::<String>::new());
        assert_eq!(notices(&store, "b1"), 1);
        assert!(store.kv_read(DONE).unwrap().is_some());
    }

    #[test]
    fn conector_comum_com_prefixo_composio_continua_ligado() {
        let store = store();
        let comum =
            serde_json::from_value(json!({"id":"composio-mcp","name":"Composio MCP","url":"https://example.test/mcp"}))
                .unwrap();
        crate::market::save_connectors(&store, &[comum]).unwrap();
        bot(&store, "b1", &["composio-mcp", "composio-gmail-ca_x"]);
        run(&store).unwrap();
        assert_eq!(connectors_of(&store, "b1"), ["composio-mcp"]);
    }

    #[test]
    fn instalacao_sem_composio_nao_ganha_aviso() {
        let store = store();
        bot(&store, "b1", &["exemplo-mcp"]);
        run(&store).unwrap();
        assert_eq!(notices(&store, "b1"), 0);
        assert!(store.kv_read(DONE).unwrap().is_some());
    }
}
