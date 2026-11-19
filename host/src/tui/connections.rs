//! Connection requests remain actionable in a terminal. Secrets stay in masked setup fields.
use super::app::{After, App, Editor, Entry, Overlay};
use super::manage::{Fields, Input, Submit};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub enum Step {
    Credentials,
    Unavailable,
    Installed,
    Authorize,
    SignIn,
    Finish,
}

pub struct Connection {
    pub entry: String,
    pub request: Value,
    pub step: Step,
}

fn input(name: &str, label: &str, secret: bool, required: bool) -> Input {
    Input {
        name: name.into(),
        label: label.into(),
        secret,
        required,
        multiline: false,
        placeholder: String::new(),
        ed: Editor::default(),
    }
}

pub fn fields(entry: &Entry) -> Fields {
    let request = &entry.data["connectionRequest"];
    let (step, inputs) = match request["kind"].as_str() {
        Some("login") => (
            Step::Credentials,
            vec![input("username", "Username / email", false, true), input("value", "Password / op://", true, true)],
        ),
        Some("secret") => (
            Step::Credentials,
            vec![input("value", request["field"].as_str().unwrap_or("Credential / op://"), true, true)],
        ),
        _ if request["connectorId"].is_string() => (Step::Installed, vec![]),
        _ => (Step::Unavailable, vec![]),
    };
    Fields {
        title: request["title"].as_str().unwrap_or("Connect service").into(),
        note: "Saved securely; never added to chat. Esc closes.".into(),
        choices: vec![],
        choice: 0,
        inputs,
        cursor: 0,
        error: matches!(step, Step::Unavailable).then(|| crate::market::REGISTRY_UNAVAILABLE.into()),
        saving: false,
        submit: Submit::Connection(Box::new(Connection { entry: entry.id.clone(), request: request.clone(), step })),
    }
}

impl App {
    pub(super) fn open_connection(&mut self) {
        let Some(bot) = &self.selected else { return };
        let entry = self
            .lane(bot)
            .into_iter()
            .find(|e| {
                e.data["connectionRequest"]["status"] == "pending" && self.pick.as_ref().is_none_or(|id| *id == e.id)
            })
            .cloned();
        if let Some(entry) = entry {
            let request = &entry.data["connectionRequest"];
            if request["kind"] == "agent" {
                // The host completes the request and resends the messages once a check finds it signed in.
                self.open_agent(request["backend"].as_str().unwrap_or_default(), true);
            } else {
                self.overlays.push(Overlay::Fields(Box::new(fields(&entry))));
            }
        } else {
            self.flash("No pending connection request here");
        }
    }

    pub(super) fn submit_connection(&self, f: &mut Fields, values: &serde_json::Map<String, Value>) {
        let Submit::Connection(c) = &f.submit else { return };
        let (method, args, reply) = match c.step {
            Step::Credentials => (
                "connectorRequestFinish",
                json!({"entryId":c.entry,"value":values.get("value"),"username":values.get("username")}),
                Step::Finish,
            ),
            Step::Unavailable => {
                f.saving = false;
                f.error = Some(crate::market::REGISTRY_UNAVAILABLE.into());
                return;
            }
            Step::Installed => ("connectors", json!({}), Step::Installed),
            Step::Authorize if f.choice == 0 => {
                ("connectorSignIn", json!({"id":c.request["connectorId"]}), Step::SignIn)
            }
            _ => (
                "connectorRequestFinish",
                json!({"entryId":c.entry,"connectorId":c.request["connectorId"]}),
                Step::Finish,
            ),
        };
        self.call(method, args, After::Connection(c.entry.clone(), reply));
    }

