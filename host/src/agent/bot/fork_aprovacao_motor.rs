//! Kilo e Cline: o Hub trava a aprovação no lançamento, para que toda ação passe pelo porteiro e vire cartão.
//!
//! - Kilo roda sozinho por padrão (regra `"*": "allow"` do próprio motor) e lê a variável `KILO_PERMISSION` por cima
//!   da config. O Hub a define com tudo em «perguntar», e tira de cada cartão o «sempre permitir», que no Kilo grava
//!   uma regra permanente na config global dele, fora do porteiro.
//! - Cline pergunta por padrão em ACP, mas a opção de sessão `auto_approve` liga a aprovação automática. O Hub nunca
//!   a repassa do bot e confere que está desligada em toda sessão (nova ou retomada); se não conseguir, derruba o motor.
use super::Actor;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::time::Duration;

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
    ["approv", "yolo", "permission", "bypass", "dangerous"].iter().any(|word| id.contains(word))
}

/// Opções do cartão: no Kilo, «sempre permitir» vira regra permanente do motor e fica de fora.
pub(super) fn card_options(backend: &str, mut options: Vec<Value>) -> Vec<Value> {
    if backend == "kilo" {
        options.retain(|option| option["kind"] != "allow_always");
    }
    options
}

/// A opção `auto_approve` só vale se vier uma vez, booleana e falsa: duplicada, ausente ou de outro tipo não prova nada.
fn auto_approve_desligada(res: &Value) -> bool {
    let opcoes: Vec<&Value> =
        res["configOptions"].as_array().into_iter().flatten().filter(|option| option["id"] == "auto_approve").collect();
    matches!(opcoes.as_slice(), [option] if option["currentValue"] == false)
}

/// O Kilo carrega a sessão antiga com as permissões que ela já tinha (inclusive um «sempre permitir» do próprio motor),
/// que o Hub não consegue ler nem revogar: o Hub abre sempre uma sessão nova e refaz a continuidade pelo histórico dele.
pub(super) fn sem_retomada(backend: &str, conn: &mut super::Conn) {
    if backend == "kilo" {
        conn.sessions.load = false;
    }
}

/// O agente que o Hub aceita no Kilo, e que a sessão precisa informar.
const KILO_AGENTE: &str = "build";

/// Servidores MCP do Hub: ferramentas `servidor_ferramenta` que a regra efetiva do Kilo não pode liberar.
const SERVIDORES_DO_HUB: [&str; 8] =
    ["chat", "memory", "manage", "computers", "routines", "team", "tasks", "connectors"];

/// Casamento de padrão do Kilo (`*` e `?`; um padrão que termina em « *» também casa sem argumentos).
fn casa(texto: &str, padrao: &str) -> bool {
    fn recursivo(texto: &[char], padrao: &[char]) -> bool {
        match padrao.split_first() {
            None => texto.is_empty(),
            Some(('*', resto)) => (0..=texto.len()).any(|i| recursivo(&texto[i..], resto)),
            Some(('?', resto)) => texto.split_first().is_some_and(|(_, t)| recursivo(t, resto)),
            Some((c, resto)) => texto.split_first().is_some_and(|(t, outros)| t == c && recursivo(outros, resto)),
        }
    }
    let texto: Vec<char> = texto.replace('\\', "/").chars().collect();
    let padrao = padrao.replace('\\', "/");
    let simples: Vec<char> = padrao.chars().collect();
    if recursivo(&texto, &simples) {
        return true;
    }
    padrao.strip_suffix(" *").is_some_and(|prefixo| recursivo(&texto, &prefixo.chars().collect::<Vec<_>>()))
}

/// A regra que vence (a última que casa) numa lista de regras `{permission, pattern, action}`; sem regra, «ask».
fn acao_efetiva(regras: &[Value], permissao: &str, alvo: &str) -> String {
    regras
        .iter()
        .rfind(|regra| {
            regra["permission"].as_str().is_some_and(|p| casa(permissao, p))
                && regra["pattern"].as_str().is_some_and(|p| casa(alvo, p))
        })
        .and_then(|regra| regra["action"].as_str())
        .unwrap_or("ask")
        .to_owned()
}

