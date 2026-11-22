//! Estado local do executor: identidade, vínculo com o Hub, autorização por local, diário e caixa de saída.
//! Tudo vive numa pasta própria (0700), separada do banco do Hub; nada aqui lê o ambiente além do que o chamador passa.
use crate::remote::crypto;
use anyhow::{Context, Result, anyhow, bail, ensure};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubLink {
    pub url: String,
    pub sign_pub: String,
    pub label: String,
    pub computer_id: String,
    #[serde(default)]
    pub binding_id: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Authorized {
    pub desktop: Option<Grant>,
}
/// A liberação vale para ESTA pasta (dispositivo e inode no momento da confirmação), não para o nome do caminho:
/// outra pasta comum posta no mesmo lugar não herda a liberação.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub path: String,
    pub at: i64,
    #[serde(default)]
    pub dev: u64,
    #[serde(default)]
    pub ino: u64,
}

pub struct Home(pub PathBuf);

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}
fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("criar {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut f = options.open(&tmp)?;
    let written = f
        .write_all(bytes)
        .and_then(|()| f.sync_all())
        .and_then(|()| fs::rename(&tmp, path))
        .and_then(|()| path.parent().map_or(Ok(()), |dir| fs::File::open(dir)?.sync_all()));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(written?)
}

