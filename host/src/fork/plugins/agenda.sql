-- F2d, agenda nativa 1: somente DDL vazio e caminho de leitura; sem escrita, semente ou texto privado.
-- Título/local/descrição ficam para payload-ref com custódia real; aqui só tempo, ocupação e identidade.
CREATE TABLE IF NOT EXISTS donna_scatty_events(
    id TEXT PRIMARY KEY NOT NULL,
    installation_id TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    resource_generation INTEGER NOT NULL CHECK(resource_generation BETWEEN 1 AND 9007199254740991),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991),
    timing TEXT NOT NULL CHECK(timing IN ('timed','all_day')),
    starts_at INTEGER,
    ends_at INTEGER,
    time_zone TEXT,
    start_date TEXT,
    end_date TEXT,
    busy INTEGER NOT NULL CHECK(busy IN (0,1)),
    CHECK((timing='timed' AND starts_at IS NOT NULL AND ends_at IS NOT NULL AND ends_at>starts_at
           AND time_zone IS NOT NULL AND length(time_zone) BETWEEN 1 AND 64 AND start_date IS NULL AND end_date IS NULL)
       OR (timing='all_day' AND starts_at IS NULL AND ends_at IS NULL AND time_zone IS NULL
           AND start_date IS NOT NULL AND end_date IS NOT NULL
           AND start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
           AND end_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND end_date>start_date)),
    FOREIGN KEY(installation_id,resource_id) REFERENCES donna_plugin_resources(installation_id,id)
) STRICT;
CREATE INDEX IF NOT EXISTS donna_scatty_events_timed
    ON donna_scatty_events(resource_id,resource_generation,starts_at) WHERE timing='timed';
CREATE INDEX IF NOT EXISTS donna_scatty_events_all_day
    ON donna_scatty_events(resource_id,resource_generation,start_date) WHERE timing='all_day';
-- Store legado com foreign_keys=OFF: barreiras locais, como no registro F1b.
CREATE TRIGGER IF NOT EXISTS donna_scatty_events_parent BEFORE INSERT ON donna_scatty_events
WHEN NOT EXISTS(
    SELECT 1 FROM donna_plugin_resources r JOIN donna_plugin_installations i ON i.id=r.installation_id
    WHERE r.id=NEW.resource_id AND i.id=NEW.installation_id AND i.builtin='scatty'
    AND r.owner='native' AND r.kind='calendar' AND r.generation=NEW.resource_generation)
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_scatty_events_identity BEFORE UPDATE OF id,installation_id,resource_id,resource_generation ON donna_scatty_events
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
CREATE TRIGGER IF NOT EXISTS donna_scatty_events_resource_delete BEFORE DELETE ON donna_plugin_resources
WHEN EXISTS(SELECT 1 FROM donna_scatty_events WHERE resource_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'plugin_constraint'); END;
