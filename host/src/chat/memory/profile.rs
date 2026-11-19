//! Nota "Sobre você" editada em Ajustes › Memória, uma por pessoa e Space (D6, MEX-1347).
//!
//! A chave é a pessoa mais o Space. Hoje o Hub conhece uma pessoa só, o dono ([`OWNER`]); membros de Equipe e
//! convidados terão id próprio na mesma chave, sem migração. A nota do dono no Pessoal continua na camada
//! compartilhada (`bot` vazio) e na lápide legada, então nota, histórico e lápide anteriores já são dele, sem cópia —
//! mas ela nunca é projetada na pasta compartilhada nem sai por ferramenta, busca ou listagem sem a pessoa do dono
//! comprovada no turno. Qualquer outra pessoa ou Space tem uma camada própria ([`layer`]) que nenhuma ferramenta,
//! pasta ou listagem de bot expõe. A nota só entra na conversa da própria pessoa: a pessoa vem da origem que o Hub
//! carimbou ([`author`]) — e, num pedido entre bots, da conversa do remetente, carimbada na admissão.
use crate::{
    api::devices::Caller,
    fork::{autoridade::Autoridade, memory_auto::worker::MemorySection, spaces::PERSONAL},
    hub::Hub,
    store::Entry,
};
use anyhow::{Result, anyhow, ensure};
use codync_mem::files::{self as mf, Author, Cause, Change};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Map, Value, json};

pub(crate) const NAME: &str = "mexerica-sobre-voce.md";
const MARK: &str = "<!-- mexerica:sobre-voce v1 -->";
const TOMBSTONE: &str = "memory.profile.removed";
/// A pessoa que o Hub conhece hoje: o dono, a quem pertencem o computador do Hub e os aparelhos com controle.
pub(crate) const OWNER: &str = "dono";
/// Prefixo da camada da nota de uma pessoa num Space: `profile:<pessoa>:<spaceId>`; `:` não aparece em id de bot.
const PERSON_LAYER: &str = "profile:";

/// Projeção legível, sem a marca interna do formato.
pub(crate) fn visible(text: &str) -> &str {
    text.strip_prefix(MARK).and_then(|s| s.strip_prefix('\n').or_else(|| s.strip_prefix("\r\n"))).unwrap_or(text)
}

/// Camada da nota no diário: a compartilhada (`None`) para o dono no Pessoal, uma própria para cada outra chave.
fn layer(person: &str, space: &str) -> Option<String> {
    (person != OWNER || space != PERSONAL).then(|| format!("{PERSON_LAYER}{person}:{space}"))
}

fn tombstone(person: &str, space: &str) -> String {
    if person == OWNER && space == PERSONAL { TOMBSTONE.to_owned() } else { format!("{TOMBSTONE}.{person}.{space}") }
}

fn revision(db: &Connection, person: &str, space: &str) -> Result<Option<String>> {
    if let Some(file) = mf::get(db, layer(person, space).as_deref(), NAME)? {
        return Ok(Some(file.rev.to_string()));
    }
    Ok(db.query_row("SELECT v FROM kv WHERE k=?1", [tombstone(person, space)], |r| r.get(0)).optional()?)
}

/// A camada compartilhada foi esquecida: a nota que ia com ela é a do dono no Pessoal.
pub(super) fn removed(db: &Connection, rev: i64) -> Result<()> {
    db.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        (tombstone(OWNER, PERSONAL), format!("removed-{rev}")),
    )?;
    Ok(())
}

/// Texto atual da nota de `person` em `space`, com a marca interna. O chamador já conferiu o Space.
pub(crate) fn note(db: &Connection, person: &str, space: &str) -> Result<Option<String>> {
    ensure!(!person.is_empty(), "invalid_person");
    ensure!(!space.is_empty(), "invalid_space");
    Ok(mf::current(db, layer(person, space).as_deref(), NAME)?)
}

/// Nota de `person` em `space` para o extrator, só leitura e com a camada exibida como compartilhada. A nota do dono
/// no Pessoal já chega pela camada compartilhada, filtrada pela pessoa do job.
pub(crate) fn extractor_note(db: &Connection, person: &str, space: &str) -> Result<Option<(mf::File, String)>> {
    let Some(key) = layer(person, space) else { return Ok(None) };
    crate::fork::spaces::require_space(db, space)?;
    let Some(mut file) = mf::get(db, Some(&key), NAME)?.filter(|f| !f.deleted) else { return Ok(None) };
    let Some(text) = mf::current(db, Some(&key), NAME)? else { return Ok(None) };
    file.bot = None;
    Ok(Some((file, visible(&text).to_owned())))
}

