-- Origem D430 separada de PL1/wire v1. Não admite Device nem concede autoridade.
-- Sem FK: apagar devices não apaga a origem. Nenhuma rota grava esta tabela neste corte.
CREATE TABLE IF NOT EXISTS personal_link_origins (
 key TEXT PRIMARY KEY NOT NULL,
 binding BLOB NOT NULL,
 tombstone INTEGER NOT NULL DEFAULT 0 CHECK(tombstone IN (0,1))
);
CREATE TRIGGER IF NOT EXISTS personal_link_origin_no_replace BEFORE INSERT ON personal_link_origins
 WHEN EXISTS(SELECT 1 FROM personal_link_origins WHERE key=NEW.key)
 BEGIN SELECT RAISE(ABORT,'D430 origin retained'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_origin_fixed BEFORE UPDATE OF key,binding ON personal_link_origins
 BEGIN SELECT RAISE(ABORT,'D430 origin immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_origin_keep BEFORE DELETE ON personal_link_origins
 BEGIN SELECT RAISE(ABORT,'D430 origin retained'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_origin_no_reopen BEFORE UPDATE OF tombstone ON personal_link_origins
 WHEN NEW.tombstone<OLD.tombstone
 BEGIN SELECT RAISE(ABORT,'D430 origin closed'); END;
-- Remoção pelo Store legado conserva a cerca; recadastro nunca reabre a origem.
CREATE TRIGGER IF NOT EXISTS personal_link_device_removed AFTER DELETE ON devices
 BEGIN UPDATE personal_link_origins SET tombstone=1 WHERE key=OLD.key; END;
-- Não reconstruir origem antiga/instalação virgem sobre evidência D430.
CREATE TRIGGER IF NOT EXISTS personal_link_blocks_pl1 BEFORE INSERT ON personal_generation
 WHEN EXISTS(SELECT 1 FROM personal_link_origins)
 BEGIN SELECT RAISE(ABORT,'D430 origin excludes PL1 provisioning'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_blocks_acs4 BEFORE INSERT ON bootstrap_generation
 WHEN EXISTS(SELECT 1 FROM personal_link_origins)
 BEGIN SELECT RAISE(ABORT,'D430 origin excludes ACS4 provisioning'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_blocks_acs4_slot BEFORE INSERT ON bootstrap_slot
 WHEN EXISTS(SELECT 1 FROM personal_link_origins)
 BEGIN SELECT RAISE(ABORT,'D430 origin excludes ACS4 provisioning'); END;
-- PR-A D433: instalação D430 isolada, journal de bytes originais, raiz, direitos, runtimes,
-- reservas e eventos. Append-only; só o escritor do procedimento protegido grava (fork/personal_link).
CREATE TABLE IF NOT EXISTS personal_link_installation (
 singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),
 install_id TEXT NOT NULL,
 hub_key TEXT NOT NULL,
 box_key TEXT NOT NULL,
 generation TEXT NOT NULL,
 install_revision INTEGER NOT NULL CHECK(install_revision>0),
 master_key_id TEXT NOT NULL,
 birth BLOB NOT NULL
);
CREATE TRIGGER IF NOT EXISTS personal_link_installation_fixed BEFORE UPDATE ON personal_link_installation
 BEGIN SELECT RAISE(ABORT,'D430 installation immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_installation_keep BEFORE DELETE ON personal_link_installation
 BEGIN SELECT RAISE(ABORT,'D430 installation retained'); END;
CREATE TABLE IF NOT EXISTS personal_link_journal (
 seq INTEGER PRIMARY KEY NOT NULL CHECK(seq>0),
 kind TEXT NOT NULL CHECK(kind IN ('birth','challenge','accept','admission','persisted','complete','management','management-receipt')),
 bytes BLOB NOT NULL
);
CREATE TRIGGER IF NOT EXISTS personal_link_journal_fixed BEFORE UPDATE ON personal_link_journal
 BEGIN SELECT RAISE(ABORT,'D430 journal immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_journal_keep BEFORE DELETE ON personal_link_journal
 BEGIN SELECT RAISE(ABORT,'D430 journal retained'); END;
CREATE TABLE IF NOT EXISTS personal_link_runtimes (
 revision INTEGER PRIMARY KEY NOT NULL CHECK(revision>0),
 runtime_id TEXT NOT NULL UNIQUE,
 boot_id TEXT NOT NULL,
 start_boottime_ms INTEGER NOT NULL,
 time_source TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS personal_link_runtimes_fixed BEFORE UPDATE ON personal_link_runtimes
 BEGIN SELECT RAISE(ABORT,'D430 runtime history immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_runtimes_keep BEFORE DELETE ON personal_link_runtimes
 BEGIN SELECT RAISE(ABORT,'D430 runtime history retained'); END;
CREATE TABLE IF NOT EXISTS personal_link_roots (
 admission_id TEXT PRIMARY KEY NOT NULL,
 root_id TEXT NOT NULL UNIQUE,
 key TEXT NOT NULL UNIQUE,
 challenge_seq INTEGER NOT NULL,
 accept_seq INTEGER NOT NULL,
 admission_seq INTEGER NOT NULL,
 persisted_seq INTEGER,
 complete_seq INTEGER,
 state TEXT NOT NULL CHECK(state IN ('pending','complete','closed'))
);
-- Só pending→complete (com PC/RC) ou, por D456, pending→closed (sem PC/RC) na Tx da nova RA local.
CREATE TRIGGER IF NOT EXISTS personal_link_roots_once BEFORE UPDATE ON personal_link_roots
 WHEN OLD.state<>'pending' OR NEW.admission_id<>OLD.admission_id OR NEW.root_id<>OLD.root_id
  OR NEW.key<>OLD.key OR NEW.challenge_seq<>OLD.challenge_seq OR NEW.accept_seq<>OLD.accept_seq
  OR NEW.admission_seq<>OLD.admission_seq OR OLD.persisted_seq IS NOT NULL OR OLD.complete_seq IS NOT NULL
  OR NOT ((NEW.state='complete' AND NEW.persisted_seq IS NOT NULL AND NEW.complete_seq IS NOT NULL)
   OR (NEW.state='closed' AND NEW.persisted_seq IS NULL AND NEW.complete_seq IS NULL))
 BEGIN SELECT RAISE(ABORT,'D430 root admission immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_roots_keep BEFORE DELETE ON personal_link_roots
 BEGIN SELECT RAISE(ABORT,'D430 root admission retained'); END;
CREATE TABLE IF NOT EXISTS personal_link_rights (
 key TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 policy BLOB NOT NULL,
 grant_seq INTEGER NOT NULL,
 consent_seq INTEGER,
 PRIMARY KEY(key,revision)
);
CREATE TRIGGER IF NOT EXISTS personal_link_rights_fixed BEFORE UPDATE ON personal_link_rights
 BEGIN SELECT RAISE(ABORT,'D430 rights history immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_rights_keep BEFORE DELETE ON personal_link_rights
 BEGIN SELECT RAISE(ABORT,'D430 rights history retained'); END;
CREATE TABLE IF NOT EXISTS personal_link_reservations (
 admission_id TEXT PRIMARY KEY NOT NULL,
 hub_key TEXT NOT NULL,
 key TEXT NOT NULL,
 op TEXT NOT NULL,
 runtime_id TEXT NOT NULL,
 anchor_boot_id TEXT,
 anchor_boottime_ms INTEGER,
 UNIQUE(hub_key,key,op)
);
CREATE TRIGGER IF NOT EXISTS personal_link_reservations_anchor BEFORE UPDATE ON personal_link_reservations
 WHEN OLD.anchor_boottime_ms IS NOT NULL OR NEW.anchor_boottime_ms IS NULL OR NEW.anchor_boot_id IS NULL
  OR NEW.admission_id<>OLD.admission_id OR NEW.hub_key<>OLD.hub_key OR NEW.key<>OLD.key OR NEW.op<>OLD.op
  OR NEW.runtime_id<>OLD.runtime_id
 BEGIN SELECT RAISE(ABORT,'D430 reservation immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_reservations_keep BEFORE DELETE ON personal_link_reservations
 BEGIN SELECT RAISE(ABORT,'D430 reservation retained'); END;
CREATE TABLE IF NOT EXISTS personal_link_events (
 seq INTEGER PRIMARY KEY AUTOINCREMENT,
 kind TEXT NOT NULL,
 subject TEXT NOT NULL,
 runtime_id TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS personal_link_events_fixed BEFORE UPDATE ON personal_link_events
 BEGIN SELECT RAISE(ABORT,'D430 events immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_events_keep BEFORE DELETE ON personal_link_events
 BEGIN SELECT RAISE(ABORT,'D430 events retained'); END;
-- Instalação D430 nascida (mesmo sem raiz) também exclui PL1/ACS4.
CREATE TRIGGER IF NOT EXISTS personal_link_installation_blocks_pl1 BEFORE INSERT ON personal_generation
 WHEN EXISTS(SELECT 1 FROM personal_link_installation)
 BEGIN SELECT RAISE(ABORT,'D430 installation excludes PL1 provisioning'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_installation_blocks_acs4 BEFORE INSERT ON bootstrap_generation
 WHEN EXISTS(SELECT 1 FROM personal_link_installation)
 BEGIN SELECT RAISE(ABORT,'D430 installation excludes ACS4 provisioning'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_installation_blocks_slot BEFORE INSERT ON bootstrap_slot
 WHEN EXISTS(SELECT 1 FROM personal_link_installation)
 BEGIN SELECT RAISE(ABORT,'D430 installation excludes ACS4 provisioning'); END;
CREATE TRIGGER IF NOT EXISTS personal_link_installation_blocks_devices BEFORE INSERT ON devices
 WHEN EXISTS(SELECT 1 FROM personal_link_installation)
 BEGIN SELECT RAISE(ABORT,'D430 installation excludes legacy devices'); END;
