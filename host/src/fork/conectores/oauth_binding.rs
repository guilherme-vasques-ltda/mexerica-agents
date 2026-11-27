//! Identidade e CAS do registro canônico cifrado. Código próprio da Mexerica.
//! Ordem: `connector_lock` -> cofre (sem DB) -> DB. Nenhum await sob essas guardas.
use super::oauth_runtime::RuntimeIntent;
use crate::{
    LockExt,
    api::devices::Caller,
    hub::Hub,
    market::{self, Connector},
    store::{DeviceSource, Scope, Store},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use zeroize::Zeroizing;

/// Hook da migração do Store (schema.rs -> `fork::migrate`), sem novo registro de tokens.
/// A época sobrevive a reopen e rollback; uma recriação idêntica recebe outra época.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let columns = db
        .prepare("PRAGMA table_info(devices)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let tx = db.unchecked_transaction()?;
    if !columns.iter().any(|c| c == "donna_oauth_epoch") {
        tx.execute("ALTER TABLE devices ADD COLUMN donna_oauth_epoch TEXT NOT NULL DEFAULT ''", [])?;
    }
    // Upgrade da definição anterior ANTES do backfill, inclusive com scopes corruptos.
    // DDL e época confirmam juntos; trocar a definição não rotaciona linhas existentes.
    tx.execute_batch("DROP TRIGGER IF EXISTS donna_oauth_device_update;")?;
    tx.execute("UPDATE devices SET donna_oauth_epoch=lower(hex(randomblob(16))) WHERE donna_oauth_epoch=''", [])?;
    tx.execute_batch("CREATE TRIGGER IF NOT EXISTS donna_oauth_device_insert AFTER INSERT ON devices BEGIN
        UPDATE devices SET donna_oauth_epoch=lower(hex(randomblob(16))) WHERE key=NEW.key; END;
        CREATE TRIGGER donna_oauth_device_update AFTER UPDATE ON devices
        WHEN OLD.key IS NOT NEW.key OR OLD.source IS NOT NEW.source OR OLD.grant_id IS NOT NEW.grant_id
          OR CASE
            WHEN json_valid(OLD.scopes)=0 THEN OLD.scopes IS NOT NEW.scopes
            WHEN json_valid(NEW.scopes)=0 THEN OLD.scopes IS NOT NEW.scopes
            WHEN json_type(OLD.scopes)<>'array' THEN OLD.scopes IS NOT NEW.scopes
            WHEN json_type(NEW.scopes)<>'array' THEN OLD.scopes IS NOT NEW.scopes
            ELSE (SELECT json_group_array(value) FROM (SELECT DISTINCT value FROM json_each(OLD.scopes) ORDER BY value))
              IS NOT (SELECT json_group_array(value) FROM (SELECT DISTINCT value FROM json_each(NEW.scopes) ORDER BY value))
          END
        BEGIN UPDATE devices SET donna_oauth_epoch=lower(hex(randomblob(16))) WHERE key=NEW.key; END;")?;
    tx.commit()?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Binding {
    instance: String,
    generation: String,
    revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registration_issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grant: Option<Grant>,
    #[serde(default, skip_serializing_if = "super::oauth_token_scopes::Estado::vazio")]
    scope_validation: super::oauth_token_scopes::Estado,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Grant {
    resource: String,
    issuer: String,
    redirect: String,
    #[serde(default)]
    scopes: Vec<String>,
}
impl Binding {
    pub(crate) fn new() -> Self {
        Self {
            instance: uuid::Uuid::new_v4().to_string(),
            generation: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            profile: None,
            registration_issuer: None,
            grant: None,
            scope_validation: super::oauth_token_scopes::Estado::default(),
        }
    }
    /// Identificador opaco da conta, para recibos: não é o `id`, o nome nem segredo.
    pub(crate) fn instance(&self) -> &str {
        &self.instance
    }
    /// Resource capturado na concessão, sem derivar de ID ou URL atual.
    pub(crate) fn resource(&self) -> Option<&str> {
        self.grant.as_ref().map(|grant| grant.resource.as_str())
    }
    pub(crate) fn reconnect(&mut self) {
        self.generation = uuid::Uuid::new_v4().to_string();
        self.grant = None;
        self.scope_validation = super::oauth_token_scopes::Estado::default();
    }
    pub(crate) fn registration_issuer(&self) -> Option<&str> {
        self.registration_issuer.as_deref()
    }
    /// Só o Hub, após DCR aceito, associa a emissão ao cadastro gravado na mesma CAS.
    pub(crate) fn register(&mut self, issuer: &str) {
        self.registration_issuer = Some(issuer.into());
    }
    pub(crate) fn clear_registration(&mut self) {
        self.registration_issuer = None;
    }
    pub(crate) fn scopes_retained(&self) -> bool {
        self.scope_validation.retido()
    }
    pub(crate) fn check_scopes(&self) -> Result<()> {
        self.scope_validation.conferir()
    }
    pub(crate) fn validate_login_scopes(&mut self, response: &serde_json::Value) -> Result<()> {
        let pedido = self.grant.as_ref().map_or(&[][..], |grant| grant.scopes.as_slice());
        self.scope_validation = super::oauth_token_scopes::Estado::login(pedido, response)?;
        Ok(())
    }
    pub(crate) fn validate_refresh_scopes(&self, response: &serde_json::Value) -> Result<()> {
        self.scope_validation.refresh(response)
    }
    pub(crate) fn retain_scopes(&mut self) -> Result<()> {
        self.revision = self.revision.checked_add(1).ok_or_else(|| anyhow!("Revisão esgotada"))?;
        self.scope_validation.reter();
        Ok(())
    }
    pub(crate) fn grant(&mut self, resource: &str, issuer: &str, redirect: &str, scopes: &[String]) {
        self.grant = Some(Grant {
            resource: resource.into(),
            issuer: issuer.into(),
            redirect: redirect.into(),
            scopes: scopes.to_vec(),
        });
    }
}

/// Autoridade capturada pelo Hub antes de filas/unlock. Não deriva de argumentos JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Local,
    Device { key: String, source: DeviceSource, grant: Option<String>, created: i64, epoch: String, lease: Option<i64> },
}
impl Origin {
    pub(crate) fn capture(hub: &Hub, caller: &Caller) -> Result<Self> {
        let origin = match caller {
            Caller::Local => Self::Local,
            Caller::Device { key, scopes, .. } if scopes.contains(&Scope::Control) => {
                let db = hub.store.db.locked();
                db.query_row(
                    "SELECT source,grant_id,created_at,donna_oauth_epoch,lease_until FROM devices WHERE key=?1",
                    [key],
                    |r| {
                        let source: String = r.get(0)?;
                        Ok(Self::Device {
                            key: key.clone(),
                            source: serde_json::from_value(source.clone().into()).unwrap_or(DeviceSource::Account),
                            grant: r.get(1)?,
                            created: r.get(2)?,
                            epoch: r.get(3)?,
                            lease: if source == "account" { r.get(4)? } else { None },
                        })
                    },
                )
                .optional()?
                .ok_or_else(|| anyhow!("Autoridade do login revogada"))?
            }
            _ => anyhow::bail!("Autoridade insuficiente para login do conector"),
        };
        origin.with_db(hub, |_| Ok(()))?;
        Ok(origin)
    }
    pub(crate) fn matches(&self, caller: &Caller) -> bool {
        match (self, caller) {
            (Self::Local, Caller::Local) => true,
            (Self::Device { key, .. }, Caller::Device { key: actual, scopes, .. }) => {
                key == actual && scopes.contains(&Scope::Control)
            }
            _ => false,
        }
    }
    fn validate(&self, hub: &Hub, db: &Connection) -> Result<()> {
        if let Self::Device { key, source, grant, created, epoch, lease } = self {
            let current = db
                .query_row(
                    "SELECT source,grant_id,created_at,scopes,lease_until,donna_oauth_epoch FROM devices WHERE key=?1",
                    [key],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, Option<i64>>(4)?,
                            r.get::<_, String>(5)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| anyhow!("Autoridade do login revogada"))?;
            let scopes: Vec<Scope> = serde_json::from_str(&current.3)?;
            ensure!(
                serde_json::from_value::<DeviceSource>(current.0.into())? == *source
                    && current.1 == *grant
                    && current.2 == *created
                    && current.5 == *epoch
                    && !epoch.is_empty()
                    && scopes.contains(&Scope::Control),
                "Autoridade original do login mudou"
            );
            if *source == DeviceSource::Account {
                ensure!(
                    hub.cloud_synced.load(Ordering::Acquire)
                        && lease.is_some_and(|until| until > crate::store::now_ms())
                        && current.4.is_some_and(|until| until > crate::store::now_ms()),
                    "Autoridade do login expirada"
                );
            }
        }
        Ok(())
    }
    pub(crate) fn with_db<T>(&self, hub: &Hub, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let mut db = hub.store.db.locked();
        let tx = db.transaction()?;
        self.validate(hub, &tx)?;
        let value = f(&tx)?;
        self.validate(hub, &tx)?;
        tx.commit()?;
        Ok(value)
    }
}

pub(crate) fn same_target(a: &Connector, b: &Connector) -> bool {
    a.id == b.id && a.oauth_binding.is_some() && a.oauth_binding == b.oauth_binding && same_config(a, b)
}
fn same_config(a: &Connector, b: &Connector) -> bool {
    a.url == b.url
        && a.command == b.command
        && a.args == b.args
        && a.env == b.env
        && a.headers == b.headers
        && oauth_config(a.oauth.as_ref()) == oauth_config(b.oauth.as_ref())
        && a.oauth_binding.as_ref().and_then(Binding::registration_issuer)
            == b.oauth_binding.as_ref().and_then(Binding::registration_issuer)
        && a.oauth_binding.as_ref().and_then(|b| b.grant.as_ref())
            == b.oauth_binding.as_ref().and_then(|b| b.grant.as_ref())
}

/// Exclui somente `access/refresh/expires_at`; cadastro/endpoint/redirect continuam configuração.
fn oauth_config(o: Option<&market::oauth::OAuth>) -> Option<(&str, Option<&str>, &[String], &str)> {
    o.map(|o| (o.client_id.as_str(), o.client_secret.as_deref(), o.redirect_uris.as_slice(), o.token_endpoint.as_str()))
}
/// Chamado somente sob `connector_lock`, sem reentrar o loader público.
pub(crate) fn initialize(all: &mut [Connector]) -> bool {
    let mut changed = false;
    for c in all {
        if c.oauth_binding.is_none() {
            c.oauth_binding = Some(Binding::new());
            changed = true;
        }
    }
    changed
}
pub(crate) fn lifecycle(before: &[Connector], after: &mut [Connector]) -> Result<()> {
    initialize(after);
    for c in after {
        if let Some(old) = before.iter().find(|old| old.id == c.id)
            && !same_config(old, c)
        {
            let binding = c.oauth_binding.as_mut().ok_or_else(|| anyhow!("Vínculo ausente"))?;
            binding.revision = binding.revision.checked_add(1).ok_or_else(|| anyhow!("Revisão esgotada"))?;
            binding.reconnect();
            binding.clear_registration();
            // Remoto que exige OAuth continua oferecendo login, sem tokens/cadastro anteriores.
            c.oauth =
                (c.command.is_none() && (old.oauth.is_some() || c.oauth.is_some())).then(market::oauth::OAuth::default);
        }
    }
    Ok(())
}

fn same_generation(a: &Connector, b: &Connector) -> bool {
    match (&a.oauth_binding, &b.oauth_binding) {
        (Some(a), Some(b)) => {
            a.instance == b.instance
                && a.generation == b.generation
                && a.revision == b.revision
                && a.profile == b.profile
        }
        _ => false,
    }
}

/// Só SQL na transação do ciphertext. Cache antigo continua disponível para saneamento.
fn invalidate_runtime(db: &Connection, before: &[Connector], after: &[Connector]) -> Result<()> {
    for old in before {
        if !after.iter().any(|c| c.id == old.id && same_generation(old, c)) {
            db.execute("DELETE FROM donna_guard_contexts WHERE connector=?1", [&old.id])?;
            db.execute("DELETE FROM donna_guard_setup WHERE connector=?1", [&old.id])?;
        }
    }
    Ok(())
}

/// Chamado sob `connector_lock`; sem Store/Cloud/cifra enquanto a transação está aberta.
/// Sela o cofre; `extra` roda NA MESMA transação que sela o cofre (ex.: o recibo da conta): a ação e o
/// recibo nascem ou falham juntos.
pub(crate) fn persist_locked(
    store: &Store,
    before: &[Connector],
    after: &[Connector],
    extra: impl FnOnce(&Connection) -> Result<()>,
) -> Result<()> {
    let sealed = market::vault::seal(store, "connectors", &serde_json::to_string(after)?)?;
    // D368: num pedido do app pareado, grava sob a autoridade original reconferida na transação.
    crate::fork::donna_fixa::pedido::gravar(store, |tx| {
        market::vault::write_sealed(tx, "connectors", &sealed)?;
        invalidate_runtime(tx, before, after)?;
        extra(tx)
    })
}

/// Snapshot canônico posterior à migração, antes da rede e do fingerprint remoto.
#[cfg(test)]
pub(crate) fn snapshot(store: &Store, id: &str) -> Result<Connector> {
    market::connectors(store)?.into_iter().find(|c| c.id == id).ok_or_else(|| anyhow!("Conector removido"))
}

/// Emissão pelo ator atual, serializada com reconexão/configuração; nunca pelo proxy.
pub(crate) fn issue_context(
    store: &Store,
    expected: &Connector,
    bot: &str,
    lane: &str,
    run: Option<&str>,
    binding: &str,
) -> Result<crate::guard_hub::mcp::contexto::Context> {
    let _guard = store.connector_lock.locked();
    let (all, _) = market::load_connectors(store)?;
    ensure!(all.iter().any(|c| same_target(c, expected)), "Snapshot antigo do ator: o vínculo do conector mudou");
    crate::guard_hub::mcp::contexto::Context::issue_bound(store, bot, &expected.id, lane, run, Some(binding))
}

/// Setup legado usa a mesma cerca de emissão, sem await ou DB pré-segurado.
pub(crate) fn issue_setup<'a>(
    store: &'a Store,
    expected: &Connector,
) -> Result<crate::guard_hub::mcp::setup::Setup<'a>> {
    let _guard = store.connector_lock.locked();
    let (all, _) = market::load_connectors(store)?;
    ensure!(all.iter().any(|c| same_target(c, expected)), "Snapshot antigo de setup: o vínculo do conector mudou");
    crate::guard_hub::mcp::setup::Setup::issue(store, &expected.id)
}

/// Testemunho transitório emitido pelo Hub antes do primeiro await. Sem `Debug`/`Serialize`.
/// O raw pode ser legado plaintext; nunca sai desta estrutura nem vai para pending/DTO/log.
pub(crate) struct StartIntent {
    connector: String,
    origin: Origin,
    raw: Option<Zeroizing<String>>,
}

fn changed_start() -> anyhow::Error {
    anyhow!("A configuração mudou. Inicie uma nova intenção de conexão.")
}

fn read_start_raw(db: &Connection) -> Result<Option<Zeroizing<String>>> {
    Ok(db
        .query_row("SELECT v FROM kv WHERE k='connectors'", [], |r| r.get::<_, String>(0))
        .optional()?
        .map(Zeroizing::new))
}

impl StartIntent {
    pub(crate) fn capture(hub: &Hub, caller: &Caller, id: &str) -> Result<Self> {
        let origin = Origin::capture(hub, caller)?;
        let _guard = hub.store.connector_lock.locked();
        // A guarda valida `Origin` antes de ler o raw, e novamente antes de terminar.
        let raw = origin.with_db(hub, read_start_raw)?;
        Ok(Self { connector: id.into(), origin, raw })
    }

    fn require_raw(&self, db: &Connection) -> Result<()> {
        let actual = read_start_raw(db)?;
        ensure!(actual.as_ref().map(|v| v.as_str()) == self.raw.as_ref().map(|v| v.as_str()), "{}", changed_start());
        Ok(())
    }

    /// Captura canônica, migração e concessão nova são uma única gravação por CAS.
    /// `connector_lock` só cobre operações síncronas; chave/open/selagem fora de DB.
    pub(crate) fn begin(self, hub: &Hub) -> Result<(Origin, Connector)> {
        let store = &hub.store;
        let _guard = store.connector_lock.locked();
        // Recusar slot/`Origin` antigos ANTES de open. Isso não autoriza a escrita adiante.
        self.origin.with_db(hub, |db| self.require_raw(db))?;
        let mut all: Vec<Connector> = match &self.raw {
            None => Vec::new(),
            Some(raw) => {
                let (plain, _) = market::vault::decode_unmigrated(store, "connectors", raw)?;
                let plain = Zeroizing::new(plain);
                serde_json::from_str(&plain).map_err(|_| changed_start())?
            }
        };
        initialize(&mut all);
        // None->Some de OUTROS legados é migração, não revogação de contexto.
        let before = all.clone();
        let c = all.iter_mut().find(|c| c.id == self.connector).ok_or_else(changed_start)?;
        let url = c
            .url
            .as_deref()
            .filter(|_| c.command.is_none())
            .ok_or_else(|| anyhow!("{} runs on this computer; it has nothing to sign in to", c.name))?;
        // M2-A: só parse, sem IO, antes de reconnect/geração/limpeza de tokens.
        super::oauth_urls::conector(url)?;
        let original = c.clone();
        let binding = c.oauth_binding.as_mut().ok_or_else(changed_start)?;
        binding.reconnect();
        if let Some(oauth) = &mut c.oauth {
            oauth.access_token.clear();
            oauth.refresh_token = None;
            oauth.expires_at = None;
        }
        if !same_config(c, &original) {
            let binding = c.oauth_binding.as_mut().ok_or_else(changed_start)?;
            binding.revision = binding.revision.checked_add(1).ok_or_else(|| anyhow!("Revisão esgotada"))?;
        }
        let saved = c.clone();
        let plain = Zeroizing::new(serde_json::to_string(&all)?);
        let sealed = Zeroizing::new(market::vault::seal(store, "connectors", &plain)?);
        #[cfg(test)]
        tests::before_start_write(hub);
        self.origin.with_db(hub, |db| {
            // CAS do raw e `Origin` revalidados na MESMA tx que grava/limpa/invalida.
            self.require_raw(db)?;
            market::vault::write_sealed(db, "connectors", &sealed)?;
            invalidate_runtime(db, &before, &all)
        })?;
        if saved.oauth_binding != original.oauth_binding {
            market::oauth::invalidate(&original);
        }
        Ok((self.origin, saved))
    }
}

/// Compara e grava o par de tokens + binding juntos; erro preserva o ciphertext anterior.
/// `f` não faz Store/Cloud/I/O. Para callback, o caller original é checado na mesma transação.
pub(crate) fn change<T>(
    store: &Store,
    expected: &Connector,
    authority: Option<(&Hub, &Origin)>,
    valid: impl Fn() -> Result<()>,
    f: impl FnOnce(&mut Connector) -> Result<T>,
) -> Result<(Connector, T)> {
    change_guarded(store, expected, authority, None, valid, f)
}

/// Proxy conserva sua própria autoridade Context/Setup, sem Origin de login fabricada.
pub(crate) fn change_runtime<T>(
    hub: &Hub,
    expected: &Connector,
    intent: &RuntimeIntent,
    f: impl FnOnce(&mut Connector) -> Result<T>,
) -> Result<(Connector, T)> {
    intent.validate_connector(expected)?;
    intent.validate_live(hub)?;
    let saved = change_guarded(&hub.store, expected, None, Some(intent), || Ok(()), f)?;
    intent.validate_live(hub)?;
    Ok(saved)
}

fn change_guarded<T>(
    store: &Store,
    expected: &Connector,
    authority: Option<(&Hub, &Origin)>,
    runtime: Option<&RuntimeIntent>,
    valid: impl Fn() -> Result<()>,
    f: impl FnOnce(&mut Connector) -> Result<T>,
) -> Result<(Connector, T)> {
    let _guard = store.connector_lock.locked();
    if let Some(intent) = runtime {
        let db = store.db.locked();
        let _ = intent.validate_db(&db)?;
    }
    let (mut all, _) = market::load_connectors(store)?;
    let c = all
        .iter_mut()
        .find(|c| {
            runtime.map_or_else(|| same_target(c, expected), |intent| intent.matches_target(c, expected))
                && c.oauth == expected.oauth
        })
        .ok_or_else(|| anyhow!("Login antigo: o vínculo do conector mudou"))?;
    valid()?;
    let value = f(c)?;
    if !same_config(c, expected) {
        let binding = c.oauth_binding.as_mut().ok_or_else(|| anyhow!("Vínculo ausente"))?;
        binding.revision = binding.revision.checked_add(1).ok_or_else(|| anyhow!("Revisão esgotada"))?;
    }
    let saved = c.clone();
    let sealed = market::vault::seal(store, "connectors", &serde_json::to_string(&all)?)?;
    let write = |db: &Connection| {
        valid()?;
        let deadline = runtime.map(|intent| intent.validate_db(db)).transpose()?.flatten();
        market::vault::write_sealed(db, "connectors", &sealed)?;
        invalidate_runtime(db, std::slice::from_ref(expected), std::slice::from_ref(&saved))?;
        // sign_out pode apagar SUA própria linha nesta tx; não adotar outra linha nem
        // exigir que a revogada reapareça. A entrada já validou id/tupla/bot/seleção.
        if let Some(deadline) = deadline {
            ensure!(deadline > crate::store::now_ms(), "Contexto de setup indisponível");
        }
        valid()
    };
    if let Some((hub, origin)) = authority {
        origin.with_db(hub, write)?;
    } else {
        let mut db = store.db.locked();
        let tx = db.transaction()?;
        write(&tx)?;
        tx.commit()?;
    }
    if saved.oauth_binding != expected.oauth_binding {
        market::oauth::invalidate(expected);
    }
    Ok((saved, value))
}

/// Valida sob a guarda, inclusive antes de entregar URL/segredo ou publicar pending.
pub(crate) fn check<T>(
    store: &Store,
    expected: &Connector,
    authority: Option<(&Hub, &Origin)>,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    check_guarded(store, expected, authority, None, f)
}

pub(crate) fn check_runtime<T>(
    hub: &Hub,
    expected: &Connector,
    intent: &RuntimeIntent,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    intent.validate_connector(expected)?;
    intent.validate_live(hub)?;
    let value = check_guarded(&hub.store, expected, None, Some(intent), f)?;
    intent.validate_live(hub)?;
    Ok(value)
}

fn check_guarded<T>(
    store: &Store,
    expected: &Connector,
    authority: Option<(&Hub, &Origin)>,
    runtime: Option<&RuntimeIntent>,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _guard = store.connector_lock.locked();
    if let Some(intent) = runtime {
        let db = store.db.locked();
        let _ = intent.validate_db(&db)?;
    }
    let (all, _) = market::load_connectors(store)?;
    ensure!(
        all.iter()
            .any(|c| runtime.map_or_else(|| same_target(c, expected), |intent| intent.matches_target(c, expected))
                && c.oauth == expected.oauth),
        "Login antigo: o vínculo do conector mudou"
    );
    if let Some((hub, origin)) = authority {
        origin.with_db(hub, |_| f())
    } else if let Some(intent) = runtime {
        let db = store.db.locked();
        let _ = intent.validate_db(&db)?;
        let value = f()?;
        let _ = intent.validate_db(&db)?;
        Ok(value)
    } else {
        f()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::Device;
    pub(crate) fn hub() -> std::sync::Arc<Hub> {
        let dir = std::env::temp_dir().join(format!("oauth-binding-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let identity = crate::remote::identity::Identity::load_or_create(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture".into(),
            identity,
            "fixture".into(),
            0,
        )
    }
    fn device() -> Device {
        Device {
            key: "same-key".into(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: 1,
            last_seen_at: None,
        }
    }
    fn origin(hub: &Hub) -> Origin {
        Origin::capture(hub, &Caller::Device { key: "same-key".into(), scopes: vec![Scope::Control], autoridade: None })
            .unwrap()
    }
    #[test]
    fn identical_upsert_and_lease_renewal_keep_epoch_but_aba_does_not() {
        let hub = hub();
        let mut d = device();
        hub.store.put_device(&d).unwrap();
        let original = origin(&hub);
        hub.store.put_device(&d).unwrap();
        assert_eq!(origin(&hub), original);
        d.name = "changed".into();
        d.platform = "changed".into();
        hub.store.put_device(&d).unwrap();
        hub.store.touch_device(&d.key).unwrap();
        hub.store.set_lease(&d.key, 123).unwrap();
        assert_eq!(origin(&hub), original);
        hub.store.remove_device(&d.key).unwrap();
        hub.store.put_device(&device()).unwrap();
        assert_ne!(origin(&hub), original);
        assert!(original.with_db(&hub, |_| Ok(())).is_err());
    }
    #[test]
    fn real_authority_changes_rotate_epoch_and_rollback_restores_it() {
        let hub = hub();
        hub.store.put_device(&device()).unwrap();
        let original = origin(&hub);
        for sql in [
            "UPDATE devices SET key='other-key'",
            "UPDATE devices SET source='account'",
            "UPDATE devices SET grant_id='other-grant'",
            "UPDATE devices SET scopes='[\"screen\"]'",
        ] {
            let db = hub.store.db.locked();
            let tx = db.unchecked_transaction().unwrap();
            tx.execute(sql, []).unwrap();
            let epoch: String = tx.query_row("SELECT donna_oauth_epoch FROM devices", [], |r| r.get(0)).unwrap();
            let Origin::Device { epoch: before, .. } = &original else { panic!("device") };
            assert_ne!(&epoch, before);
            tx.rollback().unwrap();
            drop(db);
            assert_eq!(origin(&hub), original);
        }
        let mut d = device();
        d.scopes = vec![Scope::Control, Scope::Screen];
        hub.store.put_device(&d).unwrap();
        let changed = origin(&hub);
        assert_ne!(changed, original);
        d.scopes.reverse();
        hub.store.put_device(&d).unwrap();
        assert_eq!(origin(&hub), changed, "ordem de scopes não é nova autoridade");
    }
    #[test]
    fn old_database_migrates_and_epoch_survives_reopen() {
        let path = std::env::temp_dir().join(format!("oauth-schema-{}.sqlite", uuid::Uuid::new_v4()));
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch("CREATE TABLE devices(key TEXT PRIMARY KEY,name TEXT NOT NULL,platform TEXT NOT NULL,source TEXT NOT NULL,grant_id TEXT,scopes TEXT NOT NULL,lease_until INTEGER,created_at INTEGER NOT NULL,last_seen_at INTEGER);
                INSERT INTO devices VALUES('legacy','name','platform','local',NULL,'[\"control\"]',NULL,1,NULL)").unwrap();
            migrate(&db).unwrap();
        }
        let epoch = {
            let db = Connection::open(&path).unwrap();
            db.query_row("SELECT donna_oauth_epoch FROM devices", [], |r| r.get::<_, String>(0)).unwrap()
        };
        {
            let db = Connection::open(&path).unwrap();
            migrate(&db).unwrap();
            assert_eq!(
                db.query_row("SELECT donna_oauth_epoch FROM devices", [], |r| r.get::<_, String>(0)).unwrap(),
                epoch
            );
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn binding_and_rotating_token_pair_are_one_failed_write() {
        let hub = hub();
        let c:Connector=serde_json::from_value(serde_json::json!({"id":"notes","name":"notes","url":"https://fixture.invalid/mcp","oauth":{"accessToken":"old","refreshToken":"old-refresh"}})).unwrap();
        market::save_connectors(&hub.store, &[c]).unwrap();
        let c = snapshot(&hub.store, "notes").unwrap();
        let before = hub.store.kv_read("connectors").unwrap();
        hub.store.db.locked().execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE commit_parent(id INTEGER PRIMARY KEY); CREATE TABLE commit_failure(id INTEGER REFERENCES commit_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_credentials AFTER UPDATE ON kv WHEN NEW.k='connectors' BEGIN INSERT INTO commit_failure VALUES(1); END").unwrap();
        assert!(
            change(
                &hub.store,
                &c,
                Some((&hub, &Origin::Local)),
                || Ok(()),
                |c| {
                    let o = c.oauth.as_mut().unwrap();
                    o.access_token = "new".into();
                    o.refresh_token = Some("new-refresh".into());
                    c.oauth_binding.as_mut().unwrap().reconnect();
                    Ok(())
                }
            )
            .is_err()
        );
        assert_eq!(hub.store.kv_read("connectors").unwrap(), before);
        assert_eq!(snapshot(&hub.store, "notes").unwrap(), c);
    }
    #[test]
    fn original_authority_cannot_be_recaptured_after_unlock_or_before_private_reply() {
        let hub = hub();
        hub.store.put_device(&device()).unwrap();
        let original = origin(&hub);
        hub.store.remove_device("same-key").unwrap();
        hub.store.put_device(&device()).unwrap();
        assert!(original.with_db(&hub, |_| Ok("private")).is_err());
        assert!(origin(&hub).with_db(&hub, |_| Ok(())).is_ok());
    }
    #[test]
    fn original_account_lease_expires_even_when_current_lease_was_renewed() {
        let hub = hub();
        let mut d = device();
        d.source = DeviceSource::Account;
        d.grant_id = Some("grant".into());
        d.lease_until = Some(crate::store::now_ms() + 60000);
        hub.store.put_device(&d).unwrap();
        hub.cloud_synced.store(true, Ordering::Release);
        let mut original = origin(&hub);
        let Origin::Device { lease, .. } = &mut original else { panic!("device") };
        *lease = Some(crate::store::now_ms() - 1);
        hub.store.set_lease(&d.key, crate::store::now_ms() + 120_000).unwrap();
        assert!(original.with_db(&hub, |_| Ok("private")).is_err());
        assert!(origin(&hub).with_db(&hub, |_| Ok(())).is_ok());
    }

    #[test]
    fn revoked_origin_cannot_migrate_even_plaintext_legacy_before_unlock() {
        let hub = hub();
        hub.store.put_device(&device()).unwrap();
        let raw =
            r#"[{"id":"notes","name":"notes","url":"https://fixture.invalid","oauth":{"accessToken":"legacy-token"}}]"#;
        hub.store.kv_set("connectors", raw).unwrap();
        let intent = StartIntent::capture(
            &hub,
            &Caller::Device { key: "same-key".into(), scopes: vec![Scope::Control], autoridade: None },
            "notes",
        )
        .unwrap();
        hub.store.remove_device("same-key").unwrap();
        assert!(intent.begin(&hub).is_err());
        assert!(hub.store.kv_read("connectors").unwrap().as_deref().eq(&Some(raw)));
    }

    #[test]
    fn deadline_is_revalidated_inside_write_and_rollback_keeps_whole_record() {
        let hub = hub();
        let c: Connector =
            serde_json::from_value(serde_json::json!({"id":"notes","name":"notes","url":"https://fixture.invalid"}))
                .unwrap();
        market::save_connectors(&hub.store, &[c]).unwrap();
        let c = snapshot(&hub.store, "notes").unwrap();
        let before = hub.store.kv_read("connectors").unwrap();
        let checks = std::cell::Cell::new(0);
        let result = change(
            &hub.store,
            &c,
            Some((&hub, &Origin::Local)),
            || {
                checks.set(checks.get() + 1);
                ensure!(checks.get() < 3, "fixture deadline expired after write");
                Ok(())
            },
            |c| {
                c.oauth_binding.as_mut().unwrap().reconnect();
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(checks.get(), 3);
        assert_eq!(hub.store.kv_read("connectors").unwrap(), before);
    }
    #[test]
    fn runtime_revocation_is_atomic_isolated_and_retains_old_sanitizing_cache() {
        use crate::guard_hub::mcp::{contexto::Context, runtime, setup::Setup};
        let hub = hub();
        let connector = |id: &str| {
            serde_json::from_value(serde_json::json!({"id":id,"name":id,"url":"https://fixture.invalid"})).unwrap()
        };
        market::save_connectors(&hub.store, &[connector("notes"), connector("other")]).unwrap();
        let cfg = serde_json::from_value(
            serde_json::json!({"id":"selected","name":"selected","connectors":["notes","other"]}),
        )
        .unwrap();
        hub.store.save_bot(&cfg).unwrap();
        let context = Context::issue(&hub.store, "selected", "notes", "selected", None).unwrap();
        let other = Context::issue(&hub.store, "selected", "other", "selected", None).unwrap();
        let setup = Setup::issue(&hub.store, "notes").unwrap();
        let other_setup = Setup::issue(&hub.store, "other").unwrap();
        let namespace = hub.store.guard_runtime_namespace().unwrap();
        runtime::remember(&namespace, "notes", &context.id, "bridge", &[Zeroizing::new("old-private-secret".into())]);
        let c = snapshot(&hub.store, "notes").unwrap();
        let before = hub.store.kv_read("connectors").unwrap();
        hub.store.db.locked().execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE revoke_parent(id INTEGER PRIMARY KEY); CREATE TABLE revoke_failure(id INTEGER REFERENCES revoke_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_revoke AFTER UPDATE ON kv WHEN NEW.k='connectors' BEGIN INSERT INTO revoke_failure VALUES(1); END").unwrap();
        let reconnect = |c: &mut Connector| {
            c.oauth_binding.as_mut().unwrap().reconnect();
            Ok(())
        };
        assert!(change(&hub.store, &c, None, || Ok(()), reconnect).is_err());
        assert_eq!(hub.store.kv_read("connectors").unwrap(), before);
        assert!(Context::resolve(&hub.store, &context.id, "notes").is_ok());
        assert!(setup_exists(&hub.store, &setup.id));
        hub.store.db.locked().execute_batch("DROP TRIGGER fail_revoke").unwrap();
        change(&hub.store, &c, None, || Ok(()), reconnect).unwrap();
        assert!(Context::resolve(&hub.store, &context.id, "notes").is_err());
        assert!(!setup_exists(&hub.store, &setup.id));
        assert!(Context::resolve(&hub.store, &other.id, "other").is_ok());
        assert!(setup_exists(&hub.store, &other_setup.id));
        assert!(runtime::snapshot(&namespace, "notes", &context.id, Some("bridge")).is_some());
        let new = Context::issue(&hub.store, "selected", "notes", "selected", None).unwrap();
        assert_ne!(new.id, context.id);
        market::update_connectors(&hub.store, |all| {
            all.iter_mut().find(|c| c.id == "notes").unwrap().headers.insert("x-config".into(), "changed".into());
            Ok(())
        })
        .unwrap();
        assert!(Context::resolve(&hub.store, &new.id, "notes").is_err());
        let newest = Context::issue(&hub.store, "selected", "notes", "selected", None).unwrap();
        market::remove_connector(&hub.store, "notes").unwrap();
        assert!(Context::resolve(&hub.store, &newest.id, "notes").is_err());
        assert!(Context::resolve(&hub.store, &other.id, "other").is_ok());
    }
    fn setup_exists(store: &Store, id: &str) -> bool {
        store
            .db
            .locked()
            .query_row("SELECT EXISTS(SELECT 1 FROM donna_guard_setup WHERE id=?1)", [id], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn canonical_legacy_migration_preserves_existing_context_and_tokens() {
        use crate::guard_hub::mcp::contexto::Context;
        let hub = hub();
        let cfg = serde_json::from_value(serde_json::json!({"id":"selected","name":"selected","connectors":["notes"]}))
            .unwrap();
        hub.store.save_bot(&cfg).unwrap();
        market::vault::write(&hub.store,"connectors",r#"[{"id":"notes","name":"notes","url":"https://fixture.invalid","oauth":{"accessToken":"legacy-token","refreshToken":"legacy-refresh"}}]"#).unwrap();
        let context = Context::issue(&hub.store, "selected", "notes", "selected", None).unwrap();
        let c = snapshot(&hub.store, "notes").unwrap();
        assert!(c.oauth_binding.is_some());
        assert_eq!(c.oauth.unwrap().access_token, "legacy-token");
        assert_eq!(Context::issue(&hub.store, "selected", "notes", "selected", None).unwrap().id, context.id);
    }

    #[test]
    fn actor_cannot_issue_from_old_snapshot_but_rotation_allows_current_identity() {
        let hub = hub();
        let c:Connector=serde_json::from_value(serde_json::json!({"id":"notes","name":"notes","url":"https://fixture.invalid","oauth":{"accessToken":"old"}})).unwrap();
        market::save_connectors(&hub.store, &[c]).unwrap();
        let c = snapshot(&hub.store, "notes").unwrap();
        let cfg = serde_json::from_value(serde_json::json!({"id":"selected","name":"selected","connectors":["notes"]}))
            .unwrap();
        hub.store.save_bot(&cfg).unwrap();
        let binding = crate::fork::sessoes::bindings::fixture_binding(&hub.store, "selected");
        let original = issue_context(&hub.store, &c, "selected", "selected", None, binding.id.as_str()).unwrap();
        change(
            &hub.store,
            &c,
            None,
            || Ok(()),
            |c| {
                let o = c.oauth.as_mut().unwrap();
                o.access_token = "rotated".into();
                o.expires_at = Some(123_456_789);
                Ok(())
            },
        )
        .unwrap();
        let definition = crate::guard_hub::mcp::definition(
            &hub.store,
            &c,
            std::path::Path::new("fixture-host"),
            1,
            "selected",
            "selected",
            None,
            binding.id.as_str(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            definition["args"].as_array().unwrap().last().unwrap(),
            &serde_json::json!(original.id),
            "refresh/expiry não invalidam definition"
        );
        let current = snapshot(&hub.store, "notes").unwrap();
        change(
            &hub.store,
            &current,
            None,
            || Ok(()),
            |c| {
                c.oauth_binding.as_mut().unwrap().reconnect();
                Ok(())
            },
        )
        .unwrap();
        assert!(
            crate::guard_hub::mcp::definition(
                &hub.store,
                &c,
                std::path::Path::new("fixture-host"),
                1,
                "selected",
                "selected",
                None,
                binding.id.as_str(),
            )
            .is_err()
        );
        let current = snapshot(&hub.store, "notes").unwrap();
        assert_ne!(
            issue_context(&hub.store, &current, "selected", "selected", None, binding.id.as_str()).unwrap().id,
            original.id
        );
    }

    #[test]
    fn setup_emission_rejects_old_snapshot_and_accepts_current_without_rotating_tokens() {
        let hub = hub();
        let c:Connector=serde_json::from_value(serde_json::json!({"id":"notes","name":"notes","url":"https://fixture.invalid","oauth":{"clientId":"original","accessToken":"old","refreshToken":"r"}})).unwrap();
        market::save_connectors(&hub.store, &[c]).unwrap();
        let c = snapshot(&hub.store, "notes").unwrap();
        let setup = issue_setup(&hub.store, &c).unwrap();
        change(
            &hub.store,
            &c,
            None,
            || Ok(()),
            |c| {
                let o = c.oauth.as_mut().unwrap();
                o.access_token = "rotated".into();
                o.refresh_token = Some("r2".into());
                o.expires_at = Some(42);
                Ok(())
            },
        )
        .unwrap();
        let next = issue_setup(&hub.store, &c).unwrap();
        assert_ne!(setup.id, next.id);
        assert!(setup_exists(&hub.store, &setup.id));
        let current = snapshot(&hub.store, "notes").unwrap();
        change(
            &hub.store,
            &current,
            None,
            || Ok(()),
            |c| {
                c.oauth.as_mut().unwrap().client_id = "different-client".into();
                Ok(())
            },
        )
        .unwrap();
        assert!(!setup_exists(&hub.store, &setup.id));
        assert!(issue_setup(&hub.store, &c).is_err());
        let current = snapshot(&hub.store, "notes").unwrap();
        assert!(issue_setup(&hub.store, &current).is_ok());
    }
    struct StartFence {
        reached: std::sync::Arc<std::sync::Barrier>,
        proceed: std::sync::Arc<std::sync::Barrier>,
    }
    static START_WRITE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<usize, StartFence>>> =
        std::sync::LazyLock::new(Default::default);
    pub(crate) fn before_start_write(hub: &Hub) {
        let fence = START_WRITE.locked().remove(&(std::ptr::from_ref(hub) as usize));
        if let Some(fence) = fence {
            fence.reached.wait();
            fence.proceed.wait();
        }
    }

    #[test]
    fn start_raw_cas_is_rechecked_after_open_inside_write_transaction() {
        let hub = hub();
        let c:Connector=serde_json::from_value(serde_json::json!({"id":"notes","name":"notes","url":"https://fixture.invalid","oauth":{"accessToken":"old","refreshToken":"old-refresh"}})).unwrap();
        market::save_connectors(&hub.store, &[c]).unwrap();
        let intent = StartIntent::capture(&hub, &Caller::Local, "notes").unwrap();
        let mut next = market::connectors(&hub.store).unwrap();
        next[0].oauth.as_mut().unwrap().access_token = "rotated-after-open".into();
        let plain = Zeroizing::new(serde_json::to_string(&next).unwrap());
        let next_raw =
            std::sync::Arc::new(Zeroizing::new(market::vault::seal(&hub.store, "connectors", &plain).unwrap()));
        let reached = std::sync::Arc::new(std::sync::Barrier::new(2));
        let proceed = std::sync::Arc::new(std::sync::Barrier::new(2));
        START_WRITE.locked().insert(
            std::ptr::from_ref(hub.as_ref()) as usize,
            StartFence { reached: reached.clone(), proceed: proceed.clone() },
        );
        let writer = {
            let hub = hub.clone();
            let next_raw = next_raw.clone();
            std::thread::spawn(move || {
                reached.wait();
                hub.store.kv_set("connectors", &next_raw).unwrap();
                proceed.wait();
            })
        };
        let result = intent.begin(&hub);
        writer.join().unwrap();
        assert!(result.is_err());
        assert!(
            hub.store.kv_read("connectors").unwrap().as_deref() == Some(next_raw.as_str()),
            "CAS final não pode sobrescrever rotação posterior ao open"
        );
    }

    #[test]
    fn start_failed_commit_restores_raw_tokens_binding_context_and_setup() {
        use crate::guard_hub::mcp::contexto::Context;
        let hub = hub();
        let c:Connector=serde_json::from_value(serde_json::json!({"id":"notes","name":"notes","url":"https://fixture.invalid","oauth":{"accessToken":"old","refreshToken":"old-refresh"}})).unwrap();
        market::save_connectors(&hub.store, &[c]).unwrap();
        let c = snapshot(&hub.store, "notes").unwrap();
        let cfg = serde_json::from_value(serde_json::json!({"id":"selected","name":"selected","connectors":["notes"]}))
            .unwrap();
        hub.store.save_bot(&cfg).unwrap();
        let binding = crate::fork::sessoes::bindings::fixture_binding(&hub.store, "selected");
        let context = issue_context(&hub.store, &c, "selected", "selected", None, binding.id.as_str()).unwrap();
        let setup = issue_setup(&hub.store, &c).unwrap();
        let intent = StartIntent::capture(&hub, &Caller::Local, "notes").unwrap();
        let raw = hub.store.kv_read("connectors").unwrap();
        hub.store.db.locked().execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE start_parent(id INTEGER PRIMARY KEY); CREATE TABLE start_failure(id INTEGER REFERENCES start_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_start AFTER UPDATE ON kv WHEN NEW.k='connectors' BEGIN INSERT INTO start_failure VALUES(1); END").unwrap();
        assert!(intent.begin(&hub).is_err());
        assert!(hub.store.kv_read("connectors").unwrap().eq(&raw));
        assert_eq!(snapshot(&hub.store, "notes").unwrap(), c);
        assert!(Context::resolve(&hub.store, &context.id, "notes").is_ok());
        assert!(setup_exists(&hub.store, &setup.id));
    }

    #[test]
    fn start_malformed_plaintext_and_bad_cipher_preserve_slot_without_exposing_raw() {
        let hub = hub();
        let wrong_aad = market::vault::seal(&hub.store, "different-slot", "[]").unwrap();
        for raw in [r#""private-start-canary""#.to_string(), "vault:v1:AA==".into(), wrong_aad] {
            hub.store.kv_set("connectors", &raw).unwrap();
            let intent = StartIntent::capture(&hub, &Caller::Local, "notes").unwrap();
            let result = intent.begin(&hub);
            assert!(result.is_err());
            let error = result.err().unwrap().to_string();
            assert!(!error.contains("private-start-canary"));
            assert!(hub.store.kv_read("connectors").unwrap().as_deref().eq(&Some(raw.as_str())));
        }
    }

    #[test]
    fn start_missing_slot_cannot_adopt_newly_inserted_connector() {
        let hub = hub();
        let intent = StartIntent::capture(&hub, &Caller::Local, "notes").unwrap();
        let c: Connector =
            serde_json::from_value(serde_json::json!({"id":"notes","name":"notes","url":"https://fixture.invalid"}))
                .unwrap();
        market::save_connectors(&hub.store, &[c]).unwrap();
        let raw = hub.store.kv_read("connectors").unwrap();
        assert!(intent.begin(&hub).is_err());
        assert!(hub.store.kv_read("connectors").unwrap().eq(&raw));
        assert!(StartIntent::capture(&hub, &Caller::Local, "notes").unwrap().begin(&hub).is_ok());
    }
}

#[cfg(test)]
mod scope_shape_tests {
    use super::*;

    fn epoch(db: &Connection) -> String {
        db.query_row("SELECT donna_oauth_epoch FROM devices WHERE key='shape-device'", [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn malformed_and_nonarray_scopes_cannot_revive_original_authority_on_recovery() {
        for malformed in ["{", "\"control\"", "{\"x\":\"control\"}", "null"] {
            let hub = tests::hub();
            let device = crate::store::Device {
                key: "shape-device".into(),
                name: "Shape".into(),
                platform: "fixture".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes: vec![Scope::Control],
                lease_until: None,
                created_at: 1,
                last_seen_at: None,
            };
            hub.store.put_device(&device).unwrap();
            let caller = Caller::Device { key: device.key.clone(), scopes: device.scopes.clone(), autoridade: None };
            let original = Origin::capture(&hub, &caller).unwrap();
            let original_epoch = epoch(&hub.store.db.locked());
            {
                let db = hub.store.db.locked();
                db.execute("UPDATE devices SET scopes=?1 WHERE key='shape-device'", [malformed]).unwrap();
                assert_ne!(epoch(&db), original_epoch, "array → inválido muda época");
                let invalid_epoch = epoch(&db);
                db.execute("UPDATE devices SET scopes=?1 WHERE key='shape-device'", [malformed]).unwrap();
                assert_eq!(epoch(&db), invalid_epoch, "raw inválido idêntico não é mudança");
            }
            assert!(Origin::capture(&hub, &caller).is_err(), "leitor continua fail-closed");
            assert!(original.with_db(&hub, |_| Ok(())).is_err());
            hub.store.put_device(&device).unwrap();
            assert_ne!(epoch(&hub.store.db.locked()), original_epoch, "shape → array não ressuscita época antiga");
            assert!(original.with_db(&hub, |_| Ok(())).is_err());
            assert!(Origin::capture(&hub, &caller).is_ok());
        }
    }

    #[test]
    fn corrupt_scope_and_epoch_rollback_together() {
        let hub = tests::hub();
        hub.store.db.locked().execute("INSERT INTO devices(key,name,platform,source,scopes,created_at) VALUES('shape-device','Shape','fixture','local','[\"control\"]',1)", []).unwrap();
        let before = epoch(&hub.store.db.locked());
        {
            let db = hub.store.db.locked();
            let tx = db.unchecked_transaction().unwrap();
            tx.execute("UPDATE devices SET scopes='{' WHERE key='shape-device'", []).unwrap();
            assert_ne!(epoch(&tx), before);
            tx.rollback().unwrap();
        }
        assert_eq!(epoch(&hub.store.db.locked()), before);
        let caller = Caller::Device { key: "shape-device".into(), scopes: vec![Scope::Control], autoridade: None };
        assert!(Origin::capture(&hub, &caller).is_ok());
    }

    #[test]
    fn old_trigger_definition_upgrades_without_rotating_epoch_and_survives_reopen() {
        let path = std::env::temp_dir().join(format!("oauth-trigger-upgrade-{}.sqlite", uuid::Uuid::new_v4()));
        let before = {
            let db = Connection::open(&path).unwrap();
            db.execute_batch("CREATE TABLE devices(key TEXT PRIMARY KEY,name TEXT NOT NULL,platform TEXT NOT NULL,source TEXT NOT NULL,grant_id TEXT,scopes TEXT NOT NULL,lease_until INTEGER,created_at INTEGER NOT NULL,last_seen_at INTEGER);
                INSERT INTO devices VALUES('shape-device','Shape','fixture','local',NULL,'[\"control\"]',NULL,1,NULL)").unwrap();
            migrate(&db).unwrap();
            let before = epoch(&db);
            db.execute("INSERT INTO devices(key,name,platform,source,scopes,created_at) VALUES('backfill-malformed','Legacy','fixture','local','{',1)", []).unwrap();
            db.execute("UPDATE devices SET donna_oauth_epoch='' WHERE key='backfill-malformed'", []).unwrap();
            db.execute_batch("DROP TRIGGER donna_oauth_device_update;
                CREATE TRIGGER donna_oauth_device_update AFTER UPDATE ON devices
                WHEN OLD.key IS NOT NEW.key OR OLD.source IS NOT NEW.source OR OLD.grant_id IS NOT NEW.grant_id
                  OR (SELECT json_group_array(value) FROM (SELECT DISTINCT value FROM json_each(OLD.scopes) ORDER BY value))
                     IS NOT (SELECT json_group_array(value) FROM (SELECT DISTINCT value FROM json_each(NEW.scopes) ORDER BY value))
                BEGIN UPDATE devices SET donna_oauth_epoch=lower(hex(randomblob(16))) WHERE key=NEW.key; END;").unwrap();
            before
        };
        let recovered = {
            let db = Connection::open(&path).unwrap();
            migrate(&db).unwrap();
            assert_eq!(epoch(&db), before, "upgrade de DDL não revoga autoridade sem mudança");
            let (backfilled, scopes): (String, String) = db
                .query_row("SELECT donna_oauth_epoch,scopes FROM devices WHERE key='backfill-malformed'", [], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .unwrap();
            assert!(!backfilled.is_empty(), "trigger antigo sai ANTES do UPDATE de backfill em JSON malformado");
            assert_eq!(scopes, "{", "migração não normaliza nem concede autoridade ao registro inválido");
            db.execute("UPDATE devices SET scopes='{' WHERE key='shape-device'", []).unwrap();
            let invalid = epoch(&db);
            assert_ne!(invalid, before);
            db.execute("UPDATE devices SET scopes='[\"control\"]' WHERE key='shape-device'", []).unwrap();
            assert_ne!(epoch(&db), invalid);
            epoch(&db)
        };
        {
            let db = Connection::open(&path).unwrap();
            migrate(&db).unwrap();
            assert_eq!(epoch(&db), recovered, "reopen mantém época gravada");
            db.execute("UPDATE devices SET scopes='[\"screen\",\"control\",\"control\"]' WHERE key='shape-device'", [])
                .unwrap();
            let with_screen = epoch(&db);
            db.execute("UPDATE devices SET scopes='[\"control\",\"screen\"]', name='renamed', lease_until=123 WHERE key='shape-device'", []).unwrap();
            assert_eq!(epoch(&db), with_screen, "ordem/conjunto e metadados/lease continuam estáveis");
        }
        let _ = std::fs::remove_file(path);
    }
}