/// Pessoa que escreveu a entrada, só pelo ingresso que o próprio Hub carimbou do chamador autenticado
/// (`contentOrigin.ingress`, nunca o corpo do pedido): o computador do Hub e os aparelhos com controle são do dono.
/// Ponte, ingresso ausente e entrada que não é humana não identificam ninguém.
pub(crate) fn author(entry: &Entry) -> Option<String> {
    if !entry.is_user_authored() {
        return None;
    }
    match entry.data["contentOrigin"]["ingress"]["caller"].as_str() {
        Some("local" | "device") => Some(OWNER.to_owned()),
        _ => None,
    }
}

/// Uma resposta só pertence à pessoa quando todos os pedidos anteriores desse turno têm autoria comprovada.
pub(crate) fn turn_is_person_on(db: &Connection, entry: &Entry, person: &str) -> Result<bool> {
    let sql = format!(
        "SELECT {} FROM entries WHERE bot_id=?1 AND turn=?2 AND kind='user' AND seq<?3 AND CASE WHEN ?4=bot_id THEN session IS NULL ELSE session=?4 END ORDER BY seq LIMIT 202",
        crate::store::ENTRY_COLS
    );
    let mut stmt = db.prepare(&sql)?;
    let users: Vec<Entry> = stmt
        .query_map(rusqlite::params![entry.bot_id, entry.turn, entry.seq, entry.session_id], crate::store::row_entry)?
        .collect::<rusqlite::Result<_>>()?;
    if users.len() > 201 {
        return Ok(false);
    }
    let users: Vec<_> = users.iter().filter(|user| user.data["status"] != "queued").collect();
    Ok(!users.is_empty() && users.iter().all(|user| author(user).as_deref() == Some(person)))
}

/// A pessoa de um conjunto de entradas: a mesma em todas; nenhuma entrada, uma sem pessoa ou duas pessoas dão
/// ninguém.
pub(crate) fn person_of<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Option<String> {
    let mut person: Option<String> = None;
    for entry in entries {
        let who = author(entry)?;
        if person.as_ref().is_some_and(|p| *p != who) {
            return None;
        }
        person = Some(who);
    }
    person
}

fn view(db: &Connection, person: &str, space: &str) -> Result<Value> {
    crate::fork::spaces::require_space(db, space)?;
    let content = note(db, person, space)?;
    // Lembranças soltas só existem na camada compartilhada, que é do Pessoal.
    let loose: i64 = if layer(person, space).is_none() {
        db.query_row("SELECT count(*) FROM memory_entries WHERE bot IS NULL", [], |r| r.get(0))?
    } else {
        0
    };
    Ok(json!({"spaceId":space,"content":content,"revision":revision(db, person, space)?,"loose":loose}))
}

/// `spaceId` ausente é o Pessoal; presente precisa ser texto (a guarda comum já recusa `null`).
fn space_of(object: &Map<String, Value>) -> Result<String> {
    match object.get("spaceId") {
        None => Ok(PERSONAL.to_owned()),
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(_) => Err(anyhow!("invalid_space")),
    }
}

/// Pontos da gravação depois do commit, para as provas de corrida com a exclusão do Space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Committed,
    Checked,
    Projected,
    Notified,
}

pub(crate) async fn call(hub: &std::sync::Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    call_with_hook(hub, caller, method, body, |_| {}).await
}

