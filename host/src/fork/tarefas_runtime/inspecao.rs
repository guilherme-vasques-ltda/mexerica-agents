use crate::fork::{
    control_tower::PENDING_PERMISSION,
    owner_question::PENDING_E,
    sessoes::{origin::SessionOrigin, store::SessionRow},
};
use rusqlite::{Connection, params};

pub(super) fn owner_attention(
    db: &Connection,
    space: &str,
    task: &str,
    bot: &str,
    session: &SessionRow,
) -> rusqlite::Result<bool> {
    if session.principal
        || session.id == session.chat_id
        || session.state != "open"
        || !matches!(&session.origin, SessionOrigin::Task { task_id } if task_id == task)
    {
        return Ok(false);
    }
    db.query_row(
        &format!(
            "SELECT EXISTS(
                SELECT 1 FROM entries e
                JOIN donna_space_conversations c ON c.id=e.bot_id AND c.space_id=?1 AND c.bot_id=?2 AND c.closed=0
                WHERE e.bot_id=?3 AND e.session=?4 AND e.seq>=c.context_start_seq
                  AND (SELECT COUNT(*) FROM donna_task_sessions WHERE session_id=?4)=1
                  AND CASE WHEN json_valid(e.data) THEN json_extract(e.data,'$.author')=?2
                      AND (({PENDING_PERMISSION} AND json_extract(e.data,'$.toolKind') IS NOT 'screen_access')
                        OR ({PENDING_E})) ELSE 0 END
            )"
        ),
        params![space, bot, session.chat_id, session.id],
        |row| row.get(0),
    )
}
