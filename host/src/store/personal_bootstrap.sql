CREATE TABLE IF NOT EXISTS personal_local_birth(singleton INTEGER PRIMARY KEY CHECK(singleton=1), schema INTEGER NOT NULL CHECK(schema=1));
CREATE TRIGGER IF NOT EXISTS personal_birth_fixed BEFORE UPDATE ON personal_local_birth BEGIN SELECT RAISE(ABORT,'local birth immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_birth_keep BEFORE DELETE ON personal_local_birth BEGIN SELECT RAISE(ABORT,'local birth retained'); END;
-- Estado LOCAL: não é migration D1 nem cria prova de posse histórica.
CREATE TABLE IF NOT EXISTS first_owner (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 origin TEXT NOT NULL CHECK(origin IN ('a1','acs4','personal','legacy')),
 ever_claimed INTEGER NOT NULL CHECK(ever_claimed IN (0,1)),
 a1_input TEXT, a1_signature TEXT,
 CHECK(a1_signature IS NULL OR a1_input IS NOT NULL)
);
CREATE TABLE IF NOT EXISTS personal_generation (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), config BLOB NOT NULL,
 generation TEXT NOT NULL UNIQUE, context TEXT NOT NULL,
 computer TEXT NOT NULL UNIQUE, sign_key TEXT NOT NULL UNIQUE, box_key TEXT NOT NULL,
 ever_claimed INTEGER NOT NULL DEFAULT 0 CHECK(ever_claimed IN (0,1)),
 tombstone INTEGER NOT NULL DEFAULT 0 CHECK(tombstone IN (0,1)),
 revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0)
);
CREATE TABLE IF NOT EXISTS personal_device_origins (
 key TEXT PRIMARY KEY, generation TEXT NOT NULL, grant_id TEXT
);
CREATE TRIGGER IF NOT EXISTS first_owner_single_origin BEFORE INSERT ON first_owner
 WHEN EXISTS(SELECT 1 FROM first_owner) BEGIN SELECT RAISE(ABORT,'first owner retained'); END;
CREATE TRIGGER IF NOT EXISTS personal_device_origin_no_replace BEFORE INSERT ON personal_device_origins
 WHEN EXISTS(SELECT 1 FROM personal_device_origins WHERE key=NEW.key)
 BEGIN SELECT RAISE(ABORT,'personal device origin retained'); END;
CREATE TRIGGER IF NOT EXISTS first_owner_immutable BEFORE UPDATE OF singleton,origin,a1_input,a1_signature ON first_owner
 WHEN OLD.singleton IS NOT NEW.singleton OR OLD.origin IS NOT NEW.origin OR (OLD.a1_input IS NOT NULL AND OLD.a1_input IS NOT NEW.a1_input) OR (OLD.a1_signature IS NOT NULL AND OLD.a1_signature IS NOT NEW.a1_signature)
 BEGIN SELECT RAISE(ABORT,'first owner immutable'); END;
CREATE TRIGGER IF NOT EXISTS first_owner_keep BEFORE DELETE ON first_owner
 BEGIN SELECT RAISE(ABORT,'first owner retained'); END;
CREATE TRIGGER IF NOT EXISTS first_owner_no_reopen BEFORE UPDATE OF ever_claimed ON first_owner
 WHEN OLD.ever_claimed=1 AND NEW.ever_claimed<>1 BEGIN SELECT RAISE(ABORT,'first owner closed'); END;
CREATE TRIGGER IF NOT EXISTS personal_generation_guard BEFORE INSERT ON personal_generation
 WHEN EXISTS(SELECT 1 FROM bootstrap_generation) OR EXISTS(SELECT 1 FROM bootstrap_slot) OR EXISTS(SELECT 1 FROM first_owner)
 OR EXISTS(SELECT 1 FROM devices) OR NOT EXISTS(SELECT 1 FROM personal_local_birth WHERE singleton=1 AND schema=1)
 BEGIN SELECT RAISE(ABORT,'personal installation is not virgin'); END;
CREATE TRIGGER IF NOT EXISTS personal_generation_origin AFTER INSERT ON personal_generation BEGIN
 INSERT INTO first_owner(singleton,origin,ever_claimed) VALUES(1,'personal',NEW.ever_claimed);
 END;
