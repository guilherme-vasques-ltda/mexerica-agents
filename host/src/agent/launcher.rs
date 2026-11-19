//! Validação do comando escolhido pelo dono. Evita overrides acidentais; não inspeciona código (D62/D90).

use crate::store::BotConfig;
use anyhow::{Result, anyhow, ensure};

fn name(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word).to_ascii_lowercase();
    name.strip_suffix(".exe").or_else(|| name.strip_suffix(".cmd")).unwrap_or(&name).to_owned()
}

pub fn check(cfg: &BotConfig) -> Result<()> {
    if !matches!(cfg.backend.as_str(), "codex" | "custom") {
        return Ok(());
    }
    let Some(command) = cfg.command.as_deref().filter(|c| !c.trim().is_empty()) else { return Ok(()) };
    let error = || {
        anyhow!(
            "Use um comando literal do adaptador, sem wrappers de shell/ambiente, atribuições, avaliação de código ou flags de configuração, perfil e aprovação; a Mexerica fixa a política do Codex."
        )
    };
    ensure!(
        !command.contains([
            ';', '|', '&', '$', '`', '<', '>', '\n', '\r', '*', '?', '[', ']', '{', '}', '(', ')', '#', '%', '!'
        ]),
        "{}",
        error()
    );
    let words = shlex::split(command).ok_or_else(error)?;
    ensure!(!words.is_empty(), "{}", error());
    for (i, word) in words.iter().enumerate() {
        // Só ~/ no executável: não deixa outra expansão de shell alterar o que foi validado.
        ensure!(!word.contains('~') || (i == 0 && word.starts_with("~/") && !word[2..].contains('~')), "{}", error());
        ensure!(
            !matches!(
                name(word).as_str(),
                "env"
                    | "envdir"
                    | "direnv"
                    | "s6-envdir"
                    | "dotenv"
                    | "dotenv-cli"
                    | "cross-env"
                    | "cross-env-shell"
                    | "nice"
                    | "nohup"
                    | "timeout"
                    | "stdbuf"
                    | "setsid"
                    | "taskset"
                    | "chrt"
                    | "setpriv"
                    | "sudo"
                    | "doas"
                    | "su"
                    | "xargs"
                    | "parallel"
                    | "sh"
                    | "bash"
                    | "zsh"
                    | "fish"
                    | "cmd"
                    | "powershell"
                    | "pwsh"
            ),
            "{}",
            error()
        );
        ensure!(!word.contains('=') || (i > 0 && word.starts_with("--")), "{}", error());
        let flag = word.split('=').next().unwrap_or_default();
        ensure!(
            ![
                "--config",
                "--profile",
                "--sandbox",
                "--ask-for-approval",
                "--dangerously",
                "--full-auto",
                "--yolo",
                "--eval",
                "--env",
                "--unset",
                "--ignore-environment",
                "--call",
                "--command"
            ]
            .iter()
            .any(|blocked| flag.starts_with(blocked))
                && !["-c", "-e", "-p", "-s", "-a"]
                    .iter()
                    .any(|blocked| word.starts_with(blocked) && !word.starts_with("--")),
            "{}",
            error()
        );
    }
    // Só o runner conhecido do Codex tem gramática fechada. Custom aceita flags e argumentos literais.
    ensure!(name(&words[0]) != "exec", "{}", error());
    let args = &words[1..];
    if name(&words[0]) == "codex-acp" {
        ensure!(args.is_empty(), "{}", error());
    } else if name(&words[0]) == "npx" && args.iter().any(|a| a.contains("codex-acp")) {
        let package = match args {
            [package] => Some(package),
            [yes, package] if matches!(yes.as_str(), "-y" | "--yes") => Some(package),
            _ => None,
        };
        ensure!(
            package.is_some_and(|p| p.chars().all(|c| c.is_ascii_alphanumeric() || "@/._-".contains(c))),
            "{}",
            error()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(command: &str) -> BotConfig {
        serde_json::from_value(json!({"id":"a", "name":"Teste", "backend":"custom", "command":command})).unwrap()
    }

    #[test]
    fn custom_aceita_argumentos_literais_e_flags_de_interpretador() {
        for command in [
            "python3 -X utf8 -u '/fixtures/com espaço.py'",
            "copilot --acp --stdio",
            "grok agent stdio",
            "uvx pacote",
            "python3 -m pacote",
            "~/bin/agente --acp",
            "junie --acp=true",
            "droid exec --output-format acp-daemon",
            "node /adapter/index.js literal",
            "npx -y @google/gemini-cli --experimental-acp",
            "codex-acp",
            "npx -y @agentclientprotocol/codex-acp@2.2.2",
        ] {
            assert!(check(&cfg(command)).is_ok(), "{command}");
        }
    }

    #[test]
    fn recusa_wrappers_expansoes_e_overrides_de_modo() {
        for command in [
            "env CODEX_PATH=/outro codex-acp",
            "nice env -u CODEX_CONFIG codex-acp",
            "sh script.sh",
            "exec codex-acp",
            "node --eval codigo",
            "python3 -c codigo",
            "npx --call comando",
            "npx -y dotenv-cli codex-acp",
            "agente --config arquivo",
            "agente --profile=livre",
            "agente --sandbox danger-full-access",
            "agente --ask-for-approval never",
            "agente --yolo",
            "agente --dangerously-bypass-approvals-and-sandbox",
            "/usr/bin/e[n]v codex-acp",
            "agente; outro",
            "agente $VAR",
            "codex-acp argumento",
            "npx -y codex-acp argumento",
        ] {
            assert!(check(&cfg(command)).is_err(), "{command}");
        }
    }
}
