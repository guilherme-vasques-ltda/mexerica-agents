//! Avisos conservadores de persistência no `CODEX_HOME`. Não substituem o chão da spec 13, PR 1.

use anyhow::{Context, Result, bail};
use std::path::Path;

/// A impressão do arquivo só participa da deduplicação; nunca vai ao chat ou ao log.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Warning {
    message: String,
    fingerprint: String,
}

impl Warning {
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// O aviso pertence ao bot, mesmo quando o motor começou num grupo ou numa rotina.
pub fn publish_main(hub: &crate::hub::Hub, bot: &str, warnings: &[Warning]) {
    use crate::store::{EntryKind, Lane};
    if let Err(error) = publish_changed(&hub.store, bot, warnings, |warning| {
        hub.add_entry(
            &Lane::main(bot),
            EntryKind::Notice,
            hub.store.max_turn(bot),
            &serde_json::json!({"text": warning, "style": super::bot::NoticeStyle::Info, "author": bot}),
        )
        .context("não foi possível salvar o aviso do Codex")?;
        Ok(())
    }) {
        tracing::warn!(%bot, %error, "não foi possível registrar os avisos do Codex");
    }
}

/// O hash representa um conjunto, sem depender da ordem dos arquivos em `rules/`.
/// Só marca como publicado depois de salvar todos os avisos no chat.
pub fn publish_changed(
    store: &crate::store::Store,
    bot: &str,
    warnings: &[Warning],
    mut publish: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let mut warnings: Vec<_> = warnings.iter().map(|w| (w.message.as_str(), w.fingerprint.as_str())).collect();
    warnings.sort_unstable();
    warnings.dedup();
    let hash = super::registry::sha256_hex(&serde_json::to_vec(&warnings)?);
    let key = format!("codex.config-warnings:{bot}");
    let saved = store.kv_read(&key)?;
    if saved.as_deref() == Some(hash.as_str()) {
        return Ok(());
    }
    // #38 guardava somente o conjunto de mensagens, sem as impressões privadas.
    let mut messages: Vec<_> = warnings.iter().map(|(message, _)| *message).collect();
    messages.dedup();
    let legacy_hash = super::registry::sha256_hex(&serde_json::to_vec(&messages)?);
    if saved.as_deref() == Some(legacy_hash.as_str()) {
        return store.kv_set(&key, &hash);
    }
    for (message, _) in warnings {
        publish(message)?;
    }
    store.kv_set(&key, &hash)
}

pub fn check_user_config() -> Result<Vec<Warning>> {
    if crate::fork::motor_exec::configured() {
        return check(&Path::new(crate::fork::motor_roots::MOTOR).join(".codex"));
    }
    let home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")));
    home.map_or_else(|| Ok(Vec::new()), |home| check(&home))
}

fn read_if_present(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => {
            bail!("Não foi possível conferir a configuração persistente do Codex. Confira as permissões de CODEX_HOME.")
        }
    }
}

fn compact(text: &str) -> String {
    let (mut quote, mut escaped, mut comment) = (None, false, false);
    text.chars()
        .filter(|&c| {
            if comment {
                comment = c != '\n';
                return false;
            }
            if let Some(q) = quote {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            } else if matches!(c, '\'' | '"') {
                quote = Some(c);
            } else if c == '#' {
                comment = true;
                return false;
            }
            !c.is_whitespace()
        })
        .collect()
}

/// Só detalha o aviso; não determina se o arquivo deve avisar nem interpreta Starlark.
fn rule_marker(kind: &str, args: &str) -> String {
    let (mut quote, mut escaped, mut depth, mut start) = (None, false, 0_u32, 0);
    for (i, c) in args.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if depth == 0 && matches!(c, ',' | ')') {
            if let Some(decision) = args[start..i].strip_prefix("decision=") {
                return match decision {
                    "\"allow\"" | "'allow'" => "regra decision=allow".into(),
                    "\"deny\"" | "'deny'" => "regra decision=deny".into(),
                    "\"forbidden\"" | "'forbidden'" => "regra decision=forbidden".into(),
                    "\"prompt\"" | "'prompt'" => "regra decision=prompt".into(),
                    _ => "regra com decision explícita".into(),
                };
            }
            if c == ')' {
                break;
            }
            start = i + 1;
        } else if matches!(c, '(' | '[' | '{') {
            depth += 1;
        } else if matches!(c, ')' | ']' | '}') {
            depth = depth.saturating_sub(1);
        }
    }
    if kind == "network_rule(" {
        "regra de rede incompleta (sem decision)".into()
    } else {
        "regra sem decision (permite por padrão, equivalente a allow)".into()
    }
}

