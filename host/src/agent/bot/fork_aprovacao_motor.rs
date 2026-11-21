//! Kilo e Cline: o que o Hub faz para que as ações do motor passem pelo cartão de aprovação.
//!
//! - As ferramentas do Hub (memória, rotinas, gestão, conectores…) não dependem do motor: o Hub exige o cartão no ponto em
//!   que recebe a chamada (`fork::aprovacao_despacho`).
//! - O que o motor faz sozinho no computador (editar arquivo, rodar comando) o Hub não consegue garantir de fora. O Kilo
//!   roda por construção no modo «perguntar» (variável `KILO_PERMISSION`, perfil de configuração do Hub, caminho
//!   absoluto do executável, sem retomar sessão, sem «sempre permitir» no cartão), mas segue marcado como «age no
//!   computador sem pedir aprovação»: o Hub não lê de volta nem prova a configuração do motor.
//! - Cline pergunta por padrão em ACP, mas a opção de sessão `auto_approve` liga a aprovação automática. O Hub nunca
//!   a repassa do bot e confere que está desligada em toda sessão (nova ou retomada); se não conseguir, derruba o motor.
use super::Actor;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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

/// Variáveis que o Hub impõe ao motor, depois de qualquer outra fonte (a última vence). O Kilo roda com um perfil que o
/// Hub escreve (configuração e «home» do motor), sem a configuração global nem a regra por agente da pessoa, mas com o
/// login, o cache e o estado de sempre. Isto é esforço de construção, não prova: configuração gerenciada da máquina ou da
/// organização ainda entra, e é por isso que o Kilo continua marcado como «age no computador sem pedir aprovação».
pub(super) fn launch_env(backend: &str, perfil: &Path) -> Result<Vec<(String, String)>> {
    if backend != "kilo" {
        return Ok(Vec::new());
    }
    let mut env: Vec<(String, String)> = [
        ("KILO_PERMISSION", KILO_PERMISSION.to_owned()),
        ("KILO_DISABLE_PROJECT_CONFIG", "1".to_owned()),
        ("KILO_CONFIG_CONTENT", "{}".to_owned()),
    ]
    .map(|(key, value)| (key.to_owned(), value))
    .into();
    if crate::fork::motor_exec::configured() {
        return Ok(env);
    }
    let real = |variavel: &str, sub: &str| {
        std::env::var_os(variavel)
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| dirs::home_dir().map(|home| home.join(sub)))
            .context("o Hub não achou a pasta do Kilo")
    };
    let (config, casa) = (perfil.join("config"), perfil.join("home"));
    for dir in [&config, &casa] {
        std::fs::create_dir_all(dir).context("o Hub não conseguiu criar o perfil do Kilo")?;
    }
    let texto = |dir: PathBuf| dir.to_string_lossy().into_owned();
    env.extend([
        ("XDG_CONFIG_HOME".to_owned(), texto(config)),
        ("KILO_TEST_HOME".to_owned(), texto(casa)),
        ("XDG_DATA_HOME".to_owned(), texto(real("XDG_DATA_HOME", ".local/share")?)),
        ("XDG_CACHE_HOME".to_owned(), texto(real("XDG_CACHE_HOME", ".cache")?)),
        ("XDG_STATE_HOME".to_owned(), texto(real("XDG_STATE_HOME", ".local/state")?)),
    ]);
    Ok(env)
}

/// O Kilo roda pelo caminho absoluto do executável, resolvido uma vez antes do lançamento: um programa de mesmo nome
/// colocado depois, num diretório anterior do PATH, não o substitui.
pub(super) fn fixar_executavel(backend: &str, comando: &str, env: &[(String, String)]) -> Result<String> {
    if backend != "kilo" {
        return Ok(comando.to_owned());
    }
    let comando = comando.trim();
    let (programa, resto) = comando.split_once(char::is_whitespace).unwrap_or((comando, ""));
    if Path::new(programa).is_absolute() {
        return Ok(comando.to_owned());
    }
    let path = env
        .iter()
        .rev()
        .find(|(chave, _)| chave == "PATH")
        .map(|(_, valor)| std::ffi::OsString::from(valor))
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    let achado = std::env::split_paths(&path)
        .map(|dir| dir.join(programa))
        .find(|candidato| candidato.is_file())
        .with_context(|| format!("o Hub não encontrou o executável {programa} para fixar o caminho"))?;
    let achado = achado.to_string_lossy();
    let achado = if achado.contains(char::is_whitespace) { format!("\"{achado}\"") } else { achado.into_owned() };
    Ok(format!("{achado} {resto}").trim_end().to_owned())
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
        match resposta {
            Ok(resposta) => {
                let mut res = res;
                res["configOptions"] = resposta["configOptions"].clone();
                Ok(res)
            }
            Err(erro) => {
                if let Some(conn) = self.conn.take() {
                    conn.kill().await;
                }
                bail!("{erro:#}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kilo_recebe_regras_validas_com_tudo_em_perguntar() {
        let perfil = std::env::temp_dir().join(format!("mexerica-perfil-{}", uuid::Uuid::new_v4()));
        let env = launch_env("kilo", &perfil).unwrap();
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
        assert!(launch_env("cline", &perfil).unwrap().is_empty() && launch_env("claude", &perfil).unwrap().is_empty());
        if !crate::fork::motor_exec::configured() {
            let valor = |chave: &str| env.iter().find(|(k, _)| k == chave).map(|(_, v)| v.clone());
            assert_eq!(valor("XDG_CONFIG_HOME"), Some(perfil.join("config").to_string_lossy().into_owned()));
            assert_eq!(valor("KILO_TEST_HOME"), Some(perfil.join("home").to_string_lossy().into_owned()));
            assert!(perfil.join("config").is_dir() && perfil.join("home").is_dir());
        }
        let _ = std::fs::remove_dir_all(&perfil);
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

    #[test]
    fn executavel_do_kilo_vira_caminho_absoluto() {
        let pasta = std::env::temp_dir().join(format!("mexerica-bin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&pasta).unwrap();
        std::fs::write(pasta.join("kilo"), "").unwrap();
        let env = vec![("PATH".to_owned(), pasta.to_string_lossy().into_owned())];
        let fixo = fixar_executavel("kilo", "kilo acp", &env).unwrap();
        assert_eq!(fixo, format!("{} acp", pasta.join("kilo").display()));
        assert_eq!(fixar_executavel("kilo", &fixo, &[]).unwrap(), fixo, "já absoluto não muda");
        assert!(fixar_executavel("kilo", "outro acp", &env).is_err());
        assert_eq!(fixar_executavel("cline", "cline acp", &env).unwrap(), "cline acp");
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
