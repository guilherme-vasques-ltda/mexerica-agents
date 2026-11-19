//! A caixa do `WhatsApp` de cada bot: tabela própria `donna_whatsapp_messages`, fora do chat
//! do bot (`entries`), das Sessões, do motor e do histórico consultável (busca, memória,
//! exportação). Só o dono lê, pelas três funções de `api.rs`; nenhum bot, motor ou outro
//! membro chega aqui. Guardado só o necessário (D302), pelo prazo do bot (R8: 7, 30 ou 90
//! dias; padrão 30), contado de quando chegou.
//!
//! Ciclo de vida: desligar "Ver" para de receber e mantém o que já chegou até vencer;
//! "Desconectar" mantém; "Apagar esta conversa" apaga só aquela conversa; "Desconectar e
//! apagar" e apagar o bot apagam tudo do bot.
use super::{inbox::Item, store::retention_ms};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

const PAGE_DEFAULT: u32 = 50;
const PAGE_MAX: u32 = 100;
const CHATS_MAX: i64 = 500;

const INSERT: &str = "INSERT INTO donna_whatsapp_messages(bot_id,chat_id,message_id,account_id,is_group,chat_name,
    sender_name,sender_phone,sent_at,received_at,body,media,offline,read,expires_at,sender_contact,call_media,call_secs,
    mime,file_length,transcript_state)
    VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,0,?14,?15,?16,?17,?18,?19,?20)";

fn values(item: &Item) -> (Option<&str>, Option<i64>) {
    let call = item.call.as_ref();
    (call.and_then(|c| c.media.as_deref()), call.map(|c| c.secs))
}

/// Guarda uma mensagem. A mesma mensagem lida de novo (cursor perdido, reinício) não duplica.
pub(super) fn insert(c: &Connection, bot: &str, account: &str, item: &Item, now: i64) -> Result<usize> {
    let (call_media, call_secs) = values(item);
    let keep = retention_ms(c, bot)?;
    Ok(c.execute(
        &format!("{INSERT} ON CONFLICT(bot_id,chat_id,message_id) DO NOTHING"),
        params![
            bot,
            item.chat_id,
            item.message_id,
            account,
            item.group,
            item.chat_name,
            item.sender_name,
            item.sender_phone,
            item.sent_at,
            now,
            item.body,
            item.media,
            item.offline,
            now + keep,
            item.contact,
            call_media,
            call_secs,
            item.mime,
            item.file_length,
            item.transcribe.then_some("pending"),
        ],
    )?)
}

/// Guarda uma chamada (um item por `call_id`). O que o wacli informa depois vale: tipo
/// (voz/vídeo, com o rótulo) e duração são trocados no mesmo item quando o evento os traz
/// (`internal/store/calls.go:120-140`); evento sem tipo ou sem duração não apaga o que já
/// havia. Identidade, lida e prazo de guarda ficam.
pub(super) fn insert_call(c: &Connection, bot: &str, account: &str, item: &Item, now: i64) -> Result<usize> {
    let (call_media, call_secs) = values(item);
    let keep = retention_ms(c, bot)?;
    Ok(c.execute(
        &format!(
            "{INSERT} ON CONFLICT(bot_id,chat_id,message_id) DO UPDATE SET
             body=CASE WHEN excluded.call_media IS NOT NULL THEN excluded.body ELSE body END,
             call_media=COALESCE(excluded.call_media,call_media),
             call_secs=CASE WHEN COALESCE(excluded.call_secs,0)>0 THEN excluded.call_secs ELSE call_secs END
             WHERE (excluded.call_media IS NOT NULL AND excluded.call_media IS NOT call_media)
                OR (COALESCE(excluded.call_secs,0)>0 AND excluded.call_secs IS NOT call_secs)"
        ),
        params![
            bot,
            item.chat_id,
            item.message_id,
            account,
            item.group,
            item.chat_name,
            item.sender_name,
            item.sender_phone,
            item.sent_at,
            now,
            item.body,
            item.media,
            item.offline,
            now + keep,
            item.contact,
            call_media,
            call_secs,
            item.mime,
            item.file_length,
            item.transcribe.then_some("pending"),
        ],
    )?)
}

