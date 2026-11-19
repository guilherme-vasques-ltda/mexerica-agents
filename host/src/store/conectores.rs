//! Ledger específico de efeitos de chamadas curadas de Conector. Sem FK/cascade de chat, TTL Guard ou reexecução.
use super::{Entry, EntryKind, Store, entries, next_rev, now_ms};
use crate::{
    LockExt,
    fork::conectores::operacao::{Estado, Tentativa},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_conector_tentativas(
        id TEXT PRIMARY KEY, ingress TEXT NOT NULL UNIQUE, instance TEXT NOT NULL, bot TEXT NOT NULL,
        channel TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('aguardando','autorizada','em_envio',
        'recusada','expirada','cancelada','concluida','falha_comprovada','incerta')),
        deadline INTEGER NOT NULL, rev INTEGER NOT NULL CHECK(rev>0), data TEXT NOT NULL CHECK(json_valid(data)),
        CHECK(json_extract(data,'$.id')=id), CHECK(json_extract(data,'$.rev')=rev),
        CHECK(json_extract(data,'$.state')=state), CHECK(length(json_extract(data,'$.requestDigest'))=64),
        CHECK(json_type(data,'$.source.generation') IS 'text'
            AND length(json_extract(data,'$.source.generation'))=36
            AND substr(json_extract(data,'$.source.generation'),9,1)='-'
            AND substr(json_extract(data,'$.source.generation'),14,1)='-'
            AND substr(json_extract(data,'$.source.generation'),19,1)='-'
            AND substr(json_extract(data,'$.source.generation'),24,1)='-'
            AND length(replace(json_extract(data,'$.source.generation'),'-',''))=32
            AND replace(json_extract(data,'$.source.generation'),'-','') NOT GLOB '*[^0-9a-fA-F]*'),
        CHECK(json_type(data,'$.source.configRevision') IS 'integer'
            AND json_extract(data,'$.source.configRevision')>=0),
        CHECK(json_type(data,'$.source.curationRevision') IS 'integer'
            AND json_extract(data,'$.source.curationRevision')>0),
        CHECK(json_type(data,'$.source.accessRevision') IS 'integer'
            AND json_extract(data,'$.source.accessRevision')>0));
        CREATE INDEX IF NOT EXISTS donna_conector_deadline ON donna_conector_tentativas(state,deadline);
        CREATE INDEX IF NOT EXISTS donna_conector_instance ON donna_conector_tentativas(instance,bot);
        CREATE UNIQUE INDEX IF NOT EXISTS donna_conector_pending_channel ON donna_conector_tentativas(channel)
            WHERE state IN ('aguardando','autorizada','em_envio');
        CREATE UNIQUE INDEX IF NOT EXISTS donna_conector_pending_instance ON donna_conector_tentativas(bot,instance)
            WHERE state IN ('aguardando','autorizada','em_envio');
        CREATE TABLE IF NOT EXISTS donna_conector_cartoes(
        card_id TEXT PRIMARY KEY, tentativa TEXT NOT NULL, snapshot TEXT NOT NULL);",
    )?;
    Ok(())
}

pub(crate) fn read(c: &Connection, id: &str) -> Result<Option<Tentativa>> {
    let raw: Option<String> =
        c.query_row("SELECT data FROM donna_conector_tentativas WHERE id=?1", [id], |r| r.get(0)).optional()?;
    raw.map(|s| serde_json::from_str(&s).map_err(Into::into)).transpose()
}
pub(crate) fn ingress(c: &Connection, ingress: &str) -> Result<Option<Tentativa>> {
    let id: Option<String> =
        c.query_row("SELECT id FROM donna_conector_tentativas WHERE ingress=?1", [ingress], |r| r.get(0)).optional()?;
    id.map(|id| read(c, &id)?.ok_or_else(|| anyhow!("Ledger incompleto"))).transpose()
}