impl Home {
    pub fn open(path: &Path) -> Result<Self> {
        let marker = path.join(".mexerica-executor");
        let foreign = !marker.exists() && fs::read_dir(path).is_ok_and(|mut d| d.next().is_some());
        ensure!(
            !foreign,
            "a pasta de estado {} já tem outros arquivos; use uma pasta nova ou vazia (--home)",
            path.display()
        );
        ensure!(!is_link(path), "a pasta de estado não pode ser um link");
        private_dir(path)?;
        let home = Self(path.to_path_buf());
        let _lock = home.lock()?;
        if !marker.exists() {
            private_write(&marker, b"estado do executor da Mexerica\n")?;
        }
        for epoch in ["state.epoch", "binding.epoch"] {
            let file = home.file(epoch);
            if fs::symlink_metadata(&file).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
                private_write(&file, crypto::b64(&crypto::random::<16>()).as_bytes())?;
            }
        }
        ensure!(fs::symlink_metadata(&marker).is_ok_and(|m| m.is_file()), "marcador do estado inválido");
        for sub in ["outbox", "running"] {
            let dir = path.join(sub);
            ensure!(!is_link(&dir), "{} não pode ser um link", dir.display());
            private_dir(&dir)?;
        }
        Ok(home)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub fn key(&self, create: bool) -> Result<SigningKey> {
        let path = self.file("identity.key");
        match fs::read_to_string(&path) {
            Ok(text) => Ok(SigningKey::from_bytes(&crypto::unb64_n::<32>(text.trim())?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                let seed = crypto::random::<32>();
                private_write(&path, crypto::b64(&seed).as_bytes())?;
                Ok(SigningKey::from_bytes(&seed))
            }
            Err(e) => Err(anyhow!("identidade do executor ausente ({e}); cadastre o computador antes")),
        }
    }
    pub fn link(&self) -> Result<HubLink> {
        let link: HubLink = serde_json::from_slice(
            &fs::read(self.file("hub.json")).context("este computador ainda não foi cadastrado no Hub")?,
        )
        .context("hub.json inválido")?;
        ensure_plain_url(&link.url).context("hub.json com endereço inseguro")?;
        Ok(link)
    }
    pub fn save_link(&self, link: &HubLink) -> Result<()> {
        private_write(&self.file("hub.json"), &serde_json::to_vec_pretty(link)?)
    }
    pub fn authorized(&self) -> Result<Authorized> {
        match fs::read(self.file("auth.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).context("auth.json inválido")?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Authorized::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save_authorized(&self, auth: &Authorized) -> Result<()> {
        private_write(&self.file("auth.json"), &serde_json::to_vec_pretty(auth)?)
    }
    /// Gerações separadas: a de autorização muda ao liberar ou retirar um local; a de vínculo muda só no cadastro
    /// e na revogação. Um estado antigo, sem o arquivo, entra com geração vazia e a primeira operação a cria.
    fn read_epoch(&self, name: &str) -> Result<String> {
        match fs::read_to_string(self.file(name)) {
            Ok(text) => Ok(text.trim().to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e.into()),
        }
    }
    fn write_epoch(&self, name: &str) -> Result<String> {
        let epoch = crypto::b64(&crypto::random::<16>());
        private_write(&self.file(name), epoch.as_bytes())?;
        Ok(epoch)
    }
    pub fn auth_epoch(&self) -> Result<String> {
        self.read_epoch("state.epoch")
    }
    pub fn bump_auth_epoch(&self) -> Result<String> {
        self.write_epoch("state.epoch")
    }
    pub fn binding_epoch(&self) -> Result<String> {
        self.read_epoch("binding.epoch")
    }
    pub fn bump_binding_epoch(&self) -> Result<String> {
        self.write_epoch("binding.epoch")
    }

    pub fn running(&self, job: &str) -> PathBuf {
        self.0.join("running").join(safe_id(job))
    }
    /// Guarda a tentativa ANTES de gravar: pedido, lease e se o alvo já existia. É o que permite conferir, depois de uma
    /// queda, o resultado da mesma tentativa em vez de repetir ou deixar o resultado só como "desconhecido".
    /// Fase "planned": nada no disco foi tocado ainda. `preexisting`: o alvo já existia antes da tentativa.
    pub fn mark_running(&self, job: &str, lease: &str, kind: &str, spec: &Value, preexisting: bool) -> Result<()> {
        let record =
            json!({"jobId":job,"leaseToken":lease,"kind":kind,"spec":spec,"preexisting":preexisting,"phase":"planned"});
        private_write(&self.running(job), record.to_string().as_bytes())
    }
    /// Registra, antes de qualquer byte, a identidade do arquivo e da subpasta que ESTA tentativa criou.
    pub fn mark_created(&self, job: &str, dev: u64, ino: u64, folder: Option<(u64, u64)>) -> Result<()> {
        let mut record = self.running_record(job).ok_or_else(|| anyhow!("tentativa {job} não registrada"))?;
        record["created"] = json!([dev, ino]);
        if let Some((fdev, fino)) = folder {
            record["folderId"] = json!([fdev, fino]);
        }
        private_write(&self.running(job), record.to_string().as_bytes())
    }
    /// Fase "writing": a partir daqui a tentativa pode ter criado a pasta ou o arquivo; só o disco diz o que houve.
    pub fn mark_writing(&self, job: &str) -> Result<()> {
        let mut record = self.running_record(job).ok_or_else(|| anyhow!("tentativa {job} não registrada"))?;
        record["phase"] = json!("writing");
        record["writingAt"] = json!(crate::store::now_ms());
        private_write(&self.running(job), record.to_string().as_bytes())
    }
    pub fn running_record(&self, job: &str) -> Option<Value> {
        serde_json::from_slice(&fs::read(self.running(job)).ok()?).ok()
    }
    pub fn clear_running(&self, job: &str) {
        let _ = fs::remove_file(self.running(job));
    }
    /// Pedidos iniciados cujo resultado não chegou a ser guardado: (jobId, leaseToken).
    pub fn pending_running(&self) -> Vec<(String, String)> {
        fs::read_dir(self.0.join("running"))
            .map(|d| {
                d.flatten()
                    .filter_map(|e| {
                        (!e.file_name().to_string_lossy().contains('.')).then_some(())?;
                        let v: Value = serde_json::from_slice(&fs::read(e.path()).ok()?).ok()?;
                        Some((v["jobId"].as_str()?.to_owned(), v["leaseToken"].as_str()?.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn save_outbox(&self, job: &str, result: &Value) -> Result<()> {
        private_write(&self.0.join("outbox").join(safe_id(job)), &serde_json::to_vec(result)?)
    }
    pub fn outbox(&self) -> Vec<(String, Value)> {
        fs::read_dir(self.0.join("outbox"))
            .map(|d| {
                d.flatten()
                    .filter_map(|e| {
                        let id = e.file_name().into_string().ok()?;
                        // Nomes de pedido não têm ponto: o resto é arquivo temporário de uma escrita interrompida.
                        (!id.contains('.')).then_some(())?;
                        Some((id, serde_json::from_slice(&fs::read(e.path()).ok()?).ok()?))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn clear_outbox(&self, job: &str) {
        let _ = fs::remove_file(self.0.join("outbox").join(safe_id(job)));
    }

    /// Diário de auditoria encadeado (seq, prev, hash): o Hub confere continuidade ao recebê-lo.
    pub fn audit(&self, job: &str, event: &str, detail: &Value) -> Result<u64> {
        let rows = self.audit_rows()?;
        let (seq, prev) = rows.last().map_or((1, String::new()), |r| {
            (r["seq"].as_u64().unwrap_or(0) + 1, r["hash"].as_str().unwrap_or_default().to_owned())
        });
        let body = json!({"seq":seq,"prev":prev,"at":crate::store::now_ms(),"jobId":job,"event":event,"detail":detail});
        let hash = crate::computers::xlsx::hex(&crypto::sha256(&[prev.as_bytes(), body.to_string().as_bytes()]));
        let mut record = body;
        record["hash"] = json!(hash);
        let path = self.file("journal.jsonl");
        // Uma queda no meio de uma linha não pode colar o registro novo no pedaço velho.
        let torn = fs::read(&path).is_ok_and(|b| b.last().is_some_and(|c| *c != b'\n'));
        let mut f = fs::OpenOptions::new().create(true).append(true).open(path)?;
        if torn {
            writeln!(f)?;
        }
        writeln!(f, "{record}")?;
        f.sync_all()?;
        Ok(seq)
    }
    pub fn audit_rows(&self) -> Result<Vec<Value>> {
        match fs::read(self.file("journal.jsonl")) {
            Ok(bytes) => Ok(bytes.split(|b| *b == b'\n').filter_map(|l| serde_json::from_slice(l).ok()).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(e.into()),
        }
    }
    pub fn audit_acked(&self) -> u64 {
        fs::read_to_string(self.file("audit.ack")).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0)
    }
    pub fn set_audit_acked(&self, seq: u64) -> Result<()> {
        private_write(&self.file("audit.ack"), seq.to_string().as_bytes())
    }
    /// Diário, pendências e confirmações pertencem a UM vínculo com o Hub: um novo cadastro começa limpo e nunca
    /// herda pedidos, caminhos ou hashes do vínculo anterior.
    /// Remove só arquivos comuns que são nossos; qualquer erro ou item estranho interrompe (nunca segue link).
    pub fn reset_binding(&self) -> Result<()> {
        let gone = |path: PathBuf| match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() => fs::remove_file(&path).with_context(|| format!("apagar {}", path.display())),
            Ok(_) => bail!("{} não é um arquivo comum; limpeza interrompida", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        };
        for name in ["audit.ack", "journal.jsonl"] {
            gone(self.file(name))?;
        }
        for sub in ["outbox", "running"] {
            for entry in fs::read_dir(self.0.join(sub))? {
                gone(entry?.path())?;
            }
        }
        Ok(())
    }
    /// Trinco exclusivo entre `unauthorize` e a criação do arquivo; solta ao sair de escopo.
    pub fn lock(&self) -> Result<fs::File> {
        let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.file("auth.lock"))?;
        file.lock().context("trinco da liberação")?;
        Ok(file)
    }
    /// Um só cadastro por pasta de estado, da escolha da chave até o fim do pareamento: dois `enroll` não podem
    /// trocar a chave um do outro enquanto um deles espera a rede.
    pub fn enroll_lock(&self) -> Result<fs::File> {
        let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.file("enroll.lock"))?;
        file.try_lock().map_err(|_| anyhow!("já existe um cadastro em andamento nesta pasta de estado"))?;
        Ok(file)
    }
    /// Um só `run` por pasta de estado: dois processos não podem reconciliar nem gravar a mesma tentativa.
    pub fn run_lock(&self) -> Result<fs::File> {
        let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.file("run.lock"))?;
        file.try_lock().map_err(|_| anyhow!("já existe um executor rodando com esta pasta de estado"))?;
        Ok(file)
    }
    pub fn has_link_file(&self) -> bool {
        fs::symlink_metadata(self.file("hub.json")).is_ok()
    }
    pub fn wipe_identity(&self) -> Result<()> {
        for name in ["identity.key", "hub.json", "auth.json"] {
            let path = self.file(name);
            if fs::symlink_metadata(&path).is_ok() {
                fs::remove_file(&path).with_context(|| format!("apagar {}", path.display()))?;
            }
        }
        self.reset_binding()
    }
}

fn safe_id(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).take(64).collect()
}

pub fn ensure_plain_url(url: &str) -> Result<()> {
    let (secure, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        _ => bail!("o endereço do Hub precisa começar com https://"),
    };
    let authority = rest.split('/').next().unwrap_or_default();
    ensure!(
        !authority.is_empty() && !rest.contains(['?', '#']) && !authority.contains('@'),
        "endereço do Hub inválido (sem usuário, senha, parâmetros nem âncora)"
    );
    let host = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|b| b.is_ascii_digit()))
        .map_or(authority, |(h, _)| h);
    ensure!(
        secure || matches!(host, "localhost" | "127.0.0.1" | "[::1]"),
        "o Hub precisa usar https:// (http:// só vale em localhost)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn a_new_binding_never_inherits_pending_jobs_or_the_journal_of_the_previous_one() {
        let dir = std::env::temp_dir().join(format!("mexerica-state-{}", uuid::Uuid::new_v4()));
        let home = Home::open(&dir).unwrap();
        home.mark_running("job1", "lease", "fs.write", &json!({}), false).unwrap();
        home.save_outbox("job2", &json!({"outcome":"done"})).unwrap();
        home.audit("job1", "intent", &json!({})).unwrap();
        home.set_audit_acked(1).unwrap();
        home.wipe_identity().unwrap();
        assert_eq!((home.pending_running().len(), home.outbox().len()), (0, 0));
        assert_eq!(home.audit_rows().unwrap().len(), 0);
        assert_eq!(home.audit_acked(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hub_address_must_be_https_without_credentials_and_http_only_for_localhost() {
        for ok in [
            "https://hub.example.invalid",
            "https://hub.example.invalid:8443/",
            "http://127.0.0.1:4000",
            "http://localhost",
        ] {
            assert!(ensure_plain_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://hub.example.invalid",
            "https://user:senha@hub.example.invalid",
            "https://hub.example.invalid/?token=x",
            "https://hub.example.invalid/#x",
            "ftp://hub.example.invalid",
            "https://",
        ] {
            assert!(ensure_plain_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_torn_last_journal_line_is_skipped_and_never_glued_to_the_next_record() {
        let dir = std::env::temp_dir().join(format!("mexerica-state-{}", uuid::Uuid::new_v4()));
        let home = Home::open(&dir).unwrap();
        home.audit("job1", "intent", &json!({})).unwrap();
        let journal = dir.join("journal.jsonl");
        let mut f = fs::OpenOptions::new().append(true).open(&journal).unwrap();
        f.write_all("{\"seq\":2,\"prev\":\"é".as_bytes().split_last().unwrap().1).unwrap();
        drop(f);
        assert_eq!(home.audit_rows().unwrap().len(), 1);
        assert_eq!(home.audit("job1", "created", &json!({})).unwrap(), 2);
        let rows = home.audit_rows().unwrap();
        assert_eq!((rows.len(), rows[1]["event"].clone()), (2, json!("created")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_binding_identity_is_part_of_the_link_and_both_epochs_survive_cleanup() {
        let dir = std::env::temp_dir().join(format!("mexerica-state-{}", uuid::Uuid::new_v4()));
        let home = Home::open(&dir).unwrap();
        let first = home.auth_epoch().unwrap();
        let binding = home.binding_epoch().unwrap();
        assert_ne!(first, "");
        assert_ne!(binding, "");
        let link = HubLink {
            url: "https://hub.example.invalid".into(),
            sign_pub: crypto::b64(&[7u8; 32]),
            label: "Computador".into(),
            computer_id: "computer".into(),
            binding_id: "binding".into(),
        };
        home.save_link(&link).unwrap();
        let next = home.bump_auth_epoch().unwrap();
        assert_ne!(next, first);
        assert_eq!(home.binding_epoch().unwrap(), binding);
        home.save_outbox("job", &json!({"outcome":"done"})).unwrap();
        home.wipe_identity().unwrap();
        assert_eq!(home.auth_epoch().unwrap(), next);
        assert_eq!(home.binding_epoch().unwrap(), binding);
        assert_eq!(home.outbox().len(), 0);
        let old: HubLink = serde_json::from_value(json!({
            "url":"https://hub.example.invalid","sign_pub":"x","label":"x","computer_id":"x"
        }))
        .unwrap();
        assert_eq!(old.binding_id, "");
        assert_ne!(old, link);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_state_folder_that_already_holds_other_files_is_refused() {
        let dir = std::env::temp_dir().join(format!("mexerica-state-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("trabalho.txt"), b"do dono").unwrap();
        assert!(Home::open(&dir).is_err());
        assert_eq!(fs::read(dir.join("trabalho.txt")).unwrap(), b"do dono");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_enroll_is_refused_while_the_first_one_holds_the_state_folder() {
        let dir = std::env::temp_dir().join(format!("mexerica-state-{}", uuid::Uuid::new_v4()));
        let home = Home::open(&dir).unwrap();
        let first = home.enroll_lock().unwrap();
        let refused = home.enroll_lock().unwrap_err();
        assert!(refused.to_string().contains("já existe um cadastro em andamento nesta pasta de estado"));
        drop(first);
        assert!(home.enroll_lock().is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reopening_the_state_folder_never_replaces_its_epochs() {
        let dir = std::env::temp_dir().join(format!("mexerica-state-{}", uuid::Uuid::new_v4()));
        let home = Home::open(&dir).unwrap();
        let binding = home.binding_epoch().unwrap();
        let auth = home.auth_epoch().unwrap();
        assert!(!binding.is_empty() && !auth.is_empty());
        for _ in 0..3 {
            let again = Home::open(&dir).unwrap();
            assert_eq!(again.binding_epoch().unwrap(), binding);
            assert_eq!(again.auth_epoch().unwrap(), auth);
        }
        fs::remove_file(dir.join("binding.epoch")).unwrap();
        let again = Home::open(&dir).unwrap();
        assert_eq!(again.auth_epoch().unwrap(), auth);
        let new_binding = again.binding_epoch().unwrap();
        assert!(!new_binding.is_empty() && new_binding != binding);
        let last = Home::open(&dir).unwrap();
        assert_eq!(last.binding_epoch().unwrap(), new_binding);
        assert_eq!(last.auth_epoch().unwrap(), auth);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_created_record_keeps_the_identity_of_the_subfolder_that_received_the_file() {
        let dir = std::env::temp_dir().join(format!("mexerica-state-{}", uuid::Uuid::new_v4()));
        let home = Home::open(&dir).unwrap();
        home.mark_running("job1", "lease", "fs.write", &json!({}), false).unwrap();
        home.mark_created("job1", 11, 22, Some((33, 44))).unwrap();
        let record = home.running_record("job1").unwrap();
        assert_eq!((record["created"].clone(), record["folderId"].clone()), (json!([11, 22]), json!([33, 44])));
        home.mark_created("job2", 11, 22, None).unwrap_err();
        fs::remove_dir_all(dir).unwrap();
    }
}
