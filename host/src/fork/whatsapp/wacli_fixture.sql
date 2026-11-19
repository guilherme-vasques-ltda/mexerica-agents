-- Fixture de teste do R3/R5c: tabelas `chats`, `contacts`, `messages` e `call_events` (com seus índices) do schema real do wacli
-- v0.25.1 (openclaw/wacli @ b7603b697e69be591afa83379ced1e73177604c6,
-- internal/store/schema.sql, MIT, ver THIRD_PARTY.md), copiadas sem alteração. Só para testes:
-- o Hub abre o wacli.db de verdade só para leitura e nunca cria estas tabelas.
CREATE TABLE IF NOT EXISTS chats (
    jid TEXT PRIMARY KEY,
    kind TEXT NOT NULL, -- dm|group|broadcast|newsletter|unknown
    name TEXT,
    last_message_ts INTEGER,
    archived INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    muted_until INTEGER NOT NULL DEFAULT 0,
    unread INTEGER NOT NULL DEFAULT 0,
    unread_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS contacts (
    jid TEXT PRIMARY KEY,
    phone TEXT,
    push_name TEXT,
    full_name TEXT,
    first_name TEXT,
    business_name TEXT,
    system_name TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    rowid INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_jid TEXT NOT NULL,
    chat_name TEXT,
    msg_id TEXT NOT NULL,
    sender_jid TEXT,
    sender_name TEXT,
    ts INTEGER NOT NULL,
    from_me INTEGER NOT NULL,
    text TEXT,
    display_text TEXT,
    quoted_msg_id TEXT,
    quoted_sender_jid TEXT,
    is_forwarded INTEGER NOT NULL DEFAULT 0,
    forwarding_score INTEGER NOT NULL DEFAULT 0,
    reaction_to_id TEXT,
    reaction_emoji TEXT,
    media_type TEXT,
    media_caption TEXT,
    filename TEXT,
    mime_type TEXT,
    direct_path TEXT,
    media_key BLOB,
    file_sha256 BLOB,
    file_enc_sha256 BLOB,
    file_length INTEGER,
    local_path TEXT,
    downloaded_at INTEGER,
    media_unavailable_at INTEGER,
    revoked INTEGER NOT NULL DEFAULT 0,
    deleted_for_me INTEGER NOT NULL DEFAULT 0,
    deleted_at INTEGER,
    deletion_reason TEXT,
    payload_purged_at INTEGER,
    edited INTEGER NOT NULL DEFAULT 0,
    edited_ts INTEGER NOT NULL DEFAULT 0,
    buttons TEXT,
    UNIQUE(chat_jid, msg_id),
    FOREIGN KEY (chat_jid) REFERENCES chats(jid) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS call_events (
    rowid INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_jid TEXT NOT NULL,
    chat_name TEXT,
    sender_jid TEXT,
    sender_name TEXT,
    call_id TEXT NOT NULL,
    msg_id TEXT,
    event_type TEXT NOT NULL,
    direction TEXT,
    media TEXT,
    outcome TEXT,
    reason TEXT,
    call_type TEXT,
    duration_secs INTEGER NOT NULL DEFAULT 0,
    ts INTEGER NOT NULL,
    participants TEXT,
    UNIQUE(chat_jid, call_id, event_type, ts),
    FOREIGN KEY (chat_jid) REFERENCES chats(jid) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_call_events_chat_ts ON call_events(chat_jid, ts);
CREATE INDEX IF NOT EXISTS idx_call_events_ts ON call_events(ts);
