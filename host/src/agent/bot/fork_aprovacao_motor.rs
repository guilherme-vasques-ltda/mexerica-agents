//! Kilo e Cline: o Hub trava a aprovação no lançamento, para que toda ação passe pelo porteiro e vire cartão.
//!
//! - Kilo roda sozinho por padrão (regra `"*": "allow"` do próprio motor) e lê a variável `KILO_PERMISSION` por cima
//!   da config. O Hub a define com tudo em «perguntar», e tira de cada cartão o «sempre permitir», que no Kilo grava
//!   uma regra permanente na config global dele, fora do porteiro.
//! - Cline pergunta por padrão em ACP, mas a opção de sessão `auto_approve` liga a aprovação automática. O Hub nunca
//!   a repassa do bot e confere que está desligada em toda sessão (nova ou retomada); se não conseguir, derruba o motor.
use super::Actor;
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};

/// O curinga cobre as ferramentas que não conhecemos (as do Hub chegam como `servidor_ferramenta`). As de risco que o
/// Kilo conhece também vão por nome: uma regra específica do usuário na config global (`"bash": "allow"`) vence o
/// curinga quando vem depois dele, e só uma chave de mesmo nome a substitui. Leitura de arquivo e listagem de nomes
/// seguem livres; `grep` lê conteúdo (inclusive de `.env`) sem passar pela regra de `read`, então pergunta.
const KILO_PERMISSION: &str = r#"{
    "*": "ask",
    "edit": "ask", "write": "ask", "apply_patch": "ask", "bash": "ask", "webfetch": "ask", "websearch": "ask",
    "task": "ask", "skill": "ask", "lsp": "ask", "grep": "ask", "external_directory": "ask",
    "markdown_source": "ask", "doom_loop": "ask", "semantic_search": "ask", "recall": "ask",
    "kilo_memory_recall": "ask", "kilo_memory_save": "ask", "agent_manager": "ask", "browser_open": "ask",
    "sandbox_escalation": "ask", "workflow_tool_approval": "ask", "notebook_read": "ask", "notebook_edit": "ask",
    "notebook_execute": "ask", "board_read": "ask", "board_post": "ask", "goal": "ask",
    "read": {"*": "allow", "*.env": "ask", "*.env.*": "ask", "*.env.example": "allow"},
    "glob": "allow", "list": "allow", "todoread": "allow", "todowrite": "allow",
    "question": "deny", "suggest": "deny", "plan_enter": "deny", "plan_exit": "deny",
    "repo_clone": "deny", "repo_overview": "deny"
}"#;

/// Variáveis que o Hub impõe ao motor, depois de qualquer outra fonte (a última vence).
pub(super) fn launch_env(backend: &str) -> Vec<(String, String)> {
    if backend != "kilo" {
        return Vec::new();
    }
    [("KILO_PERMISSION", KILO_PERMISSION), ("KILO_DISABLE_PROJECT_CONFIG", "1"), ("KILO_CONFIG_CONTENT", "{}")]
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .into()
}

/// Opção de modelo que muda como o motor aprova ou executa: só o Hub decide isso, nunca o bot.
pub(super) fn is_approval_option(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    ["approv", "yolo", "permission", "bypass", "dangerous", "allow", "accept", "trust"]
        .iter()
        .any(|word| id.contains(word))
}

/// Opções do cartão: no Kilo, «sempre permitir» vira regra permanente do motor e fica de fora.
pub(super) fn card_options(backend: &str, mut options: Vec<Value>) -> Vec<Value> {
    if backend == "kilo" {
        options.retain(|option| option["kind"] != "allow_always");
    }
    options
}

fn auto_approve(res: &Value) -> Option<&Value> {
    res["configOptions"].as_array()?.iter().find(|option| option["id"] == "auto_approve")
}

impl Actor {
    /// Cline: `auto_approve` precisa estar desligada. Só `false` conta como desligada; qualquer outra coisa, inclusive
    /// a opção ausente, liga: sem prova de que está desligada, o motor cai.
    pub(super) async fn lock_cline_approval(&mut self, sid: &str, res: Value) -> Result<Value> {
        let Some(option) = auto_approve(&res) else {
            if let Some(conn) = self.conn.take() {
                conn.kill().await;
            }
            bail!("o Cline não informou se a aprovação automática está desligada");
        };
        if option["currentValue"] == false {
            return Ok(res);
        }
        let answer = self
            .session_request(
                "session/set_config_option",
                json!({"sessionId": sid, "configId": "auto_approve", "value": false}),
            )
            .await;
        let answer = answer.and_then(|answer| {
            ensure!(
                auto_approve(&answer).is_some_and(|option| option["currentValue"] == false),
                "o Cline não confirmou a aprovação automática desligada"
            );
            Ok(answer)
        });
        if answer.is_err()
            && let Some(conn) = self.conn.take()
        {
            conn.kill().await;
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kilo_recebe_regras_validas_com_tudo_em_perguntar() {
        let env = launch_env("kilo");
        let rules: Value =
            serde_json::from_str(&env.iter().find(|(key, _)| key == "KILO_PERMISSION").unwrap().1).unwrap();
        assert_eq!(
            rules.as_object().unwrap().keys().next().unwrap(),
            "*",
            "o curinga vem primeiro; o último que casa vence"
        );
        assert_eq!(rules["*"], "ask");
        for ferramenta in ["edit", "write", "bash", "webfetch", "websearch", "task", "grep", "external_directory"] {
            assert_eq!(rules[ferramenta], "ask", "{ferramenta}: regra do usuário na config global não pode liberar");
        }
        assert!(env.contains(&("KILO_DISABLE_PROJECT_CONFIG".into(), "1".into())));
        assert!(launch_env("cline").is_empty() && launch_env("claude").is_empty());
    }

    #[test]
    fn so_o_kilo_perde_o_sempre_permitir() {
        let options = vec![
            json!({"optionId": "once", "kind": "allow_once"}),
            json!({"optionId": "always", "kind": "allow_always"}),
            json!({"optionId": "reject", "kind": "reject_once"}),
        ];
        assert_eq!(card_options("kilo", options.clone()).len(), 2);
        assert_eq!(card_options("cline", options).len(), 3);
    }

    #[test]
    fn opcao_de_aprovacao_nunca_vem_do_bot() {
        for id in ["auto_approve", "yolo", "permission-policy", "bypassPermissions", "allowAll", "acceptEdits"] {
            assert!(is_approval_option(id), "{id}");
        }
        for id in ["effort", "fast", "reasoning_effort", "context"] {
            assert!(!is_approval_option(id), "{id}");
        }
    }
}