CREATE TRIGGER IF NOT EXISTS personal_generation_fixed BEFORE UPDATE OF singleton,config,generation,context,computer,sign_key,box_key ON personal_generation
 BEGIN SELECT RAISE(ABORT,'personal generation immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_generation_keep BEFORE DELETE ON personal_generation
 BEGIN SELECT RAISE(ABORT,'personal generation retained'); END;
CREATE TRIGGER IF NOT EXISTS personal_no_reopen BEFORE UPDATE ON personal_generation
 WHEN NEW.ever_claimed<OLD.ever_claimed OR NEW.tombstone<OLD.tombstone OR NEW.revision<OLD.revision
 BEGIN SELECT RAISE(ABORT,'personal generation closed'); END;
CREATE TRIGGER IF NOT EXISTS personal_claim_marker AFTER UPDATE OF ever_claimed ON personal_generation
 WHEN NEW.ever_claimed=1 BEGIN UPDATE first_owner SET ever_claimed=1 WHERE singleton=1 AND origin='personal'; END;
CREATE TRIGGER IF NOT EXISTS acs4_personal_generation_guard BEFORE INSERT ON bootstrap_generation
 WHEN EXISTS(SELECT 1 FROM personal_generation) OR EXISTS(SELECT 1 FROM first_owner WHERE origin<>'acs4')
 BEGIN SELECT RAISE(ABORT,'first owner profile conflict'); END;
CREATE TRIGGER IF NOT EXISTS acs4_personal_slot_guard BEFORE INSERT ON bootstrap_slot
 WHEN EXISTS(SELECT 1 FROM personal_generation) OR NOT EXISTS(SELECT 1 FROM bootstrap_generation)
 OR EXISTS(SELECT 1 FROM first_owner WHERE origin<>'acs4')
 BEGIN SELECT RAISE(ABORT,'first owner slot conflict'); END;
CREATE TRIGGER IF NOT EXISTS acs4_personal_slot_update_guard BEFORE UPDATE ON bootstrap_slot
 WHEN EXISTS(SELECT 1 FROM personal_generation) OR EXISTS(SELECT 1 FROM first_owner WHERE origin<>'acs4')
 BEGIN SELECT RAISE(ABORT,'first owner slot conflict'); END;
CREATE TRIGGER IF NOT EXISTS personal_device_origin_guard BEFORE INSERT ON personal_device_origins
 WHEN NOT EXISTS(SELECT 1 FROM personal_generation WHERE generation=NEW.generation)
 BEGIN SELECT RAISE(ABORT,'personal device generation mismatch'); END;
CREATE TRIGGER IF NOT EXISTS acs4_first_owner AFTER INSERT ON bootstrap_generation BEGIN
 INSERT INTO first_owner(singleton,origin,ever_claimed) SELECT 1,'acs4',NEW.ever_claimed
 WHERE NOT EXISTS(SELECT 1 FROM first_owner);
 END;
CREATE TRIGGER IF NOT EXISTS acs4_first_owner_claim AFTER UPDATE OF ever_claimed ON bootstrap_generation
 WHEN NEW.ever_claimed=1 BEGIN UPDATE first_owner SET ever_claimed=1 WHERE singleton=1 AND origin='acs4'; END;
CREATE TRIGGER IF NOT EXISTS first_owner_profile_guard BEFORE INSERT ON first_owner
 WHEN (NEW.origin IN ('a1','legacy') AND (EXISTS(SELECT 1 FROM personal_device_origins) OR EXISTS(SELECT 1 FROM personal_generation) OR EXISTS(SELECT 1 FROM bootstrap_generation) OR EXISTS(SELECT 1 FROM bootstrap_slot)))
 OR (NEW.origin='acs4' AND (EXISTS(SELECT 1 FROM personal_generation) OR NOT EXISTS(SELECT 1 FROM bootstrap_generation)))
 OR (NEW.origin='personal' AND (NOT EXISTS(SELECT 1 FROM personal_generation) OR EXISTS(SELECT 1 FROM bootstrap_generation) OR EXISTS(SELECT 1 FROM bootstrap_slot)))
 BEGIN SELECT RAISE(ABORT,'first owner profile conflict'); END;
CREATE TRIGGER IF NOT EXISTS personal_device_origin_insert AFTER INSERT ON devices
 WHEN EXISTS(SELECT 1 FROM personal_generation) BEGIN
 INSERT INTO personal_device_origins(key,generation,grant_id)
 SELECT NEW.key,generation,NEW.grant_id FROM personal_generation
 WHERE NOT EXISTS(SELECT 1 FROM personal_device_origins WHERE key=NEW.key);
 END;
CREATE TRIGGER IF NOT EXISTS personal_device_origin_update AFTER UPDATE ON devices
 WHEN EXISTS(SELECT 1 FROM personal_generation) BEGIN
 INSERT INTO personal_device_origins(key,generation,grant_id)
 SELECT NEW.key,generation,NEW.grant_id FROM personal_generation
 WHERE NOT EXISTS(SELECT 1 FROM personal_device_origins WHERE key=NEW.key);
 END;
CREATE TRIGGER IF NOT EXISTS personal_device_origin_fixed BEFORE UPDATE ON personal_device_origins
 BEGIN SELECT RAISE(ABORT,'personal device origin immutable'); END;
CREATE TRIGGER IF NOT EXISTS personal_device_origin_keep BEFORE DELETE ON personal_device_origins
 BEGIN SELECT RAISE(ABORT,'personal device origin retained'); END;
CREATE TRIGGER IF NOT EXISTS legacy_first_owner_device AFTER INSERT ON devices
 WHEN NOT EXISTS(SELECT 1 FROM first_owner) BEGIN
 INSERT INTO first_owner(singleton,origin,ever_claimed) VALUES(1,'legacy',1);
 END;