fn warning(file: &Path, marker: &str, content: &str) -> Warning {
    let message = format!(
        "Aviso do Codex: CODEX_HOME/{} contém {marker}. O motor continuará, mas essa configuração compartilhada pode alterar aprovações em Pedir ou iniciar processos. Revise-a fora do bot. O sempre permitir deve ficar nas regras do Hub (D106), não nos arquivos do Codex.",
        file.display()
    );
    Warning { message, fingerprint: super::registry::sha256_hex(content.as_bytes()) }
}

pub(crate) fn check(home: &Path) -> Result<Vec<Warning>> {
    if crate::fork::motor_exec::configured() {
        return crate::fork::motor_codex_audit::check(home);
    }
    check_local(home)
}

/// Subcomando interno após a troca de UID; não delega novamente.
pub(crate) fn check_local(home: &Path) -> Result<Vec<Warning>> {
    let mut warnings = Vec::new();
    let config_content = read_if_present(&home.join("config.toml"))?;
    let config = compact(&config_content);
    for marker in ["mcp_servers", "hooks", "notify"] {
        if config.contains(marker) {
            warnings.push(warning(Path::new("config.toml"), marker, &config_content));
        }
    }
    let rules = match std::fs::read_dir(home.join("rules")) {
        Ok(rules) => rules,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(warnings),
        Err(_) => bail!("Não foi possível conferir as regras persistentes do Codex em CODEX_HOME/rules."),
    };
    for rule in rules {
        let rule = rule.context("Não foi possível conferir uma regra persistente do Codex")?;
        if rule.path().extension().is_some_and(|extension| extension == "rules") {
            let content = read_if_present(&rule.path())?;
            // Não depender do compact nem dos marcadores: apelidos e strings multilinha
            // podem esconder regras. Qualquer linha ativa exige um aviso conservador.
            if !content.lines().any(|line| !line.trim().is_empty() && !line.trim().starts_with('#')) {
                continue;
            }
            let file = Path::new("rules").join(rule.file_name());
            let before = warnings.len();
            let text = compact(&content);
            for kind in ["prefix_rule(", "network_rule("] {
                for (start, _) in text.match_indices(kind) {
                    let marker = rule_marker(kind, &text[start + kind.len()..]);
                    // Uma mensagem por classe e arquivo; a impressão cobre todas as regras.
                    let notice = warning(&file, &marker, &content);
                    if !warnings.iter().any(|w| w.message == notice.message) {
                        warnings.push(notice);
                    }
                }
            }
            if warnings.len() == before {
                warnings.push(warning(
                    &file,
                    "regras que a auditoria não classificou e que devem ser tratadas como allow",
                    &content,
                ));
            }
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avisa_persistencia_sem_bloquear_nem_imprimir_conteudo() {
        let home = std::env::temp_dir().join(format!("donnabot-audit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join("rules")).unwrap();
        for (config, marker) in [
            ("[mcp_servers.externo]\ncommand='conteudo-privado'", "mcp_servers"),
            ("hooks = { programa = 'conteudo-privado' }", "hooks"),
            ("notify = ['conteudo-privado']", "notify"),
        ] {
            std::fs::write(home.join("config.toml"), config).unwrap();
            let warnings = check(&home).unwrap();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].message().contains("CODEX_HOME/config.toml"));
            assert!(warnings[0].message().contains(marker));
            assert!(!warnings[0].message().contains("conteudo-privado"));
            assert!(!warnings[0].message().contains(&warnings[0].fingerprint));
        }
        std::fs::write(home.join("config.toml"), "# mcp_servers e hooks são só comentário\nmodel='modelo'").unwrap();
        assert_eq!(check(&home).unwrap().len(), 0);
        for decision in ["allow", "deny", "forbidden", "prompt"] {
            for assignment in [format!("decision = \"{decision}\""), format!("decision =\n '{decision}'")] {
                std::fs::write(
                    home.join("rules/default.rules"),
                    format!("prefix_rule(pattern=['python3'], {assignment})"),
                )
                .unwrap();
                let warnings = check(&home).unwrap();
                assert_eq!(warnings.len(), 1);
                assert!(warnings[0].message().contains("CODEX_HOME/rules/default.rules"));
                assert!(warnings[0].message().contains(&format!("regra decision={decision}")));
                assert!(!warnings[0].message().contains("python3"));
            }
        }
        for (kind, marker) in [
            ("prefix_rule", "sem decision (permite por padrão, equivalente a allow)"),
            ("network_rule", "regra de rede incompleta (sem decision)"),
        ] {
            std::fs::write(
                home.join("rules/default.rules"),
                format!("{kind} (\npattern=['conteudo-privado(decision=deny)'] # decision='deny'\n)"),
            )
            .unwrap();
            let warnings = check(&home).unwrap();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].message().contains(marker));
            assert!(!warnings[0].message().contains("conteudo-privado"));
            assert!(!warnings[0].message().contains(&warnings[0].fingerprint));
        }
        std::fs::write(
            home.join("rules/default.rules"),
            "prefix_rule(pattern=['privado'], decision='deny')\nnetwork_rule(host='privado')",
        )
        .unwrap();
        let mixed = check(&home).unwrap();
        assert_eq!(mixed.len(), 2);
        assert!(mixed.iter().any(|w| w.message().contains("decision=deny")));
        assert!(mixed.iter().any(|w| w.message().contains("regra de rede incompleta")));
        std::fs::write(
            home.join("rules/default.rules"),
            "# prefix_rule(pattern=['privado'])\n  # network_rule(host='privado', decision='deny')",
        )
        .unwrap();
        assert_eq!(check(&home).unwrap().len(), 0);
        // Falha de leitura continua sendo erro; marcadores conhecidos são somente avisos.
        std::fs::remove_file(home.join("config.toml")).unwrap();
        std::fs::create_dir(home.join("config.toml")).unwrap();
        assert!(check(&home).is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn toda_linha_ativa_avisa_inclusive_evasoes_sem_marcador() {
        let home = std::env::temp_dir().join(format!("donnabot-audit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join("rules")).unwrap();
        for (content, marker) in [
            ("p = prefix_rule\np(pattern=[\"curl\"])", "não classificou"),
            ("p = prefix_rule\np(pattern=[\"curl\"], decision=\"allow\")", "não classificou"),
            (r##"x = ["""a"#""", prefix_rule(pattern=["curl"], decision="allow")]"##, "não classificou"),
            ("prefix_rule(\npattern=[\"curl\"],\ndecision=\"allow\"\n)", "regra decision=allow"),
            ("prefix_rule( # comentário no meio\npattern=[\"curl\"], decision=\"allow\")", "regra decision=allow"),
        ] {
            std::fs::write(home.join("rules/default.rules"), content).unwrap();
            let warnings = check(&home).unwrap();
            assert_eq!(warnings.len(), 1, "{content}");
            assert!(warnings[0].message().contains(marker));
            assert!(warnings[0].message().contains("CODEX_HOME/rules/default.rules"));
            assert!(!warnings[0].message().contains("curl"));
            assert_eq!(warnings[0].fingerprint, super::super::registry::sha256_hex(content.as_bytes()));
            assert!(!warnings[0].message().contains(&warnings[0].fingerprint));
        }
        for content in ["", " \n\t\n", "# comentário\n  # prefix_rule(pattern=[\"curl\"])\n"] {
            std::fs::write(home.join("rules/default.rules"), content).unwrap();
            assert!(check(&home).unwrap().is_empty(), "{content}");
        }
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn publica_so_quando_o_conjunto_muda_e_separa_por_bot() {
        let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let notice = |message: &str| Warning { message: message.into(), fingerprint: "arquivo".into() };
        let warnings = vec![notice("regra allow"), notice("hooks")];
        let reordered = vec![notice("hooks"), notice("regra allow"), notice("hooks")];
        let mut published = Vec::new();
        for set in [&warnings, &reordered] {
            publish_changed(&store, "bot", set, |warning| {
                published.push(warning.to_owned());
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(published, ["hooks", "regra allow"]);
        let changed = vec![notice("regra forbidden")];
        for (bot, set) in [("bot", &changed), ("outro", &warnings), ("bot", &Vec::new()), ("bot", &changed)] {
            publish_changed(&store, bot, set, |warning| {
                published.push(warning.to_owned());
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(published, ["hooks", "regra allow", "regra forbidden", "hooks", "regra allow", "regra forbidden"]);
    }

    #[test]
    fn falha_de_publicacao_nao_silencia_o_proximo_inicio() {
        let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let warnings = vec![Warning { message: "aviso".into(), fingerprint: "arquivo".into() }];
        assert!(publish_changed(&store, "bot", &warnings, |_| bail!("falha no chat")).is_err());
        let mut published = Vec::new();
        publish_changed(&store, "bot", &warnings, |warning| {
            published.push(warning.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(published, ["aviso"]);
    }

    #[test]
    fn migra_hash_legado_sem_republicar_e_volta_a_avisar_quando_o_arquivo_muda() {
        let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let mut warnings = vec![
            Warning { message: "regra allow".into(), fingerprint: "arquivo".into() },
            Warning { message: "hooks".into(), fingerprint: "config".into() },
            Warning { message: "hooks".into(), fingerprint: "config".into() },
        ];
        let legacy = super::super::registry::sha256_hex(&serde_json::to_vec(&["hooks", "regra allow"]).unwrap());
        store.kv_set("codex.config-warnings:bot", &legacy).unwrap();
        let mut published = Vec::new();
        for _ in 0..2 {
            publish_changed(&store, "bot", &warnings, |message| {
                published.push(message.to_owned());
                Ok(())
            })
            .unwrap();
            assert_eq!(published, Vec::<String>::new());
            assert_ne!(store.kv_get("codex.config-warnings:bot").as_deref(), Some(legacy.as_str()));
        }
        warnings[0].fingerprint = "arquivo alterado".into();
        publish_changed(&store, "bot", &warnings, |message| {
            published.push(message.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(published, ["hooks", "regra allow"]);
        store.kv_set("codex.config-warnings:outro", &legacy).unwrap();
        warnings[0].message = "regra deny".into();
        let mut changed = Vec::new();
        publish_changed(&store, "outro", &warnings, |message| {
            changed.push(message.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(changed, ["hooks", "regra deny"]);
    }

    #[test]
    fn regra_nova_no_mesmo_arquivo_avisa_sem_expor_conteudo_ou_impressao() {
        let home = std::env::temp_dir().join(format!("donnabot-audit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join("rules")).unwrap();
        let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let mut published = Vec::new();
        for kind in ["prefix_rule", "p"] {
            let rule = format!("p = prefix_rule\n{kind}(pattern=['primeiro-privado'])");
            let extended = format!("{rule}\n{kind}(pattern=['segundo-privado'])");
            for content in [&rule, &rule, &extended, &extended] {
                std::fs::write(home.join("rules/default.rules"), content).unwrap();
                let warnings = check(&home).unwrap();
                assert_eq!(warnings.len(), 1);
                publish_changed(&store, kind, &warnings, |message| {
                    assert!(!message.contains("privado"));
                    assert!(!message.contains(&warnings[0].fingerprint));
                    published.push(message.to_owned());
                    Ok(())
                })
                .unwrap();
            }
        }
        assert_eq!(published.len(), 4);
        assert_eq!(published[0], published[1]);
        assert_eq!(published[2], published[3]);
        std::fs::remove_dir_all(home).unwrap();
    }
}