async fn call_with_hook(
    hub: &std::sync::Arc<Hub>,
    caller: &Caller,
    method: &str,
    body: Value,
    hook: impl Fn(Step) + Send + 'static,
) -> Result<Value> {
    // A RPC é do dono (computador do Hub ou aparelho dele com controle): lê e grava só a nota dele.
    let authority = Autoridade::capturar(hub, caller)?;
    // A gravação segura a trava do quadro, a mesma da exclusão do Space, do commit até o último efeito: a exclusão
    // fica antes (a transação recusa) ou depois (a gravação e seus efeitos já saíram), nunca entre os dois.
    let guard = if method == "memoryProfile" { None } else { Some(hub.store.team_lock.clone().lock_owned().await) };
    let (hub, method) = (hub.clone(), method.to_owned());
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let object = body.as_object().ok_or_else(|| anyhow!("invalid_field"))?;
        let space = space_of(object)?;
        let extra = usize::from(object.contains_key("spaceId"));
        if method == "memoryProfile" {
            ensure!(object.len() == extra, "invalid_field");
            return authority.with_db(&hub, None, |db| view(db, OWNER, &space));
        }
        ensure!(
            object.len() == 2 + extra && object.contains_key("content") && object.contains_key("base"),
            "invalid_field"
        );
        // MX-280: closure destacada não pode gravar depois do sinal de parada.
        let _write = hub.memory_write()?;
        let content = match &body["content"] {
            Value::Null => None,
            Value::String(s) if !s.trim().is_empty() => Some(s.as_str()),
            _ => return Err(anyhow!("invalid_field: nota vazia")),
        };
        let base = match &body["base"] {
            Value::Null => None,
            Value::String(s) => Some(s.as_str()),
            _ => return Err(anyhow!("invalid_field")),
        };
        let key = layer(OWNER, &space);
        let value = super::api::write(&MemorySection::enter(&hub), &authority, |db, now, rev| {
            // Space e revisão reconferidos na mesma transação da gravação.
            crate::fork::spaces::require_space(db, &space)?;
            ensure!(revision(db, OWNER, &space)?.as_deref() == base, "memory_profile_conflict");
            if let Some(text) = content {
                codync_mem::check::secrets(text).map_err(|_| anyhow!("secret_detected"))?;
            }
            let change = Change {
                bot: key.as_deref(),
                name: NAME,
                content,
                author: Author::User,
                author_bot: None,
                cause: if content.is_some() { Cause::Edit } else { Cause::Delete },
                mtime: now,
                size: content.map_or(0, |s| s.len() as u64),
            };
            // Mesmo uma exclusão de nota ausente cria uma revisão, como na demo.
            let version = mf::record(db, &change, now, rev)?;
            if content.is_some() {
                codync_mem::nature::set_file(db, version.id, codync_mem::nature::KindNature::Said, None, None)?;
                codync_mem::origin::set_file(
                    db,
                    version.id,
                    &codync_mem::origin::Origin {
                        schema: 1,
                        kind: codync_mem::origin::Kind::User,
                        sources: Vec::new(),
                        external: false,
                    },
                )?;
            }
            view(db, OWNER, &space)
        })?;
        // Sob a trava, cada efeito ainda vê o Space; a conferência fica como defesa.
        hook(Step::Committed);
        hub.store.require_space(&space)?;
        hook(Step::Checked);
        if key.is_none() {
            super::file_api::project(&hub, None, NAME);
        }
        hook(Step::Projected);
        super::profile_changed(&hub, &space);
        hook(Step::Notified);
        let _ = hub.events.send(json!({"type":"memoryProfile","spaceId":space,"revision":value["revision"]}));
        Ok(value)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;
    use crate::api::dispatch;
    use crate::store::{BotConfig, Store};
    use std::sync::Arc;

    fn with_hub() -> (Arc<Hub>, std::path::PathBuf) {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store
            .save_bot(
                &serde_json::from_value(
                    json!({"id":"pessoal","name":"Pessoal","backend":"fixture","cwd":"/tmp","sharedMemory":true}),
                )
                .unwrap(),
            )
            .unwrap();
        let dir = std::env::temp_dir().join(format!("mexerica-sobre-voce-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let identity = crate::remote::identity::Identity::load_or_create(&dir).unwrap();
        (Hub::new(store, "sobre-voce".into(), identity, "fixture".into(), 0), dir)
    }

    async fn create_space(hub: &Arc<Hub>, name: &str, letters: &str) -> String {
        let snap = dispatch(hub, &Caller::Local, "donnaSpaces", json!({"hubId":hub.host_id})).await.unwrap();
        let created = dispatch(
            hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({
                "hubId":hub.host_id, "expectedRevision":snap["revision"], "name":name,
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":letters}}
            }),
        )
        .await
        .unwrap();
        created["spaces"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn save(hub: &Arc<Hub>, space: Option<&str>, content: &str, base: &Value) -> Result<Value> {
        let mut body = json!({"content":content,"base":base});
        if let Some(space) = space {
            body["spaceId"] = json!(space);
        }
        dispatch(hub, &Caller::Local, "saveMemoryProfile", body).await
    }

    /// Grava uma versão da nota direto na camada `bot` do diário (`None`: a compartilhada, legada).
    fn record(store: &Store, bot: Option<&str>, text: &str) {
        store
            .memory_write(|db, now, rev| {
                mf::record(
                    db,
                    &Change {
                        bot,
                        name: NAME,
                        content: Some(text),
                        author: Author::User,
                        author_bot: None,
                        cause: Cause::Edit,
                        mtime: now,
                        size: text.len() as u64,
                    },
                    now,
                    rev,
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn bot(value: Value) -> BotConfig {
        serde_json::from_value(value).unwrap()
    }

    fn of(mut cfg: BotConfig, person: Option<&str>) -> BotConfig {
        cfg.person = person.map(str::to_owned);
        cfg
    }

    #[tokio::test]
    async fn pedido_em_b_devolve_so_b_e_so_a_pessoa_da_conversa_recebe() {
        let (hub, dir) = with_hub();
        record(&hub.store, None, &format!("{MARK}\nPessoal: evitar pressa.\n"));
        let b = create_space(&hub, "Trabalho", "TR").await;
        let c = create_space(&hub, "Clube", "CL").await;
        let personal = dispatch(&hub, &Caller::Local, "memoryProfile", json!({})).await.unwrap();
        assert_eq!(personal["spaceId"], PERSONAL);
        assert!(personal["content"].as_str().unwrap().contains("Pessoal: evitar pressa."));
        // Outro Space começa sem nota, sem herdar a do Pessoal.
        let empty = dispatch(&hub, &Caller::Local, "memoryProfile", json!({"spaceId":b})).await.unwrap();
        assert_eq!(empty, json!({"spaceId":b,"content":null,"revision":null,"loose":0}));
        let bot_b = bot(json!({"id":"bot-b","name":"B","backend":"fixture","cwd":"/tmp","spaceId":b}));
        hub.store.save_bot(&bot_b).unwrap();
        let heard_personal = super::super::current_revision(&hub.store, "pessoal");
        let mut events = hub.events.subscribe();
        let saved_b = save(&hub, Some(&b), "B: relatórios curtos.", &Value::Null).await.unwrap();
        assert_eq!(saved_b["spaceId"], b.as_str());
        let event = loop {
            let event = events.try_recv().unwrap();
            if event["type"] == "memoryProfile" {
                break event;
            }
        };
        assert_eq!(event["spaceId"], b.as_str());
        assert_eq!(event["revision"], saved_b["revision"]);
        // O evento chega ao stream de B e nunca ao do Pessoal nem ao de C.
        let delivered = crate::fork::spaces::filter_event(&hub, &b, event.clone()).unwrap();
        assert_eq!(delivered, event);
        assert!(crate::fork::spaces::filter_event(&hub, PERSONAL, event.clone()).is_none());
        assert!(crate::fork::spaces::filter_event(&hub, &c, event).is_none());
        // Só os bots de B ouvem a mudança da nota de B.
        assert_eq!(super::super::current_revision(&hub.store, "pessoal"), heard_personal);
        assert_ne!(super::super::current_revision(&hub.store, "bot-b"), "");
        save(&hub, Some(&c), "C: fala inglês.", &Value::Null).await.unwrap();
        let read_b = dispatch(&hub, &Caller::Local, "memoryProfile", json!({"spaceId":b})).await.unwrap();
        assert_eq!(read_b, saved_b);
        let read_c = dispatch(&hub, &Caller::Local, "memoryProfile", json!({"spaceId":c})).await.unwrap();
        assert_eq!(read_c["content"], "C: fala inglês.");
        assert_eq!(dispatch(&hub, &Caller::Local, "memoryProfile", json!({})).await.unwrap(), personal);
        // A revisão é do Space: a do Pessoal não serve de base em B.
        let conflict = save(&hub, Some(&b), "x", &personal["revision"]).await.unwrap_err();
        assert!(conflict.to_string().contains("memory_profile_conflict"));
        // Fronteira comum: `null` recusa, Space inexistente recusa, campo extra recusa.
        for (body, expected) in [
            (json!({"spaceId":null}), "invalid_space"),
            (json!({"spaceId":"nao-existe"}), "space_not_found"),
            (json!({"spaceId":b,"x":1}), "invalid_field"),
        ] {
            let error = dispatch(&hub, &Caller::Local, "memoryProfile", body).await.unwrap_err().to_string();
            assert!(error.contains(expected), "{expected}: {error}");
        }
        assert!(
            save(&hub, Some("nao-existe"), "x", &Value::Null)
                .await
                .unwrap_err()
                .to_string()
                .contains("space_not_found")
        );

        // Abertura: a conversa do dono recebe só a nota dele no Space do bot.
        let opening_b = super::super::opening_scored(&hub.store, &of(bot_b.clone(), Some(OWNER)), None);
        assert!(
            opening_b.contains("B: relatórios curtos.")
                && !opening_b.contains("Pessoal: evitar")
                && !opening_b.contains("C: fala")
        );
        let personal_bot = hub.store.bot("pessoal").unwrap().unwrap().config;
        let opening_p = super::super::opening_scored(&hub.store, &of(personal_bot.clone(), Some(OWNER)), None);
        assert!(opening_p.contains("Pessoal: evitar pressa.") && !opening_p.contains("B: relatórios"));
        assert!(!opening_p.contains(MARK));
        let mut moved = of(bot_b.clone(), Some(OWNER));
        moved.space_id.clone_from(&c);
        let opening_c = super::super::opening_scored(&hub.store, &moved, None);
        assert!(opening_c.contains("C: fala inglês.") && !opening_c.contains("B: relatórios"));
        // Mesmo bot e Space, conversa de outra pessoa ou de remetente desconhecido: nada da nota do dono.
        for person in [None, Some("membro-b")] {
            for cfg in [of(bot_b.clone(), person), of(personal_bot.clone(), person)] {
                let opening = super::super::opening_scored(&hub.store, &cfg, None);
                assert!(!opening.contains("B: relatórios") && !opening.contains("Pessoal: evitar"), "{person:?}");
            }
        }
        // A chave é pessoa + Space: a nota futura de outra pessoa só entra na conversa dela.
        record(&hub.store, layer("membro-b", &b).as_deref(), "Membro B: prefere áudio.");
        let member = super::super::opening_scored(&hub.store, &of(bot_b.clone(), Some("membro-b")), None);
        assert!(member.contains("Membro B: prefere áudio.") && !member.contains("B: relatórios"));
        let owner = super::super::opening_scored(&hub.store, &of(bot_b, Some(OWNER)), None);
        assert!(owner.contains("B: relatórios") && !owner.contains("Membro B"));
        assert_eq!(dispatch(&hub, &Caller::Local, "memoryProfile", json!({"spaceId":b})).await.unwrap(), read_b);
        // Identidade alheia e chamada interna sem Space não recebem nota nenhuma, nem a do Pessoal.
        let mut foreign = of(personal_bot.clone(), Some(OWNER));
        foreign.space_id.clone_from(&b);
        foreign.context = Some(crate::fork::spaces::ExecutionContext {
            conversation_id: "conversa".into(),
            memory_id: "outra".into(),
            foreign_identity: true,
        });
        let opening_f = super::super::opening_scored(&hub.store, &foreign, None);
        assert!(!opening_f.contains("B: relatórios") && !opening_f.contains("Pessoal: evitar"));
        let mut unscoped = of(personal_bot, Some(OWNER));
        unscoped.space_id.clear();
        assert!(!super::super::opening_scored(&hub.store, &unscoped, None).contains("Pessoal: evitar"));
        assert!(note(&hub.store.db.locked(), OWNER, "").unwrap_err().to_string().contains("invalid_space"));
        assert!(note(&hub.store.db.locked(), "", PERSONAL).unwrap_err().to_string().contains("invalid_person"));
        // A nota de B não entra na camada compartilhada do Pessoal nem na camada do bot de B.
        let db = hub.store.db.locked();
        assert_eq!(note(&db, OWNER, PERSONAL).unwrap(), mf::current(&db, None, NAME).unwrap());
        assert!(mf::layer(&db, None).unwrap().iter().all(|f| f.name == NAME));
        assert_eq!(mf::layer(&db, Some("bot-b")).unwrap(), Vec::<mf::File>::new());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn exclusao_do_space_espera_a_gravacao_e_todos_os_efeitos() {
        let (hub, dir) = with_hub();
        let b = create_space(&hub, "Trabalho", "TR").await;
        let first = save(&hub, Some(&b), "antes", &Value::Null).await.unwrap();
        let snap = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({"hubId":hub.host_id})).await.unwrap();
        let (steps, mut seen) = tokio::sync::mpsc::unbounded_channel();
        let (resume, paused) = std::sync::mpsc::channel::<()>();
        let (worker, observer, space) = (hub.clone(), hub.clone(), b.clone());
        let body = json!({"spaceId":b,"content":"depois","base":first["revision"]});
        let mut events = hub.events.subscribe();
        let pending = tokio::spawn(async move {
            call_with_hook(&worker, &Caller::Local, "saveMemoryProfile", body, move |step| {
                // Em cada ponto depois do commit: a trava da exclusão está segura e o Space ainda existe.
                let held = observer.store.team_lock.try_lock().is_err();
                let exists = observer.store.require_space(&space).is_ok();
                steps.send((step, held, exists)).unwrap();
                paused.recv().unwrap();
            })
            .await
        });
        let mut deletion = None;
        for expected in [Step::Committed, Step::Checked, Step::Projected, Step::Notified] {
            let (step, held, exists) = seen.recv().await.unwrap();
            assert_eq!(step, expected);
            assert!(held && exists, "{step:?}");
            // A exclusão pedida no meio da gravação espera a trava: não passa na frente de nenhum efeito.
            let waiting = deletion.get_or_insert_with(|| {
                let (hub, b, revision) = (hub.clone(), b.clone(), snap["revision"].clone());
                tokio::spawn(async move {
                    let body =
                        json!({"hubId":hub.host_id,"spaceId":b,"expectedRevision":revision,"confirmName":"Trabalho"});
                    dispatch(&hub, &Caller::Local, "donnaSpaceDelete", body).await
                })
            });
            tokio::task::yield_now().await;
            assert!(!waiting.is_finished(), "{step:?}: a exclusão passou na frente");
            resume.send(()).unwrap();
        }
        let saved = pending.await.unwrap().unwrap();
        assert_eq!(saved["content"], "depois");
        let event = std::iter::from_fn(|| events.try_recv().ok()).find(|e| e["type"] == "memoryProfile").unwrap();
        assert_eq!(event["spaceId"], b.as_str());
        deletion.unwrap().await.unwrap().unwrap();
        // A gravação confirmada fica registrada; depois da exclusão nada mais grava nem lê.
        let key = layer(OWNER, &b);
        let versions = hub.store.memory_read(|db| mf::versions(db, key.as_deref(), NAME, 10)).unwrap();
        assert_eq!(versions.len(), 2);
        let mut events = hub.events.subscribe();
        let after = save(&hub, Some(&b), "tarde", &Value::Null).await.unwrap_err();
        assert!(after.to_string().contains("space_not_found"));
        assert!(std::iter::from_fn(|| events.try_recv().ok()).all(|e| e["type"] != "memoryProfile"));
        let read = dispatch(&hub, &Caller::Local, "memoryProfile", json!({"spaceId":b})).await.unwrap_err();
        assert!(read.to_string().contains("space_not_found"));
        assert_eq!(hub.store.memory_read(|db| mf::versions(db, key.as_deref(), NAME, 10)).unwrap().len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn gravacoes_concorrentes_com_a_mesma_base_so_uma_vence() {
        let (hub, dir) = with_hub();
        let b = create_space(&hub, "Trabalho", "TR").await;
        let first = save(&hub, Some(&b), "base", &Value::Null).await.unwrap();
        let (one, two) = tokio::join!(
            save(&hub, Some(&b), "um", &first["revision"]),
            save(&hub, Some(&b), "dois", &first["revision"])
        );
        let failures: Vec<_> = [&one, &two].into_iter().filter_map(|result| result.as_ref().err()).collect();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].to_string().contains("memory_profile_conflict"));
        let won = one.as_ref().or(two.as_ref()).unwrap();
        assert_eq!(&dispatch(&hub, &Caller::Local, "memoryProfile", json!({"spaceId":b})).await.unwrap(), won);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nota_historico_e_lapide_legados_ficam_no_pessoal_apos_reabrir() {
        let dir = crate::store::private_fixture_dir("mexerica-sobre-voce-legado");
        let path = dir.join("hub.db");
        {
            let store = Store::open(&path).unwrap();
            store.db.locked().execute("INSERT INTO kv(k,v) VALUES(?1,'removed-3')", [TOMBSTONE]).unwrap();
            let db = store.db.locked();
            assert_eq!(revision(&db, OWNER, PERSONAL).unwrap().as_deref(), Some("removed-3"));
            assert_eq!(revision(&db, OWNER, "outro").unwrap(), None);
            assert_eq!(revision(&db, "membro-b", PERSONAL).unwrap(), None);
            drop(db);
            record(&store, None, "primeira");
            record(&store, None, "segunda");
        }
        // Reabrir duas vezes repete as migrações sem copiar nem mover nada.
        for _ in 0..2 {
            let store = Store::open(&path).unwrap();
            let db = store.db.locked();
            assert_eq!(note(&db, OWNER, PERSONAL).unwrap().as_deref(), Some("segunda"));
            assert_eq!(mf::versions(&db, None, NAME, 10).unwrap().len(), 2);
            assert_eq!(note(&db, OWNER, "outro").unwrap(), None);
            assert_eq!(note(&db, "membro-b", PERSONAL).unwrap(), None);
            assert_eq!(revision(&db, OWNER, "outro").unwrap(), None);
            let copies: i64 = db
                .query_row("SELECT count(*) FROM memory_file_versions WHERE bot LIKE 'profile:%'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(copies, 0);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ponte_e_ferramentas_nao_expoem_gravacao() {
        assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(&"saveMemoryProfile"));
        assert!(!super::super::file_tools::definitions().iter().any(|v| v["name"] == "saveMemoryProfile"));
    }

    #[test]
    fn origem_carimbada_identifica_o_dono_e_o_resto_ninguem() {
        let entry = |kind: &str, data: Value| Entry {
            id: "e".into(),
            seq: 1,
            bot_id: "bot".into(),
            rev: 1,
            kind: kind.into(),
            turn: 1,
            data,
            created_at: 0,
            updated_at: 0,
            session_id: "bot".into(),
        };
        let stamped = |caller: &str| json!({"text":"oi","contentOrigin":{"ingress":{"caller":caller}}});
        for caller in ["local", "device"] {
            assert_eq!(author(&entry("user", stamped(caller))).as_deref(), Some(OWNER), "{caller}");
        }
        for data in [stamped("ponte"), json!({"text":"oi"}), json!({"text":"oi","contentOrigin":{"ingress":null}})] {
            assert_eq!(author(&entry("user", data.clone())), None, "{data}");
        }
        // Mensagem de outro bot ou de rotina, mesmo com ingresso, não é de pessoa nenhuma.
        let mut relayed = stamped("local");
        relayed["sourceBotId"] = json!("outro");
        assert_eq!(author(&entry("user", relayed)), None);
        assert_eq!(author(&entry("agent", stamped("local"))), None);
        let owner = entry("user", stamped("local"));
        let unknown = entry("user", stamped("ponte"));
        assert_eq!(person_of([&owner, &owner]).as_deref(), Some(OWNER));
        assert_eq!(person_of([&owner, &unknown]), None);
        assert_eq!(person_of(std::iter::empty::<&Entry>()), None);
    }

    #[test]
    fn abertura_so_no_pessoal_com_interruptor_pessoa_e_sem_marca() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        record(&store, None, &format!("{MARK}\n# Sobre você\n\n## O que não fazer\nEvitar pressa.\n"));
        let mut cfg: BotConfig =
            serde_json::from_value(json!({"id":"b", "name":"Bot", "backend":"claude", "sharedMemory":true})).unwrap();
        assert!(!super::super::opening_scored(&store, &cfg, None).contains("Evitar pressa."), "sem pessoa");
        cfg.person = Some(OWNER.into());
        let opening = super::super::opening_scored(&store, &cfg, None);
        assert!(opening.contains("Evitar pressa."));
        assert!(!opening.contains(MARK));
        cfg.shared_memory = Some(false);
        assert!(!super::super::opening_scored(&store, &cfg, None).contains("Evitar pressa."));
        cfg.shared_memory = Some(true);
        cfg.space_id = "outro".into();
        assert!(!super::super::opening_scored(&store, &cfg, None).contains("Evitar pressa."));
    }
}