/// O selo já está pronto. O callback só usa a Connection recebida.
pub(crate) fn prepare(
    c: &Connection,
    mut op: Tentativa,
    sealed: &str,
    guard: impl FnOnce(&Connection) -> Result<()>,
) -> Result<(Tentativa, Vec<Entry>)> {
    let tx = c.unchecked_transaction()?;
    guard(&tx)?;
    if let Some(previous) = ingress(&tx, &op.ingress)? {
        ensure!(
            previous.original_digest == op.original_digest
                && previous.context == op.context
                && previous.source.same_grant(&op.source),
            "Ingresso repetido com corpo ou contexto conflitante"
        );
        return Ok((previous, vec![]));
    }
    let count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM donna_conector_tentativas WHERE state IN ('aguardando','autorizada','em_envio')",
        [],
        |r| r.get(0),
    )?;
    ensure!(count < 16, "Limite de 16 pedidos pendentes no Hub");
    let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM donna_conector_tentativas WHERE state IN ('aguardando','autorizada','em_envio') AND (channel=?1 OR (instance=?2 AND bot=?3)))",
        params![op.context.channel,op.source.instance,op.context.bot], |r| r.get(0))?;
    ensure!(!collision, "Já existe pedido pendente neste canal ou bot/instância");
    ensure!(sealed.starts_with("vault:v1:"), "Dados privados devem estar cifrados");
    crate::market::vault::write_sealed(&tx, &op.slot(), sealed)?;
    let mut emitted = Vec::new();
    if let Some(marker) = entries::insert_marker(&tx, &op.context.lane, op.context.turn)? {
        emitted.push(marker);
    }
    let entry = entries::insert_row(
        &tx,
        &op.card_id,
        &op.context.lane,
        EntryKind::Permission.as_str(),
        op.context.turn,
        &op.public(),
    )?;
    op.rev = entry.rev;
    emitted.push(entry);
    tx.execute(
        "INSERT INTO donna_conector_cartoes(card_id,tentativa,snapshot) VALUES(?1,?2,?3)",
        params![op.card_id, op.id, op.snapshot_id],
    )?;
    tx.execute("INSERT INTO donna_conector_tentativas(id,ingress,instance,bot,channel,state,deadline,rev,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![op.id,op.ingress,op.source.instance,op.context.bot,op.context.channel,op.state.name(),op.card_deadline,op.rev,serde_json::to_string(&op)?])?;
    tx.commit()?;
    Ok((op, emitted))
}