/// As ferramentas que a regra efetiva do agente `build` precisa deixar em «ask» ou «deny», com os alvos de teste.
fn ferramentas_do_hub_e_de_risco() -> Vec<(String, &'static [&'static str])> {
    let mut lista: Vec<(String, &'static [&'static str])> = [
        ("edit", &["a.txt", "src/main.rs", "*"][..]),
        ("write", &["a.txt"][..]),
        ("apply_patch", &["a.txt"][..]),
        ("bash", &["rm -rf build", "git status", "cat README.md", "*"][..]),
        ("webfetch", &["https://example.invalid/"][..]),
        ("websearch", &["x"][..]),
        ("task", &["general"][..]),
        ("skill", &["x"][..]),
        ("grep", &["segredo"][..]),
        ("external_directory", &["/tmp/*"][..]),
        ("agent_manager", &["*"][..]),
        ("kilo_memory_save", &["*"][..]),
    ]
    .into_iter()
    .map(|(nome, alvos)| (nome.to_owned(), alvos))
    .collect();
    lista.extend(SERVIDORES_DO_HUB.iter().map(|servidor| (format!("{servidor}_sondagem"), &["*"][..])));
    lista
}

/// Confere a regra efetiva já resolvida do agente (`kilo debug agent build`): nenhuma ferramenta de risco nem do Hub pode
/// estar liberada. Devolve os nomes que passariam sem cartão.
fn ferramentas_liberadas(agente: &Value) -> Vec<String> {
    let Some(regras) = agente["permission"].as_array() else { return vec!["(regra efetiva ilegível)".to_owned()] };
    let mut liberadas = Vec::new();
    for (ferramenta, alvos) in ferramentas_do_hub_e_de_risco() {
        if alvos.iter().any(|alvo| acao_efetiva(regras, &ferramenta, alvo) == "allow") {
            liberadas.push(ferramenta);
        }
    }
    liberadas
}

/// Lê de volta, com o mesmo comando, ambiente e pasta do lançamento, a regra efetiva do agente do Kilo (configuração
/// global, regra por agente, curingas e o que mais o motor tenha resolvido) e recusa o lançamento se alguma ferramenta de
/// risco ou do Hub passaria sem cartão. Sem como ler, também recusa.
pub(super) async fn provar_kilo(backend: &str, comando: &str, cwd: &str, env: &[(String, String)]) -> Result<()> {
    if backend != "kilo" {
        return Ok(());
    }
    let base =
        comando.trim_end().strip_suffix(" acp").context("o Hub não sabe ler a regra efetiva deste comando do Kilo")?;
    let shell = crate::shell::line(&format!("{base} debug agent {KILO_AGENTE}"));
    let pasta = std::path::Path::new(cwd);
    let mut processo = if crate::fork::motor_exec::configured() {
        let mut processo = crate::fork::motor_exec::command(shell.program, pasta);
        processo.args(&shell.args);
        processo
    } else {
        shell.tokio()
    };
    processo
        .env_clear()
        .envs(crate::agent::environment::inherited(std::env::vars_os()))
        .envs(shell.env)
        .envs(env.iter().map(|(chave, valor)| (chave, valor)))
        .current_dir(crate::fork::motor_exec::hub_cwd(pasta))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let saida = tokio::time::timeout(Duration::from_secs(30), crate::fork::motor_exec::output(&mut processo))
        .await
        .context("o Kilo não respondeu à leitura da regra efetiva")?
        .context("não foi possível ler a regra efetiva do Kilo")?;
    ensure!(saida.status.success(), "o Kilo não informou a regra efetiva do agente {KILO_AGENTE}");
    let agente: Value = serde_json::from_slice(&saida.stdout).context("a regra efetiva do Kilo veio ilegível")?;
    let liberadas = ferramentas_liberadas(&agente);
    ensure!(
        liberadas.is_empty(),
        "a configuração do Kilo libera sem cartão: {}. A Mexerica não abre o Kilo assim.",
        liberadas.join(", ")
    );
    Ok(())
}

