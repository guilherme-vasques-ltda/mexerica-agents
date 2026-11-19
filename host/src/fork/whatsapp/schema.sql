-- WhatsApp R1: só ajustes. Ligado/desligado vive em donna_plugin_installations.
CREATE TABLE IF NOT EXISTS donna_whatsapp_hub(
    id INTEGER PRIMARY KEY CHECK(id=1),
    mode TEXT NOT NULL CHECK(mode IN ('donna_only','per_bot')),
    transcribe_provider TEXT CHECK(transcribe_provider IN ('openai','gemini')),
    transcribe_model TEXT,
    CHECK((transcribe_provider IS NULL)=(transcribe_model IS NULL))
) STRICT;
CREATE TABLE IF NOT EXISTS donna_whatsapp_bots(
    bot_id TEXT PRIMARY KEY NOT NULL,
    read_chats INTEGER NOT NULL CHECK(read_chats IN (0,1)),
    reply_chats INTEGER NOT NULL CHECK(reply_chats IN (0,1)),
    read_groups INTEGER NOT NULL CHECK(read_groups IN (0,1)),
    reply_groups INTEGER NOT NULL CHECK(reply_groups IN (0,1)),
    reply_mode TEXT NOT NULL CHECK(reply_mode IN ('approval','autonomous')),
    -- R4b (D357): regras da conexão. Intervalo X de 8 a 300 s; limite de 1 a 200 por hora.
    delay_seconds INTEGER NOT NULL CHECK(delay_seconds BETWEEN 8 AND 300),
    hourly_limit INTEGER NOT NULL CHECK(hourly_limit BETWEEN 1 AND 200),
    transcribe_audio INTEGER NOT NULL CHECK(transcribe_audio IN (0,1)),
    -- R8: por quantos dias a caixa do bot guarda as mensagens (D302: padrão 30).
    retention_days INTEGER NOT NULL DEFAULT 30 CHECK(retention_days IN (7,30,90)),
    -- R8: sobe a cada troca do prazo; o app descarta o que tinha carregado de uma versão anterior.
    retention_rev INTEGER NOT NULL DEFAULT 0 CHECK(retention_rev>=0),
    -- MX-18: limite por dia (janela móvel de 24 h) e silêncio por número.
    daily_limit INTEGER NOT NULL DEFAULT 100 CHECK(daily_limit BETWEEN 1 AND 500),
    quiet_enabled INTEGER NOT NULL DEFAULT 1 CHECK(quiet_enabled IN (0,1)),
    quiet_start INTEGER NOT NULL DEFAULT 1320 CHECK(quiet_start BETWEEN 0 AND 1439),
    quiet_end INTEGER NOT NULL DEFAULT 480 CHECK(quiet_end BETWEEN 0 AND 1439),
    quiet_time_zone TEXT,
    CHECK(reply_chats<=read_chats),
    CHECK(reply_groups<=read_groups)
) STRICT;
-- R2: o número de cada bot. Sem linha = nenhum número. Sessão e mensagens ficam
-- na pasta do bot (whatsapp/<bot_id>/), nunca aqui. account_id muda a cada pareamento.
CREATE TABLE IF NOT EXISTS donna_whatsapp_numbers(
    bot_id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK(state IN ('linked','logged_out')),
    linked_at INTEGER NOT NULL,
    -- Número confirmado pelo wacli (`auth status`) para a conta deste pareamento; NULL até confirmar.
    phone TEXT
) STRICT;
-- R3: até onde o Hub já leu o wacli.db do bot (rowid), por número. Mensagens não ficam aqui:
-- o que o bot pode ver entra na conversa dele; o resto só passa pelo cursor.
CREATE TABLE IF NOT EXISTS donna_whatsapp_cursors(
    bot_id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL,
    last_rowid INTEGER NOT NULL CHECK(last_rowid>=0),
    -- R5c: o mesmo para as chamadas (`call_events` do wacli).
    last_call_rowid INTEGER NOT NULL DEFAULT 0 CHECK(last_call_rowid>=0)
) STRICT;
-- R3: a caixa do WhatsApp de cada bot (tela própria do dono; nunca o chat do bot, as
-- Sessões, o motor ou o histórico consultável). Só o necessário, com prazo (D302).
CREATE TABLE IF NOT EXISTS donna_whatsapp_messages(
    bot_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    is_group INTEGER NOT NULL CHECK(is_group IN (0,1)),
    chat_name TEXT,
    sender_name TEXT NOT NULL,
    sender_phone TEXT,
    sent_at INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    body TEXT NOT NULL,
    media TEXT,
    offline INTEGER NOT NULL CHECK(offline IN (0,1)),
    read INTEGER NOT NULL CHECK(read IN (0,1)),
    -- R5c: remetente salvo na agenda (sem isso o app mostra "~Nome"); chamada recebida.
    sender_contact INTEGER NOT NULL DEFAULT 0 CHECK(sender_contact IN (0,1)),
    call_media TEXT CHECK(call_media IN ('audio','video')),
    call_secs INTEGER,
    -- R3b: mensagem de voz transcrita (conteúdo de fora, D125; só o dono vê; mesmo prazo).
    mime TEXT,
    file_length INTEGER,
    transcript TEXT,
    transcript_state TEXT CHECK(transcript_state IN ('pending','done','too_large','no_key','unsupported','failed')),
    transcript_provider TEXT CHECK(transcript_provider IN ('openai','gemini')),
    transcript_tries INTEGER NOT NULL DEFAULT 0,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY(bot_id,chat_id,message_id)
) STRICT;
CREATE INDEX IF NOT EXISTS donna_whatsapp_messages_chat ON donna_whatsapp_messages(bot_id,chat_id,sent_at);
CREATE INDEX IF NOT EXISTS donna_whatsapp_messages_unread ON donna_whatsapp_messages(bot_id,read);
-- R5c: conversas fixadas no WhatsApp do número (só o JID), lidas do `chats.pinned` do wacli.
CREATE TABLE IF NOT EXISTS donna_whatsapp_pins(
    bot_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    PRIMARY KEY(bot_id,chat_id)
) STRICT;
-- R5c: chamada vista antes de o wacli saber a direção (`internal/wa/calls.go:252-265`); entra
-- quando a reconciliação vê a mesma linha completada. Só IDs.
CREATE TABLE IF NOT EXISTS donna_whatsapp_call_pending(
    bot_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    PRIMARY KEY(bot_id,chat_id,call_id)
) STRICT;
-- R4b (D357): registro dos envios de cada conexão (bot × número), somando todas as rotinas.
-- `jitter_pct` = variação sorteada (0–30 %); o intervalo X é sempre o atual do dono. Só a última hora fica.
CREATE TABLE IF NOT EXISTS donna_whatsapp_pace(
    bot_id TEXT NOT NULL,
    at_ms INTEGER NOT NULL,
    jitter_pct INTEGER NOT NULL CHECK(jitter_pct BETWEEN 0 AND 30)
) STRICT;
CREATE INDEX IF NOT EXISTS donna_whatsapp_pace_bot ON donna_whatsapp_pace(bot_id,at_ms);

