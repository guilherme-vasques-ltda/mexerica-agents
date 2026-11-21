-- Mail Manager: caixas, remetentes, concessões, referências opacas e recibos de efeito.
-- Tabelas próprias. Não leem nem reaproveitam o esquema legado de e-mail do registro de
-- plugins (builtin 'email', recurso 'mailbox'), que continua só como compatibilidade.
CREATE TABLE IF NOT EXISTS mail_manager_plugin(
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),
    state TEXT NOT NULL CHECK(state IN ('disabled','enabled'))
) STRICT;
CREATE TABLE IF NOT EXISTS mail_manager_mailboxes(
    id TEXT PRIMARY KEY NOT NULL,
    label TEXT NOT NULL
) STRICT;
-- Um endereço pertence a uma única caixa: é por ele que o Hub confere o remetente.
-- Endereços são ASCII; a unicidade vale para a forma canônica (toda em minúsculas), para que duas caixas não
-- reivindiquem o mesmo endereço com só a caixa das letras diferente; o envio usa o original.
CREATE TABLE IF NOT EXISTS mail_manager_addresses(
    canonical TEXT PRIMARY KEY NOT NULL,
    address TEXT NOT NULL,
    mailbox_id TEXT NOT NULL REFERENCES mail_manager_mailboxes(id),
    CHECK(canonical=lower(address))
) STRICT;
CREATE TABLE IF NOT EXISTS mail_manager_grants(
    id TEXT PRIMARY KEY NOT NULL,
    mailbox_id TEXT NOT NULL REFERENCES mail_manager_mailboxes(id),
    participant_kind TEXT NOT NULL CHECK(participant_kind IN ('bot','person')),
    participant_id TEXT NOT NULL,
    permission TEXT NOT NULL CHECK(permission IN ('view','send','delete')),
    state TEXT NOT NULL CHECK(state IN ('active','revoked'))
) STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS mail_manager_grants_active
    ON mail_manager_grants(mailbox_id,participant_kind,participant_id,permission) WHERE state='active';
CREATE INDEX IF NOT EXISTS mail_manager_grants_participant
    ON mail_manager_grants(participant_kind,participant_id,state);
CREATE TRIGGER IF NOT EXISTS mail_manager_grants_identity
BEFORE UPDATE OF id,mailbox_id,participant_kind,participant_id,permission ON mail_manager_grants
BEGIN SELECT RAISE(ABORT,'mail_manager_constraint'); END;
CREATE TRIGGER IF NOT EXISTS mail_manager_grants_no_resurrection
BEFORE UPDATE OF state ON mail_manager_grants WHEN OLD.state='revoked' AND NEW.state<>'revoked'
BEGIN SELECT RAISE(ABORT,'mail_manager_constraint'); END;
-- Referência opaca emitida pelo Hub, presa a uma caixa e com o tipo explícito.
CREATE TABLE IF NOT EXISTS mail_manager_refs(
    handle TEXT PRIMARY KEY NOT NULL,
    mailbox_id TEXT NOT NULL REFERENCES mail_manager_mailboxes(id),
    kind TEXT NOT NULL CHECK(kind IN ('message','attachment')),
    message_id TEXT NOT NULL CHECK(message_id<>''),
    attachment_id TEXT NOT NULL,
    CHECK((kind='message' AND attachment_id='') OR (kind='attachment' AND attachment_id<>'')),
    UNIQUE(mailbox_id,kind,message_id,attachment_id)
) STRICT;
-- Recibo por participante e chave de operação: 'pending' foi reservado antes do efeito,
-- 'done' guarda o resultado. Repetir a chave nunca repete o efeito.
CREATE TABLE IF NOT EXISTS mail_manager_receipts(
    participant_kind TEXT NOT NULL CHECK(participant_kind IN ('bot','person')),
    participant_id TEXT NOT NULL,
    key TEXT NOT NULL,
    request TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','done')),
    outcome TEXT,
    CHECK((state='done')=(outcome IS NOT NULL)),
    PRIMARY KEY(participant_kind,participant_id,key)
) STRICT;
