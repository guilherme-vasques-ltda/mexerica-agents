-- D537: email/mailbox/mailbox_*/local_draft são apenas compatibilidade de esquema
-- legado. Não estreitar CHECKs nem remover linhas; runtime não aceita esses valores.
-- Inventário e backlog: docs/archive/d537-email/schema-legado.md.
-- F1b, schema 1: somente DDL; não provisiona estado nem restaura linhas.
-- WhatsApp R1: 'whatsapp' entrou no CHECK; bancos antigos são reconstruídos em schema.rs.
-- WhatsApp R3: recurso 'whatsapp_number' e capacidades whatsapp_read_*; idem (recursos e grants).
CREATE TABLE IF NOT EXISTS donna_plugin_installations(
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL CHECK(version=1),
    hub_id TEXT NOT NULL,
    builtin TEXT NOT NULL CHECK(builtin IN ('scatty','email','whatsapp')),
    state TEXT NOT NULL CHECK(state IN ('disabled','enabled')),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 9007199254740991),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991),
    UNIQUE(hub_id,builtin)
) STRICT;
CREATE TABLE IF NOT EXISTS donna_plugin_accounts(
    id TEXT PRIMARY KEY NOT NULL,
    installation_id TEXT NOT NULL REFERENCES donna_plugin_installations(id),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 9007199254740991),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991),
    UNIQUE(installation_id,id)
) STRICT;
CREATE TABLE IF NOT EXISTS donna_plugin_resources(
    id TEXT PRIMARY KEY NOT NULL,
    installation_id TEXT NOT NULL REFERENCES donna_plugin_installations(id),
    owner TEXT NOT NULL CHECK(owner IN ('native','external')),
    hub_id TEXT,
    space_id TEXT,
    account_id TEXT,
    kind TEXT NOT NULL CHECK(kind IN ('calendar','mailbox','whatsapp_number')),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 9007199254740991),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991),
    CHECK((owner='native' AND hub_id IS NOT NULL AND space_id IS NOT NULL AND account_id IS NULL AND kind='calendar')
       OR (owner='external' AND hub_id IS NULL AND space_id IS NULL AND account_id IS NOT NULL)),
    UNIQUE(installation_id,id),
    FOREIGN KEY(installation_id,account_id) REFERENCES donna_plugin_accounts(installation_id,id)
) STRICT;
CREATE TABLE IF NOT EXISTS donna_plugin_grants(
    id TEXT PRIMARY KEY NOT NULL,
    installation_id TEXT NOT NULL REFERENCES donna_plugin_installations(id),
    resource_id TEXT NOT NULL,
    bot_id TEXT NOT NULL REFERENCES bots(id),
    context TEXT NOT NULL CHECK(json_valid(context)),
    capability TEXT NOT NULL CHECK(capability IN ('calendar_availability','calendar_read','calendar_create','calendar_update','mailbox_read','mailbox_draft_local','whatsapp_read_chats','whatsapp_read_groups','whatsapp_reply_chats','whatsapp_reply_groups')),
    effect TEXT NOT NULL CHECK(effect IN ('read','native_write_with_card','external_read_with_card','local_draft','external_write_with_card')),
    installation_generation INTEGER NOT NULL CHECK(installation_generation BETWEEN 1 AND 9007199254740991),
    resource_generation INTEGER NOT NULL CHECK(resource_generation BETWEEN 1 AND 9007199254740991),
    resource_revision INTEGER NOT NULL CHECK(resource_revision BETWEEN 1 AND 9007199254740991),
    account_generation INTEGER CHECK(account_generation BETWEEN 1 AND 9007199254740991),
    account_revision INTEGER CHECK(account_revision BETWEEN 1 AND 9007199254740991),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 9007199254740991),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991),
    state TEXT NOT NULL CHECK(state IN ('active','revoked')),
    CHECK((account_generation IS NULL)=(account_revision IS NULL)),
    CHECK((capability IN ('calendar_availability','calendar_read') AND effect IN ('read','external_read_with_card'))
       OR (capability IN ('calendar_create','calendar_update') AND effect='native_write_with_card')
       OR (capability='mailbox_read' AND effect='read')
       OR (capability='mailbox_draft_local' AND effect='local_draft')
       OR (capability IN ('whatsapp_read_chats','whatsapp_read_groups') AND effect='read')
       OR (capability IN ('whatsapp_reply_chats','whatsapp_reply_groups') AND effect='external_write_with_card')),
    FOREIGN KEY(installation_id,resource_id) REFERENCES donna_plugin_resources(installation_id,id)
) STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS donna_plugin_grants_active
    ON donna_plugin_grants(context,resource_id,capability) WHERE state='active';