-- MX-18: conversas que o dono marcou para o modo "sozinho" responder sozinho.
CREATE TABLE IF NOT EXISTS donna_whatsapp_marked(
    bot_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    marked_at INTEGER NOT NULL,
    auto INTEGER NOT NULL DEFAULT 0 CHECK(auto IN (0,1)),
    PRIMARY KEY(bot_id,chat_id)
) STRICT;

-- MX-18: fila de saída gravada. Uma linha por envio; o id é determinístico para deduplicar.
CREATE TABLE IF NOT EXISTS donna_whatsapp_outbox(
    id TEXT PRIMARY KEY NOT NULL,
    bot_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    is_group INTEGER NOT NULL CHECK(is_group IN (0,1)),
    reply_to TEXT NOT NULL,
    body TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('owner_card','bot_human','bot_routine','owner_direct')),
    state TEXT NOT NULL CHECK(state IN ('pending','held','sending','sent','uncertain','expired','cancelled')),
    reason TEXT,
    not_before INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0),
    lease_until INTEGER,
    last_attempt_at INTEGER,
    sent_at INTEGER,
    sent_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS donna_whatsapp_outbox_due ON donna_whatsapp_outbox(state,not_before);
CREATE INDEX IF NOT EXISTS donna_whatsapp_outbox_bot ON donna_whatsapp_outbox(bot_id,state);