/// A chamada já está na caixa (a reconciliação só a completa).
pub(super) fn call_known(c: &Connection, bot: &str, item: &Item) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_whatsapp_messages WHERE bot_id=?1 AND chat_id=?2 AND message_id=?3)",
        params![bot, item.chat_id, item.message_id],
        |r| r.get(0),
    )?)
}

/// Chamada vista sem direção ainda: a reconciliação a guarda quando o wacli completar.
pub(super) fn mark_pending(c: &Connection, bot: &str, chat: &str, call_id: &str) -> Result<()> {
    c.execute(
        "INSERT OR IGNORE INTO donna_whatsapp_call_pending(bot_id,chat_id,call_id) VALUES(?1,?2,?3)",
        params![bot, chat, call_id],
    )?;
    Ok(())
}

/// Tira a chamada da espera; `true` se ela estava esperando.
pub(super) fn take_pending(c: &Connection, bot: &str, chat: &str, call_id: &str) -> Result<bool> {
    Ok(c.execute(
        "DELETE FROM donna_whatsapp_call_pending WHERE bot_id=?1 AND chat_id=?2 AND call_id=?3",
        params![bot, chat, call_id],
    )? > 0)
}

/// Troca as conversas fixadas do bot pelas do número; `true` se mudou algo.
pub(super) fn set_pins(c: &Connection, bot: &str, chats: &[String]) -> Result<bool> {
    let mut stmt = c.prepare("SELECT chat_id FROM donna_whatsapp_pins WHERE bot_id=?1 ORDER BY chat_id")?;
    let current = stmt.query_map([bot], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut wanted = chats.to_vec();
    wanted.sort();
    wanted.dedup();
    if current == wanted {
        return Ok(false);
    }
    let tx = c.unchecked_transaction()?;
    tx.execute("DELETE FROM donna_whatsapp_pins WHERE bot_id=?1", [bot])?;
    for chat in &wanted {
        tx.execute("INSERT INTO donna_whatsapp_pins(bot_id,chat_id) VALUES(?1,?2)", params![bot, chat])?;
    }
    tx.commit()?;
    Ok(true)
}

/// Limpeza de todas as caixas, independente de número conectado, plugin ou pausa (início do
/// Hub e de hora em hora). Devolve os bots que perderam mensagens, para o aviso à barra.
pub(super) fn expire_all(c: &Connection, now: i64) -> Result<Vec<String>> {
    let mut stmt = c.prepare("SELECT DISTINCT bot_id FROM donna_whatsapp_messages WHERE expires_at<=?1")?;
    let bots = stmt.query_map([now], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
    c.execute("DELETE FROM donna_whatsapp_messages WHERE expires_at<=?1", [now])?;
    Ok(bots)
}

/// O dono trocou o prazo do bot: o vencimento de cada mensagem ainda guardada passa a contar o
/// prazo novo desde a chegada. O que já venceu sai antes, na mesma transação: alongar nunca
/// traz de volta o que já venceu, mesmo antes da limpeza de hora em hora.
pub(super) fn retime(c: &Connection, bot: &str, keep: i64) -> Result<()> {
    c.execute("DELETE FROM donna_whatsapp_messages WHERE bot_id=?1 AND expires_at<=?2", params![bot, now()])?;
    c.execute("UPDATE donna_whatsapp_messages SET expires_at=received_at+?2 WHERE bot_id=?1", params![bot, keep])?;
    Ok(())
}

/// Agora, para nunca entregar o que já venceu (mesmo antes da próxima limpeza).
fn now() -> i64 {
    crate::store::now_ms()
}

/// "Desconectar e apagar" e bot apagado: tudo do bot sai.
pub(super) fn erase(c: &Connection, bot: &str) -> Result<()> {
    c.execute("DELETE FROM donna_whatsapp_messages WHERE bot_id=?1", [bot])?;
    c.execute("DELETE FROM donna_whatsapp_pins WHERE bot_id=?1", [bot])?;
    c.execute("DELETE FROM donna_whatsapp_call_pending WHERE bot_id=?1", [bot])?;
    Ok(())
}

/// "Apagar esta conversa deste computador": só aquela conversa, só daquele bot. Devolve
/// quantas mensagens saíram.
pub(super) fn erase_chat(c: &Connection, bot: &str, chat: &str) -> Result<usize> {
    let tx = c.unchecked_transaction()?;
    let n = tx.execute("DELETE FROM donna_whatsapp_messages WHERE bot_id=?1 AND chat_id=?2", params![bot, chat])?;
    tx.execute("DELETE FROM donna_whatsapp_pins WHERE bot_id=?1 AND chat_id=?2", params![bot, chat])?;
    tx.execute("DELETE FROM donna_whatsapp_call_pending WHERE bot_id=?1 AND chat_id=?2", params![bot, chat])?;
    tx.commit()?;
    Ok(n)
}

/// Não lidas do bot, para o contador da barra (leitura `donnaWhatsapp`).
pub(super) fn unread(c: &Connection, bot: &str) -> Result<i64> {
    Ok(c.query_row(
        "SELECT COUNT(*) FROM donna_whatsapp_messages WHERE bot_id=?1 AND read=0 AND expires_at>?2",
        params![bot, now()],
        |r| r.get(0),
    )?)
}

/// `{state, text, provider}` da transcrição de uma mensagem de voz, ou `null` (não transcreve).
fn transcript_json(state: Option<String>, text: Option<&str>, provider: Option<&str>) -> Value {
    let Some(state) = state else { return Value::Null };
    json!({"state": state, "text": text, "provider": provider})
}

/// Uma mensagem de voz esperando transcrição.
pub(super) struct Voice {
    /// A conta (número) em que a voz chegou: só ela autoriza transcrever.
    pub account: String,
    pub chat_id: String,
    pub message_id: String,
    pub group: bool,
    pub file_length: Option<i64>,
    pub mime: Option<String>,
}

/// As mensagens de voz do bot esperando transcrição, da mais antiga para a mais nova (inclusive
/// as que chegaram com o computador desligado). No máximo 2 tentativas por mensagem.
pub(super) fn pending_voice(c: &Connection, bot: &str, limit: i64) -> Result<Vec<Voice>> {
    let mut stmt = c.prepare(
        "SELECT chat_id,message_id,is_group,file_length,mime,account_id FROM donna_whatsapp_messages
         WHERE bot_id=?1 AND transcript_state='pending' AND transcript_tries<2 AND expires_at>?2
         ORDER BY sent_at,rowid LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![bot, now(), limit], |r| {
        Ok(Voice {
            account: r.get(5)?,
            chat_id: r.get(0)?,
            message_id: r.get(1)?,
            group: r.get(2)?,
            file_length: r.get(3)?,
            mime: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Fim da transcrição de uma voz: texto e provedor (`done`), outro estado final, ou `None` =
/// não transcrever (opção desligada ou "Ver" revogado depois).
pub(super) fn finish_voice(
    c: &Connection,
    bot: &str,
    voice: &Voice,
    state: Option<&str>,
    text: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    c.execute(
        "UPDATE donna_whatsapp_messages SET transcript_state=?4,transcript=?5,transcript_provider=?6
         WHERE bot_id=?1 AND chat_id=?2 AND message_id=?3 AND account_id=?7 AND transcript_state='pending'",
        params![bot, voice.chat_id, voice.message_id, state, text, provider, voice.account],
    )?;
    Ok(())
}

/// Tentativa que falhou: conta e, na segunda, encerra como `failed` (sem repetir em laço).
pub(super) fn fail_voice(c: &Connection, bot: &str, voice: &Voice) -> Result<()> {
    c.execute(
        "UPDATE donna_whatsapp_messages SET transcript_tries=transcript_tries+1,
         transcript_state=CASE WHEN transcript_tries+1>=2 THEN 'failed' ELSE 'pending' END
         WHERE bot_id=?1 AND chat_id=?2 AND message_id=?3 AND transcript_state='pending'",
        params![bot, voice.chat_id, voice.message_id],
    )?;
    Ok(())
}

/// `{media, durationSecs}` da chamada, ou `null`.
fn call_json(media: Option<&str>, secs: Option<i64>, is_call: bool) -> Value {
    if !is_call {
        return Value::Null;
    }
    json!({"media": media, "durationSecs": secs.filter(|s| *s > 0)})
}

/// Conversas do bot, como no `WhatsApp`: fixadas primeiro, depois a mais recente. Cada uma
/// traz nome, número (individual), alfinete, última mensagem (remetente, se é contato
/// salvo, mídia, chamada), não lidas e `marked` (o dono marcou para o modo "sozinho", MX-18).
/// `account`: só as mensagens dessa conta (o número atual); `None` = toda a caixa do bot (tela do dono).
pub(super) fn chats(c: &Connection, bot: &str, account: Option<&str>) -> Result<Value> {
    // A marca vale por conta: na tela do dono (`None`) vale a conta ligada agora.
    let marked_account = match account {
        Some(account) => Some(account.to_owned()),
        None => c
            .query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1 AND state='linked'", [bot], |r| {
                r.get(0)
            })
            .optional()?,
    };
    let marked: std::collections::HashMap<String, bool> = match marked_account {
        Some(account) => {
            let mut stmt =
                c.prepare("SELECT chat_id,auto FROM donna_whatsapp_marked WHERE bot_id=?1 AND account_id=?2")?;
            stmt.query_map(params![bot, account], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        }
        None => std::collections::HashMap::default(),
    };
    let mut stmt = c.prepare(
        "SELECT m.chat_id,m.is_group,m.chat_name,m.sender_name,m.body,m.media,m.sent_at,m.offline,
           (SELECT COUNT(*) FROM donna_whatsapp_messages u WHERE u.bot_id=m.bot_id AND u.chat_id=m.chat_id
              AND u.read=0 AND u.expires_at>?3 AND (?4 IS NULL OR u.account_id=?4)),
           m.sender_phone,m.sender_contact,m.call_media,m.call_secs,m.message_id LIKE 'call:%',
           EXISTS(SELECT 1 FROM donna_whatsapp_pins p WHERE p.bot_id=m.bot_id AND p.chat_id=m.chat_id) AS pinned
         FROM donna_whatsapp_messages m
         WHERE m.bot_id=?1 AND m.expires_at>?3 AND m.rowid=(SELECT l.rowid FROM donna_whatsapp_messages l
           WHERE l.bot_id=m.bot_id AND l.chat_id=m.chat_id AND l.expires_at>?3 AND (?4 IS NULL OR l.account_id=?4)
           ORDER BY l.sent_at DESC,l.rowid DESC LIMIT 1)
         ORDER BY pinned DESC,m.sent_at DESC,m.rowid DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![bot, CHATS_MAX, now(), account], |r| {
        let group = r.get::<_, bool>(1)?;
        let chat_id: String = r.get(0)?;
        Ok(json!({
            "whatsappChatId": chat_id,
            "marked": marked.contains_key(&chat_id),
            "groupAuto": group && marked.get(&chat_id).copied().unwrap_or(false),
            "kind": if group { "group" } else { "individual" },
            "name": r.get::<_, Option<String>>(2)?,
            // Individual: o número, para quando não há nome de contato. Grupo: nenhum.
            "phone": if group { None } else { r.get::<_, Option<String>>(9)? },
            "pinned": r.get::<_, bool>(14)?,
            "last": {
                "senderName": r.get::<_, String>(3)?,
                "senderContact": r.get::<_, bool>(10)?,
                "body": r.get::<_, String>(4)?,
                "media": r.get::<_, Option<String>>(5)?,
                "call": call_json(r.get::<_, Option<String>>(11)?.as_deref(), r.get(12)?, r.get(13)?),
                "sentAt": r.get::<_, i64>(6)?,
                "offline": r.get::<_, bool>(7)?,
            },
            "unread": r.get::<_, i64>(8)?,
        }))
    })?;
    let chats = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"botId": bot, "chats": chats, "unread": unread(c, bot)?}))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Before {
    sent_at: i64,
    seq: i64,
}

/// Mensagens de uma conversa, da mais recente para a mais antiga, em páginas. `next` é o
/// `before` da página seguinte (ausente = acabou).
pub(super) fn page(
    c: &Connection,
    bot: &str,
    chat: &str,
    account: Option<&str>,
    before: Option<&Before>,
    limit: Option<u32>,
) -> Result<Value> {
    let limit = limit.unwrap_or(PAGE_DEFAULT);
    ensure!((1..=PAGE_MAX).contains(&limit), "invalid_field: o limite vai de 1 a 100.");
    let (at, seq) = before.map_or((i64::MAX, i64::MAX), |b| (b.sent_at, b.seq));
    let mut stmt = c.prepare(
        "SELECT rowid,message_id,sender_name,sender_phone,sent_at,body,media,offline,read,
           sender_contact,call_media,call_secs,message_id LIKE 'call:%',received_at,
           transcript_state,transcript,transcript_provider
         FROM donna_whatsapp_messages WHERE bot_id=?1 AND chat_id=?2 AND (sent_at,rowid)<(?3,?4) AND expires_at>?6
           AND (?7 IS NULL OR account_id=?7)
         ORDER BY sent_at DESC,rowid DESC LIMIT ?5",
    )?;
    let rows = stmt.query_map(params![bot, chat, at, seq, i64::from(limit) + 1, now(), account], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(4)?,
            json!({
                "seq": r.get::<_, i64>(0)?,
                "messageId": r.get::<_, String>(1)?,
                // `contact: false` = só o nome do perfil: o app mostra "~Nome" (como o WhatsApp).
                "from": {
                    "name": r.get::<_, String>(2)?,
                    "phone": r.get::<_, Option<String>>(3)?,
                    "contact": r.get::<_, bool>(9)?,
                },
                "call": call_json(r.get::<_, Option<String>>(10)?.as_deref(), r.get(11)?, r.get(12)?),
                // R3b: voz transcrita (conteúdo de fora, D125) ou o estado da transcrição.
                "transcript": transcript_json(
                    r.get(14)?,
                    r.get::<_, Option<String>>(15)?.as_deref(),
                    r.get::<_, Option<String>>(16)?.as_deref(),
                ),
                "sentAt": r.get::<_, i64>(4)?,
                "body": r.get::<_, String>(5)?,
                "media": r.get::<_, Option<String>>(6)?,
                "offline": r.get::<_, bool>(7)?,
                "read": r.get::<_, bool>(8)?,
                // R8: com `keepAfter`, o app tira da tela o que já venceu.
                "receivedAt": r.get::<_, i64>(13)?,
                // D125: conteúdo de fora, nunca ordem.
                "external": true,
            }),
        ))
    })?;
    let mut rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next = more.then(|| rows.last().map(|(seq, at, _)| json!({"sentAt": at, "seq": seq}))).flatten();
    let header: Option<(bool, Option<String>)> = c
        .query_row(
            "SELECT is_group,chat_name FROM donna_whatsapp_messages WHERE bot_id=?1 AND chat_id=?2 AND expires_at>?3
               AND (?4 IS NULL OR account_id=?4)
             ORDER BY sent_at DESC,rowid DESC LIMIT 1",
            params![bot, chat, now(), account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (group, name) = header.unwrap_or((false, None));
    // R8: o prazo é o mesmo para todas as mensagens do bot (contado da chegada); o que chegou
    // até `keepAfter` já venceu, inclusive páginas que o app já tinha carregado.
    let keep_after = now() - retention_ms(c, bot)?;
    // Versão do prazo: mudou desde a página que o app tem = o app troca o que tinha, sem juntar.
    let retention_rev: i64 = c
        .query_row("SELECT retention_rev FROM donna_whatsapp_bots WHERE bot_id=?1", [bot], |r| r.get(0))
        .optional()?
        .unwrap_or(0);
    let messages: Vec<Value> = rows.into_iter().map(|(_, _, m)| m).collect();
    Ok(json!({
        "botId": bot,
        "whatsappChatId": chat,
        "kind": if group { "group" } else { "individual" },
        "name": name,
        "messages": messages,
        "next": next,
        "keepAfter": keep_after,
        "retentionRev": retention_rev,
    }))
}

/// Marca a conversa como lida; devolve as não lidas que sobraram no bot.
pub(super) fn mark_read(c: &Connection, bot: &str, chat: &str) -> Result<(usize, i64)> {
    let changed = c.execute(
        "UPDATE donna_whatsapp_messages SET read=1 WHERE bot_id=?1 AND chat_id=?2 AND read=0 AND expires_at>?3",
        params![bot, chat, now()],
    )?;
    Ok((changed, unread(c, bot)?))
}