/// CAS e projeção compartilham a transação. Não recria Entry apagada.
pub(crate) fn change(
    c: &Connection,
    id: &str,
    action: impl FnOnce(&Connection, &mut Tentativa) -> Result<Option<String>>,
) -> Result<(Tentativa, Vec<Entry>)> {
    let tx = c.unchecked_transaction()?;
    let mut op = read(&tx, id)?.ok_or_else(|| anyhow!("Tentativa não encontrada"))?;
    let old_rev = op.rev;
    let old_card = op.card_id.clone();
    let sealed = action(&tx, &mut op)?;
    let mut emitted = Vec::new();
    if old_card == op.card_id {
        update_projection(&tx, &op.card_id, &op.public(), &mut emitted)?;
        op.rev = if let Some(entry) = emitted.last() { entry.rev } else { next_rev(&tx)? };
    } else {
        update_projection(
            &tx,
            &old_card,
            &serde_json::json!({"author":op.context.bot,"status":"cancelled",
            "voiceApproval":"app_only","text":"Cartão substituído por uma edição."}),
            &mut emitted,
        )?;
        if let Some(marker) = entries::insert_marker(&tx, &op.context.lane, op.context.turn)? {
            emitted.push(marker);
        }
        let entry =
            entries::insert_row(&tx, &op.card_id, &op.context.lane, "permission", op.context.turn, &op.public())?;
        op.rev = entry.rev;
        emitted.push(entry);
        tx.execute(
            "INSERT INTO donna_conector_cartoes(card_id,tentativa,snapshot) VALUES(?1,?2,?3)",
            params![op.card_id, op.id, op.snapshot_id],
        )?;
    }
    if let Some(sealed) = sealed {
        ensure!(sealed.starts_with("vault:v1:"), "Selo inválido");
        crate::market::vault::write_sealed(&tx, &op.slot(), &sealed)?;
    }
    if op.state.terminal() {
        tx.execute("DELETE FROM kv WHERE k=?1", [op.slot()])?;
    }
    let changed = tx.execute(
        "UPDATE donna_conector_tentativas SET state=?2,rev=?3,data=?4 WHERE id=?1 AND rev=?5",
        params![op.id, op.state.name(), op.rev, serde_json::to_string(&op)?, old_rev],
    )?;
    ensure!(changed == 1, "Revisão concorrente");
    tx.commit()?;
    Ok((op, emitted))
}
fn update_projection(c: &Connection, id: &str, data: &serde_json::Value, emitted: &mut Vec<Entry>) -> Result<()> {
    // Preserva carimbo de Space e procedência. A entrada continua na mesma sessão.
    let current: Option<(String, String)> =
        c.query_row("SELECT bot_id,data FROM entries WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    if let Some((chat, before)) = current {
        let before = serde_json::from_str(&before)?;
        let next = super::guard_hub::preserve(&before, data);
        let next = crate::fork::spaces::stamp_entry(c, &chat, &next)?;
        c.execute(
            "UPDATE entries SET data=?2,rev=?3,updated_at=?4 WHERE id=?1",
            params![id, next.to_string(), next_rev(c)?, now_ms()],
        )?;
        // A leitura normal do Store obteria o mesmo DB lock. Ler por Connection.
        let entry = entries::read_on(c, id)?.ok_or_else(|| anyhow!("Projeção perdida"))?;
        emitted.push(entry);
    }
    Ok(())
}

impl Store {
    pub(crate) fn conector_attempt(&self, id: &str) -> Result<Option<Tentativa>> {
        read(&self.db.locked(), id)
    }
    pub(crate) fn conector_card(&self, id: &str) -> Result<bool> {
        Ok(self.db.locked().query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_conector_cartoes WHERE card_id=?1)",
            [id],
            |r| r.get(0),
        )?)
    }
    pub(crate) fn conector_recover(&self) -> Result<()> {
        let c = self.db.locked();
        let tx = c.unchecked_transaction()?;
        let ids: Vec<String> = tx
            .prepare("SELECT id FROM donna_conector_tentativas WHERE state IN ('aguardando','autorizada','em_envio')")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        // change usa sua própria transação; recuperar tudo numa única cerca SQL.
        for id in ids {
            let mut op = read(&tx, &id)?.ok_or_else(|| anyhow!("Ledger incompleto"))?;
            op.state = match op.state {
                Estado::Aguardando => Estado::Expirada,
                Estado::Autorizada => Estado::Cancelada,
                Estado::EmEnvio => Estado::Incerta,
                _ => unreachable!(),
            };
            op.completed_at = Some(now_ms());
            op.delivery = "retida".into();
            let mut projections = Vec::new();
            update_projection(&tx, &op.card_id, &op.public(), &mut projections)?;
            op.rev = if let Some(entry) = projections.last() { entry.rev } else { next_rev(&tx)? };
            tx.execute(
                "UPDATE donna_conector_tentativas SET state=?2,rev=?3,data=?4 WHERE id=?1",
                params![op.id, op.state.name(), op.rev, serde_json::to_string(&op)?],
            )?;
            tx.execute("DELETE FROM kv WHERE k=?1", [op.slot()])?;
        }
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn human_entry(c: &Connection, id: &str) -> Result<Entry> {
    entries::read_on(c, id)?.ok_or_else(|| anyhow!("Mensagem humana indisponível"))
}

pub(crate) fn cipher(c: &Connection, slot: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [slot], |r| r.get(0)).optional()?)
}
