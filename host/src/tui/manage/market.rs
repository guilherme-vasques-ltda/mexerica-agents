// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The marketplace: agents, connectors and skills.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{Value, json};

use super::super::app::{After, App, Confirm, ConfirmAct, Editor, Overlay, fuzzy};
use super::{Fields, Input, Market, Reply, Row, Submit, TABS, Tab, ctrl, s};

impl App {
    pub fn open_market(&mut self) {
        self.call("refreshBackends", json!({}), After::Backends);
        self.call("connectors", json!({}), After::Connectors);
        self.call("skills", json!({}), After::Skills);
        self.sheet("marketSkills", json!({}), Reply::Skills);
        self.overlays.push(Overlay::Market(Box::new(Market {
            tab: Tab::Agents,
            query: Editor::default(),
            skills: None,
            cursor: 0,
            error: None,
        })));
    }

    /// The marketplace's rows on its current tab, filtered by the query.
    pub fn market_rows<'a>(&'a self, m: &'a Market) -> Vec<Row<'a>> {
        let q = m.query.text.trim();
        let hit = |v: &Value, keys: &[&str]| {
            let fields: Vec<&str> = keys.iter().map(|k| v[*k].as_str().unwrap_or_default()).collect();
            fuzzy(q, &fields)
        };
        match m.tab {
            Tab::Agents => {
                let agents = self
                    .backends
                    .iter()
                    .filter(|b| b["available"] == true || b["installed"] == true || b["curated"] == true);
                let node = agents.clone().any(|b| b["node"] == "missing").then_some(Row::Node);
                node.into_iter().chain(agents.filter(|b| hit(b, &["name", "id"])).map(Row::Agent)).collect()
            }
            Tab::Connectors => {
                let mut rows: Vec<Row> =
                    self.plugins.0.iter().filter(|c| hit(c, &["name", "description"])).map(Row::Installed).collect();
                rows.push(Row::Custom);
                rows
            }
            Tab::Skills => {
                let installed: Vec<String> = self.plugins.1.iter().map(|v| s(v, "id").to_lowercase()).collect();
                let mut rows: Vec<Row> =
                    self.plugins.1.iter().filter(|v| hit(v, &["name", "description"])).map(Row::OwnSkill).collect();
                rows.extend(
                    m.skills
                        .iter()
                        .flatten()
                        .filter(|v| !installed.contains(&s(v, "source").to_lowercase()))
                        .filter(|v| hit(v, &["name", "description"]))
                        .map(Row::Skill),
                );
                rows
            }
        }
    }

    pub(in crate::tui) fn market_key(&mut self, mut m: Box<Market>, k: KeyEvent) -> Option<Overlay> {
        let n = self.market_rows(&m).len();
        match k.code {
            KeyCode::Esc => return None,
            KeyCode::Tab | KeyCode::BackTab => {
                let i = TABS.iter().position(|(t, _)| *t == m.tab).unwrap_or(0);
                let d = if k.code == KeyCode::Tab { 1 } else { TABS.len() - 1 };
                m.tab = TABS[(i + d) % TABS.len()].0;
                m.cursor = 0;
            }
            KeyCode::Down => m.cursor = (m.cursor + 1).min(n.saturating_sub(1)),
            KeyCode::Up => m.cursor = m.cursor.saturating_sub(1),
            KeyCode::Char('n' | 'j') if ctrl(k) => m.cursor = (m.cursor + 1).min(n.saturating_sub(1)),
            KeyCode::Char('p') if ctrl(k) => m.cursor = m.cursor.saturating_sub(1),
            KeyCode::Char('r') if ctrl(k) => {
                self.call("connectors", json!({}), After::Connectors);
                self.call("skills", json!({}), After::Skills);
                self.call("refreshBackends", json!({}), After::Backends);
            }
            KeyCode::Enter => return self.market_enter(m),
            KeyCode::Char('x') if ctrl(k) => {
                let (id, name, skill) = match self.market_rows(&m).get(m.cursor) {
                    Some(Row::Installed(c)) => (s(c, "id"), s(c, "name"), false),
                    Some(Row::OwnSkill(v)) => (s(v, "id"), s(v, "name"), true),
                    _ => return Some(Overlay::Market(m)),
                };
                let c = Confirm {
                    title: format!("Remove {name}?"),
                    detail: if skill {
                        "Bots stop using this skill.".into()
                    } else {
                        "Bots lose it, and the keys saved for it are deleted.".into()
                    },
                    note: String::new(),
                    button: "Remove",
                    act: if skill { ConfirmAct::RemoveSkill(id) } else { ConfirmAct::RemoveConnector(id) },
                };
                self.overlays.push(Overlay::Market(m));
                self.overlays.push(Overlay::Confirm(c));
                return None;
            }
            _ => {
                if m.query.key(k) {
                    m.cursor = 0;
                }
            }
        }
        Some(Overlay::Market(m))
    }

    fn market_enter(&mut self, m: Box<Market>) -> Option<Overlay> {
        enum Act {
            Node,
            Agent(String),
            SignIn(String),
            ManualRequired,
            Custom,
            Skill(Value, bool),
            None,
        }
        let act = match self.market_rows(&m).get(m.cursor) {
            Some(Row::Node) => Act::Node,
            Some(Row::Agent(b)) => Act::Agent(s(b, "id")),
            Some(Row::Installed(c)) if c["configuration"] == "manualRequired" => Act::ManualRequired,
            Some(Row::Installed(c)) if c["auth"] == "signedOut" => Act::SignIn(s(c, "id")),
            Some(Row::Custom) => Act::Custom,
            Some(Row::Skill(v)) => Act::Skill((*v).clone(), true),
            Some(Row::OwnSkill(v)) => Act::Skill((*v).clone(), false),
            _ => Act::None,
        };
        match act {
            Act::Node => {
                let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
                self.start_setup("", "node", None, &json!({"cols": cols, "rows": rows}));
            }
            Act::Agent(id) => {
                self.overlays.push(Overlay::Market(m));
                self.open_agent(&id, false);
                return None;
            }
            Act::SignIn(id) => {
                self.flash("Starting the sign-in…");
                self.sheet("connectorSignIn", json!({"id": id}), Reply::SignIn);
            }
            Act::ManualRequired => {
                self.flash("Configuração MCP incompleta; o alvo precisa ser informado manualmente.");
            }
            Act::Custom => {
                self.overlays.push(Overlay::Market(m));
                self.overlays.push(Overlay::Fields(Box::new(Fields {
                    title: "Custom connector".into(),
                    note: "Paste the MCP config from a README or another app (Claude, Cursor, VS Code). Every server in it is added.".into(),
                    choices: vec![],
                    choice: 0,
                    inputs: vec![Input {
                        name: "config".into(),
                        label: "Config".into(),
                        secret: false,
                        required: true,
                        multiline: true,
                        placeholder: r#"{"mcpServers": {"name": {"command": "npx", "args": ["-y", "…"]}}}"#.into(),
                        ed: Editor::default(),
                    }],
                    cursor: 0,
                    error: None,
                    saving: false,
                    submit: Submit::Import,
                })));
                return None;
            }
            Act::Skill(v, catalog) => {
                self.overlays.push(Overlay::Market(m));
                self.open_skill(&v, catalog);
                return None;
            }
            Act::None => {}
        }
        Some(Overlay::Market(m))
    }
}
