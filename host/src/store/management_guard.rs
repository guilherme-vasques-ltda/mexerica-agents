//! D125: exposição persistente para a gestão legada, até o cartão com delta.
impl super::Store {
    /// Gestão falha fechada após ferramenta sem identidade interna verificada ou conteúdo externo
    /// no histórico, inclusive sem conectores. Não depende de título/narrativa do bot.
    /// A marca persiste entre turnos e reinícios; não lê nem duplica corpos nos recibos.
    pub(crate) fn guard_management_exposed(&self, bot: &str, observed_external: bool) -> anyhow::Result<bool> {
        use crate::LockExt;
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let exposed = observed_external || management_exposed_on(&tx, bot)?;
        if exposed {
            mark_management_exposed_on(&tx, bot)?;
        }
        tx.commit()?;
        Ok(exposed)
    }
}

pub(crate) struct ManagementOrigin<'a, F> {
    pub(crate) manager: &'a str,
    pub(crate) observed_external: F,
}

impl<F: FnOnce() -> bool> ManagementOrigin<'_, F> {
    pub(super) fn guard(self, tx: rusqlite::Transaction<'_>) -> anyhow::Result<rusqlite::Transaction<'_>> {
        if management_exposed_on(&tx, self.manager)? || (self.observed_external)() {
            mark_management_exposed_on(&tx, self.manager)?;
            tx.commit()?;
            anyhow::bail!("Gestão recusada: conteúdo externo requer confirmação do dono");
        }
        Ok(tx)
    }
}

fn mark_management_exposed_on(db: &rusqlite::Connection, bot: &str) -> anyhow::Result<()> {
    db.execute(
        "INSERT INTO donna_guard_state(bot,lane,external_exposed) VALUES(?1,'*',1)
         ON CONFLICT(bot,lane) DO UPDATE SET external_exposed=1",
        [bot],
    )?;
    Ok(())
}

fn management_exposed_on(db: &rusqlite::Connection, bot: &str) -> anyhow::Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_guard_state WHERE bot=?1 AND external_exposed=1)
                 OR EXISTS(SELECT 1 FROM entries WHERE bot_id=?1 AND (
                    (kind='tool' AND COALESCE(json_extract(data,'$.hubToolOrigin'),'unknown')<>'internal')
                    OR json_type(data,'$.sourceBotId')='text'
                    OR json_type(data,'$.runId')='text'
                    OR json_array_length(data,'$.attachments')>0
                    OR json_array_length(data,'$.pasted')>0
                    OR json_type(data,'$.replyTo')='object'
                    OR json_array_length(data,'$.contentOrigin.attachments')>0
                    OR json_type(data,'$.contentOrigin.reply')='object'
                    OR EXISTS(SELECT 1 FROM json_each(data,'$.contentOrigin.segments')
                        WHERE json_extract(value,'$.kind') IN ('pasted','quote','attachment','generated')
                        OR (json_extract(data,'$.contentOrigin.status')='appReported'
                            AND json_extract(value,'$.kind')='unknown'))
                 ))",
        [bot],
        |row| row.get(0),
    )?)
}