    pub(super) fn connection_reply(&mut self, id: &str, step: Step, result: Result<Value, String>) {
        let Some(index) = self.overlays.iter().position(
            |o| matches!(o, Overlay::Fields(f) if matches!(&f.submit, Submit::Connection(c) if c.entry == id)),
        ) else {
            return;
        };
        let Overlay::Fields(f) = &mut self.overlays[index] else { return };
        f.saving = false;
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                f.error = Some(e);
                return;
            }
        };
        f.error = None;
        let Submit::Connection(c) = &mut f.submit else { return };
        let mut finish = false;
        match step {
            Step::Finish => {
                self.overlays.remove(index);
                self.flash("Connection request updated");
                return;
            }
            Step::Installed => {
                let connector = value["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|v| v["id"] == c.request["connectorId"])
                    .cloned()
                    .unwrap_or(Value::Null);
                if connector.is_null() {
                    f.error = Some("Connector is no longer installed".into());
                    return;
                }
                if connector["configuration"] == "manualRequired" {
                    f.error = Some("Configuração MCP incompleta; informe o alvo manualmente.".into());
                    return;
                }
                c.request["connectorId"] = connector["id"].clone();
                for i in &mut f.inputs {
                    i.ed.clear();
                }
                f.inputs.clear();
                f.cursor = 0;
                if connector["auth"] == "signedOut" {
                    c.step = Step::Authorize;
                    f.choices = vec!["Sign in".into(), "Check connection".into()];
                    f.choice = 0;
                } else {
                    c.step = Step::Finish;
                    finish = true;
                }
            }
            Step::SignIn => {
                c.step = Step::Authorize;
                f.choice = 1;
                f.note = "Sign-in link copied. Finish in browser, then Enter checks. Ctrl+X cancels.".into();
                if let Some(url) = value["url"].as_str() {
                    super::app::copy(url);
                    super::manage::open_browser(url, &self.url);
                }
            }
            Step::Credentials | Step::Authorize | Step::Unavailable => {}
        }
        if finish {
            f.saving = true;
            let Submit::Connection(c) = &f.submit else { return };
            let args = json!({"entryId":c.entry,"connectorId":c.request["connectorId"]});
            self.call("connectorRequestFinish", args, After::Connection(id.into(), Step::Finish));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::app::Kind;
    use super::*;
    async fn fixture() -> (
        App,
        tokio::sync::mpsc::UnboundedReceiver<super::super::app::Msg>,
        std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        use axum::{Json, Router, extract::Path, routing::post};
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = calls.clone();
        let router = Router::new().route(
            "/api/{method}",
            post(move |Path(method): Path<String>, Json(body): Json<Value>| {
                let received = received.clone();
                async move {
                    received.lock().unwrap().push(json!({"method":method,"body":body}));
                    Json(json!({}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (App::new(super::super::net::Client::new(&url, Some("fixture")), tx, url), rx, calls, task)
    }

    fn open(app: &mut App, request: &Value) {
        let e = Entry {
            id: "request".into(),
            seq: 1,
            rev: 1,
            turn: 1,
            kind: Kind::Notice,
            data: json!({"connectionRequest":request}),
            created_at: 0,
        };
        app.overlays.push(Overlay::Fields(Box::new(fields(&e))));
    }

    fn key(app: &mut App, code: crossterm::event::KeyCode, modifiers: crossterm::event::KeyModifiers) {
        app.on_key(crossterm::event::KeyEvent::new(code, modifiers));
    }

    async fn reply(app: &mut App, rx: &mut tokio::sync::mpsc::UnboundedReceiver<super::super::app::Msg>) {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await.unwrap().unwrap();
        app.on_msg(msg);
    }

    #[tokio::test]
    async fn credentials_submit_the_exact_secret_and_esc_sends_nothing() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let (mut app, mut rx, calls, server) = fixture().await;
        open(&mut app, &json!({"kind":"login","status":"pending"}));
        let Some(Overlay::Fields(f)) = app.overlays.last_mut() else { panic!("fields") };
        f.inputs[0].ed = Editor::with("name");
        f.inputs[1].ed = Editor::with(" secret with spaces ");
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        reply(&mut app, &mut rx).await;
        assert!(app.overlays.is_empty());
        assert_eq!(
            calls.lock().unwrap()[0]["body"],
            json!({"entryId":"request","username":"name","value":" secret with spaces "})
        );
        open(&mut app, &json!({"kind":"secret","status":"pending"}));
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(calls.lock().unwrap().len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn agent_sign_in_request_opens_the_agent_sheet_checking_again() {
        let (mut app, mut rx, calls, server) = fixture().await;
        let request = json!({"kind":"agent","status":"pending","title":"Claude Code","backend":"claude"});
        let e = Entry {
            id: "card".into(),
            seq: 1,
            rev: 1,
            turn: 1,
            kind: Kind::Notice,
            data: json!({"connectionRequest":request}),
            created_at: 0,
        };
        app.entries.entry("bot".into()).or_default().insert(1, e);
        app.selected = Some("bot".into());
        // Looking signed in doesn't skip the check: this agent just refused a turn.
        app.backends = vec![json!({"id":"claude","signedIn":true})];
        app.open_connection();
        let Some(Overlay::Agent(a)) = app.overlays.last() else { panic!("agent sheet") };
        assert_eq!((a.backend.as_str(), a.busy), ("claude", true));
        reply(&mut app, &mut rx).await;
        assert_eq!(calls.lock().unwrap()[0], json!({"method":"agentAuth","body":{"backend":"claude"}}));
        server.abort();
    }

    #[test]
    fn login_fields_mask_the_password_and_keep_it_out_of_chat() {
        let e = Entry {
            id: "r".into(),
            seq: 1,
            rev: 1,
            turn: 1,
            kind: Kind::Notice,
            data: json!({"connectionRequest":{"kind":"login","status":"pending"}}),
            created_at: 0,
        };
        let f = fields(&e);
        assert!(!f.inputs[0].secret);
        assert!(f.inputs[1].secret);
        assert!(f.inputs.iter().all(|i| i.ed.text.is_empty()));
        assert!(matches!(f.submit, Submit::Connection(c) if matches!(c.step, Step::Credentials)));
    }
}