impl Actor {
    /// Cline: `auto_approve` precisa vir uma vez, booleana e falsa. Ligada, tenta desligar e confere o eco; ausente,
    /// duplicada ou de outro tipo, o motor cai: sem prova de que está desligada, não há turno.
    pub(super) async fn lock_cline_approval(&mut self, sid: &str, res: Value) -> Result<Value> {
        if auto_approve_desligada(&res) {
            return Ok(res);
        }
        let opcoes: Vec<&Value> = res["configOptions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|option| option["id"] == "auto_approve")
            .collect();
        let resposta = if matches!(opcoes.as_slice(), [option] if option["currentValue"].is_boolean()) {
            self.session_request(
                "session/set_config_option",
                json!({"sessionId": sid, "configId": "auto_approve", "value": false}),
            )
            .await
        } else {
            Err(anyhow::anyhow!("o Cline não informou, de forma inequívoca, se a aprovação automática está desligada"))
        };
        let resposta = resposta.and_then(|resposta| {
            ensure!(auto_approve_desligada(&resposta), "o Cline não confirmou a aprovação automática desligada");
            Ok(resposta)
        });
        if let Err(erro) = resposta {
            if let Some(conn) = self.conn.take() {
                conn.kill().await;
            }
            bail!("{erro:#}");
        }
        Ok(res)
    }

    /// Kilo: a sessão precisa informar o agente que o Hub leu de volta (`build`); outro agente ou nenhuma informação,
    /// o motor cai.
    pub(super) async fn confirmar_agente_do_kilo(&mut self, res: Value) -> Result<Value> {
        let agente = res["configOptions"].as_array().into_iter().flatten().find(|o| o["id"] == "mode");
        if agente.is_some_and(|o| o["currentValue"] == KILO_AGENTE) {
            return Ok(res);
        }
        if let Some(conn) = self.conn.take() {
            conn.kill().await;
        }
        bail!("o Kilo não confirmou o agente {KILO_AGENTE} lido pelo Hub");
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
        for id in ["auto_approve", "yolo", "permission-policy", "bypassPermissions", "dangerouslySkip"] {
            assert!(is_approval_option(id), "{id}");
        }
        for id in ["effort", "fast", "reasoning_effort", "context", "allowed_tools", "acceptance"] {
            assert!(!is_approval_option(id), "{id}");
        }
    }

    fn agente(regras: &Value) -> Value {
        json!({"name": "build", "permission": regras})
    }

    #[test]
    fn regra_efetiva_com_tudo_em_perguntar_passa() {
        let regras = json!([
            {"permission": "*", "pattern": "*", "action": "allow"},
            {"permission": "*", "pattern": "*", "action": "ask"},
            {"permission": "read", "pattern": "*", "action": "allow"},
            {"permission": "bash", "pattern": "cat *", "action": "ask"},
        ]);
        assert_eq!(ferramentas_liberadas(&agente(&regras)), Vec::<String>::new());
    }

    #[test]
    fn regra_por_agente_ou_curinga_depois_do_curinga_libera_e_e_apanhado() {
        let base = json!([{"permission": "*", "pattern": "*", "action": "ask"}]);
        let mut por_agente = base.as_array().unwrap().clone();
        por_agente.push(json!({"permission": "bash", "pattern": "*", "action": "allow"}));
        assert!(ferramentas_liberadas(&agente(&json!(por_agente))).contains(&"bash".to_owned()));
        let mut curinga = base.as_array().unwrap().clone();
        curinga.push(json!({"permission": "manage_*", "pattern": "*", "action": "allow"}));
        assert!(ferramentas_liberadas(&agente(&json!(curinga))).contains(&"manage_sondagem".to_owned()));
        let mut por_comando = base.as_array().unwrap().clone();
        por_comando.push(json!({"permission": "bash", "pattern": "git *", "action": "allow"}));
        assert!(
            ferramentas_liberadas(&agente(&json!(por_comando))).contains(&"bash".to_owned()),
            "um comando liberado já conta"
        );
    }

    #[test]
    fn regra_ilegivel_nao_prova_nada() {
        assert_ne!(ferramentas_liberadas(&json!({"name": "build"})), Vec::<String>::new());
        assert_ne!(ferramentas_liberadas(&json!({"permission": "ask"})), Vec::<String>::new());
    }

    #[test]
    fn padrao_do_kilo_casa_como_o_motor() {
        assert!(casa("git status", "git *") && casa("git", "git *") && !casa("gitx", "git *"));
        assert!(casa("manage_x", "manage_*") && casa("a/b/c", "a/*") && casa("ab", "a?") && !casa("abc", "a?"));
    }

    #[test]
    fn auto_approve_so_vale_uma_vez_booleana_e_falsa() {
        let opcao = |valor: Value| json!({"id": "auto_approve", "currentValue": valor});
        assert!(auto_approve_desligada(&json!({"configOptions": [opcao(json!(false))]})));
        for ruim in [
            json!({}),
            json!({"configOptions": []}),
            json!({"configOptions": [opcao(json!(true))]}),
            json!({"configOptions": [opcao(json!("false"))]}),
            json!({"configOptions": [opcao(json!(false)), opcao(json!(false))]}),
            json!({"configOptions": [opcao(json!(false)), opcao(json!(true))]}),
        ] {
            assert!(!auto_approve_desligada(&ruim), "{ruim}");
        }
    }
}
