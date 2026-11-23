//! Validação do comando escolhido pelo dono. Evita overrides acidentais; não inspeciona código (D62/D90).

use crate::store::BotConfig;
use anyhow::{Result, anyhow, ensure};

fn name(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word).to_ascii_lowercase();
    name.strip_suffix(".exe").or_else(|| name.strip_suffix(".cmd")).unwrap_or(&name).to_owned()
}

fn check_target(cfg: &BotConfig, bin: &str) -> Result<()> {
    ensure!(!crate::fork::assinatura_pessoal::muse(bin), crate::fork::assinatura_pessoal::Refusal::Meta);
    ensure!(
        cfg.backend == "claude" || !crate::fork::assinatura_pessoal::claude(bin),
        crate::fork::assinatura_pessoal::Refusal::Automation
    );
    crate::agent::backends::require_supported_execution(bin)?;
    let removed = crate::agent::backends::HARNESSES
        .iter()
        .any(|h| crate::fork::motores_retirados::retirado(h.id) && h.bins.contains(&bin));
    ensure!(!removed, "Este comando abre um motor que não está disponível nesta versão da Mexerica.");
    Ok(())
}

fn check_known_launch(cfg: &BotConfig, words: &[String], depth: u8) -> Result<()> {
    let unknown =
        || anyhow!("Use um comando literal do adaptador; não foi possível identificar o destino do inicializador.");
    let mut words = words.iter();
    let Some(executable) = words.find(|word| {
        !word
            .split_once('=')
            .is_some_and(|(key, _)| !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
    }) else {
        return Ok(());
    };
    let args = words.as_slice();
    ensure!(depth < 8, "{}", unknown());
    let bin = name(executable);
    check_target(cfg, &bin)?;
    match bin.as_str() {
        "env" => {
            let mut tail = args;
            while let Some((option, rest)) = tail.split_first() {
                tail = match option.as_str() {
                    "--" => {
                        tail = rest;
                        break;
                    }
                    "-" | "-i" | "--ignore-environment" => rest,
                    "-u" | "--unset" | "-C" | "--chdir" => rest.get(1..).ok_or_else(unknown)?,
                    flag if flag.starts_with("--unset=") || flag.starts_with("--chdir=") => rest,
                    flag if flag.starts_with('-') => return Err(unknown()),
                    _ => break,
                };
            }
            let first_target = tail.iter().position(|word| !word.contains('=')).unwrap_or(tail.len());
            check_known_launch(cfg, &tail[first_target..], depth + 1)
        }
        "sh" | "bash" | "zsh" if args.first().is_some_and(|flag| flag == "-c") => {
            let script = args.get(1).ok_or_else(unknown)?;
            ensure!(!script.starts_with('-'), "{}", unknown());
            ensure!(
                !script.contains([
                    ';', '|', '&', '$', '`', '<', '>', '\n', '\r', '(', ')', '#', '*', '?', '[', ']', '{', '}', '~'
                ]),
                "{}",
                unknown()
            );
            let script_words = shlex::split(script).filter(|parts| !parts.is_empty()).ok_or_else(unknown)?;
            check_known_launch(cfg, &script_words, depth + 1)
        }
        "sh" | "bash" | "zsh" if args.iter().take_while(|a| a.starts_with('-')).any(|a| a.contains('c')) => {
            Err(unknown())
        }
        "npx" | "bunx" | "pnpm" => {
            let tail = if bin == "pnpm" {
                let Some(dlx) = args.iter().position(|arg| arg == "dlx") else {
                    return Ok(());
                };
                ensure!(dlx == 0 || !args[0].starts_with('-'), "{}", unknown());
                if dlx != 0 {
                    return Ok(());
                }
                &args[dlx + 1..]
            } else {
                args
            };
            let mut launch = tail.iter().skip_while(|arg| matches!(arg.as_str(), "-y" | "--yes"));
            let mut target = launch.next().map(String::as_str);
            if target == Some("--") {
                target = launch.next().map(String::as_str);
            }
            let package = target.filter(|word| !word.starts_with('-')).ok_or_else(unknown)?;
            let base = package.rsplit_once('@').filter(|(base, _)| !base.is_empty()).map_or(package, |(base, _)| base);
            check_target(cfg, &name(base))
        }
        _ => Ok(()),
    }
}

pub fn check(cfg: &BotConfig) -> Result<()> {
    if let Some(command) = cfg.command.as_deref().filter(|c| !c.trim().is_empty()) {
        let mut quote = None;
        let mut escaped = false;
        for ch in command.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            match (quote, ch) {
                (Some('\''), '\'') | (Some('"'), '"') => quote = None,
                (None, '\'' | '"') => quote = Some(ch),
                (None | Some('"'), '\\') => escaped = true,
                (
                    None,
                    ';' | '|' | '&' | '$' | '`' | '<' | '>' | '\n' | '\r' | '(' | ')' | '#' | '*' | '?' | '[' | ']'
                    | '{' | '}',
                )
                | (Some('"'), '$' | '`') => {
                    return Err(anyhow!("Use um comando literal do adaptador, sem expansão ou operadores de shell."));
                }
                _ => {}
            }
        }
        let words = shlex::split(command).ok_or_else(|| anyhow!("Comando do adaptador inválido."))?;
        check_known_launch(cfg, &words, 0)?;
    }
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
    fn classifica_pacotes_e_comandos_literais_sem_inspecionar_wrappers_arbitrarios() {
        for (command, blocked) in [
            ("env LANG=C claude-code", true),
            ("LANG=C claude-code", true),
            ("meu-adaptador; claude-code", true),
            ("sh -c 'LANG=C claude-code'", true),
            ("bash -lc 'claude-code'", true),
            ("bash -c -e claude-code", true),
            ("pnpm --silent dlx @anthropic-ai/claude-code@1", true),
            ("env -- LANG=C claude-code", true),
            ("env -i -u PATH claude-code", true),
            ("env env claude-code", true),
            ("sh -c 'env LANG=C claude-code'", true),
            ("sh -c 'claude-code && outro'", true),
            ("npx -p pacote outro", true),
            ("env LANG=C kilo", true),
            ("bunx kilo", true),
            ("sh -c 'npx -y @anthropic-ai/claude-code@1'", true),
            ("bunx @anthropic-ai/claude-code@1", true),
            ("pnpm dlx @anthropic-ai/claude-code@1", true),
            ("env LANG=C pnpm dlx @anthropic-ai/claude-code@1", true),
            ("sh -c 'meu-adaptador @anthropic-ai/claude-code'", false),
            ("bunx meu-adaptador @anthropic-ai/claude-code", false),
            ("pnpm run claude-code", false),
        ] {
            let mut bot = cfg(command);
            bot.backend = "grok".into();
            assert_eq!(check(&bot).is_err(), blocked, "{command}");
        }
    }

    #[test]
    fn custom_aceita_argumentos_literais_e_flags_de_interpretador() {
        assert!(check(&cfg("meu-adaptador --state-dir /srv/claude")).is_ok());
        assert!(check(&cfg("npx -y @anthropic-ai/claude-code@1")).is_err());
        assert!(check(&cfg("npx -y --yes @anthropic-ai/claude-code@1")).is_err());
        for command in [
            "python3 -X utf8 -u '/fixtures/com espaço.py'",
            "copilot --acp --stdio",
            "grok agent stdio",
            "uvx pacote",
            "python3 -m pacote",
            "~/bin/agente --acp",
            "droid exec --output-format acp-daemon",
            "node /adapter/index.js literal",
            "npx -y @google/gemini-cli --experimental-acp",
            "codex-acp",
            "npx -y @agentclientprotocol/codex-acp@2.2.2",
        ] {
            assert!(check(&cfg(command)).is_ok(), "{command}");
        }
    }

    /// Só o binário conhecido de um motor retirado é recusado; qualquer outro executável segue
    /// pelas regras comuns do comando próprio, sem declaração de bloqueio arbitrário.
    #[test]
    fn comando_proprio_que_abre_binario_de_motor_retirado_e_recusado_em_qualquer_backend() {
        const MSG: &str = "Este comando abre um motor que não está disponível nesta versão da Mexerica.";
        for backend in ["custom", "codex", "claude"] {
            for command in [
                "kilo",
                "/caminho/qualquer/kilo",
                "junie --acp=true",
                "cline --acp",
                "goose acp",
                "amp login",
                "mcode acp",
                "qwen --acp",
                "vibe",
                "vibe-acp",
                "OMP",
                "pi",
                "Kilo.EXE",
            ] {
                let cfg: BotConfig =
                    serde_json::from_value(json!({"id":"a", "name":"Teste", "backend":backend, "command":command}))
                        .unwrap();
                let erro = check(&cfg).unwrap_err().to_string();
                assert_eq!(erro, MSG, "{backend}: {command}");
            }
        }
        for command in ["meu-agente --acp", "codex-acp", "grok agent stdio"] {
            let cfg: BotConfig =
                serde_json::from_value(json!({"id":"a", "name":"Teste", "backend":"custom", "command":command}))
                    .unwrap();
            check(&cfg).unwrap_or_else(|e| panic!("{command}: {e}"));
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