CREATE INDEX IF NOT EXISTS donna_plugin_grants_bot ON donna_plugin_grants(bot_id,state);
CREATE INDEX IF NOT EXISTS donna_plugin_grants_installation ON donna_plugin_grants(installation_id,state);
-- O Store legado usa foreign_keys=OFF. Estas barreiras locais preservam as
-- relações novas sem alterar a política global de conexões/tabelas alheias.
CREATE TRIGGER IF NOT EXISTS donna_plugin_accounts_parent BEFORE INSERT ON donna_plugin_accounts
WHEN NOT EXISTS(SELECT 1 FROM donna_plugin_installations WHERE id=NEW.installation_id)
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_resources_parent BEFORE INSERT ON donna_plugin_resources
WHEN NOT EXISTS(
    SELECT 1 FROM donna_plugin_installations i WHERE i.id=NEW.installation_id
    AND ((i.builtin='scatty' AND NEW.kind='calendar') OR (i.builtin='email' AND NEW.kind='mailbox' AND NEW.owner='external')
      OR (i.builtin='whatsapp' AND NEW.kind='whatsapp_number' AND NEW.owner='external'))
    AND ((NEW.owner='native' AND NEW.hub_id=i.hub_id)
      OR (NEW.owner='external' AND EXISTS(SELECT 1 FROM donna_plugin_accounts a WHERE a.id=NEW.account_id AND a.installation_id=i.id))))
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_grants_parent BEFORE INSERT ON donna_plugin_grants
WHEN NOT EXISTS(
    SELECT 1 FROM donna_plugin_resources r JOIN donna_plugin_installations i ON i.id=r.installation_id
    JOIN bots b ON b.id=NEW.bot_id AND b.deleted=0
      AND CASE WHEN json_valid(b.config) THEN json_extract(b.config,'$.spaceId')='00000000-0000-4000-8000-000000000001'
        AND json_extract(b.config,'$.id')=b.id AND COALESCE(json_extract(b.config,'$.kind'),'agent')='agent' ELSE 0 END
    LEFT JOIN donna_plugin_accounts a ON a.id=r.account_id AND a.installation_id=i.id
    WHERE r.id=NEW.resource_id AND i.id=NEW.installation_id AND i.state='enabled'
    AND i.generation=NEW.installation_generation AND r.generation=NEW.resource_generation AND r.revision=NEW.resource_revision
    AND ((r.owner='native' AND NEW.account_generation IS NULL AND NEW.account_revision IS NULL)
      OR (r.owner='external' AND a.generation=NEW.account_generation AND a.revision=NEW.account_revision))
    AND json_extract(NEW.context,'$.version')=1
    AND json_extract(NEW.context,'$.hubId')=i.hub_id
    AND json_extract(NEW.context,'$.botId')=NEW.bot_id
    AND json_extract(NEW.context,'$.spaceId')='00000000-0000-4000-8000-000000000001'
    AND json_extract(NEW.context,'$.audience.kind') IN ('principal','additional')
    AND json_extract(NEW.context,'$.teamId') IS NULL
    AND ((r.owner='native' AND r.space_id=json_extract(NEW.context,'$.spaceId')
          AND ((NEW.capability IN ('calendar_availability','calendar_read') AND NEW.effect='read')
            OR (NEW.capability IN ('calendar_create','calendar_update') AND NEW.effect='native_write_with_card')))
      OR (r.owner='external' AND i.builtin='scatty' AND NEW.capability IN ('calendar_availability','calendar_read') AND NEW.effect='external_read_with_card')
      OR (r.owner='external' AND i.builtin='email' AND ((NEW.capability='mailbox_read' AND NEW.effect='read')
         OR (NEW.capability='mailbox_draft_local' AND NEW.effect='local_draft')))
      OR (r.owner='external' AND i.builtin='whatsapp' AND r.kind='whatsapp_number'
          AND ((NEW.capability IN ('whatsapp_read_chats','whatsapp_read_groups') AND NEW.effect='read')
            OR (NEW.capability IN ('whatsapp_reply_chats','whatsapp_reply_groups') AND NEW.effect='external_write_with_card')))))
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_installations_identity BEFORE UPDATE OF id,hub_id,builtin,version ON donna_plugin_installations
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_accounts_identity BEFORE UPDATE OF id,installation_id ON donna_plugin_accounts
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_resources_identity BEFORE UPDATE OF id,installation_id,owner,hub_id,space_id,account_id,kind ON donna_plugin_resources
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_grants_identity BEFORE UPDATE OF id,installation_id,resource_id,bot_id,context,capability,effect,installation_generation,resource_generation,resource_revision,account_generation,account_revision ON donna_plugin_grants
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_grants_no_resurrection BEFORE UPDATE OF state ON donna_plugin_grants
WHEN OLD.state='revoked' AND NEW.state<>'revoked'
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_installations_delete BEFORE DELETE ON donna_plugin_installations
WHEN EXISTS(SELECT 1 FROM donna_plugin_accounts WHERE installation_id=OLD.id)
  OR EXISTS(SELECT 1 FROM donna_plugin_resources WHERE installation_id=OLD.id)
  OR EXISTS(SELECT 1 FROM donna_plugin_grants WHERE installation_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_accounts_delete BEFORE DELETE ON donna_plugin_accounts
WHEN EXISTS(SELECT 1 FROM donna_plugin_resources WHERE account_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_plugin_resources_delete BEFORE DELETE ON donna_plugin_resources
WHEN EXISTS(SELECT 1 FROM donna_plugin_grants WHERE resource_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
