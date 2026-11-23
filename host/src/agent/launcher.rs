//! Validação do comando escolhido pelo dono. Evita overrides acidentais; não inspeciona código (D62/D90).

use crate::store::BotConfig;
use anyhow::{Result, anyhow, ensure};

fn name(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word).to_ascii_lowercase();
    name.strip_suffix(".exe").or_else(|| name.strip_suffix(".cmd")).unwrap_or(&name).to_owned()
}

fn literal_launch(words: &[String]) -> Option<&str> {
    let (executable, args) = words.split_first()?;
    let bin = name(executable);
    let args = match bin.as_str() {
        "npx" | "bunx" => args,
        "pnpm" if args.first().is_some_and(|arg| arg == "dlx") => &args[1..],
        _ => return Some(executable),
    };
    let mut args = args.iter().skip_while(|arg| matches!(arg.as_str(), "-y" | "--yes"));
    match args.next().map(String::as_str) {
        Some("--") => args.next().map(String::as_str),
        Some(package) if !package.starts_with('-') => Some(package),
        _ => None,
    }
}

fn check_known_launch(cfg: &BotConfig, palavras: &[String]) -> Result<()> {
    if let Some(executavel) = palavras.first() {
        let nome = name(executavel);
        let mut literal = palavras;
        if nome == "env" {
            literal = &literal[1..];
            while let Some((assignment, rest)) = literal.split_first()
                && assignment.split_once('=').is_some_and(|(key, _)| {
                    !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
            {
                literal = rest;
            }
            if literal.first().is_some_and(|word| word == "--") {
                literal = &literal[1..];
            }
        }
        let shell = literal.first().is_some_and(|word| matches!(name(word).as_str(), "sh" | "bash" | "zsh"))
            && literal.get(1).is_some_and(|flag| flag == "-c");
        let script = shell.then(|| literal.get(2)).flatten().filter(|script| {
            !script.contains([';', '|', '&', '$', '`', '<', '>', '\n', '\r', '(', ')', '#', '*', '?'])
        });
        let inner = script.and_then(|script| shlex::split(script));
        let target = literal_launch(inner.as_deref().unwrap_or(literal));
        for word in std::iter::once(executavel.as_str()).chain(target) {
            let bin = name(word.rsplit_once('@').filter(|(base, _)| !base.is_empty()).map_or(word, |(base, _)| base));
            ensure!(!crate::fork::assinatura_pessoal::muse(&bin), crate::fork::assinatura_pessoal::Refusal::Meta);
            ensure!(
                cfg.backend == "claude" || !crate::fork::assinatura_pessoal::claude(&bin),
                crate::fork::assinatura_pessoal::Refusal::Automation
            );
        }
        crate::agent::backends::require_supported_execution(&nome)?;
        let abre_retirado = crate::agent::backends::HARNESSES
            .iter()
            .any(|h| crate::fork::motores_retirados::retirado(h.id) && h.bins.contains(&nome.as_str()));
        ensure!(!abre_retirado, "Este comando abre um motor que não está disponível nesta versão da Mexerica.");
    }
    Ok(())
}

pub fn check(cfg: &BotConfig) -> Result<()> {
    if let Some(command) = cfg.command.as_deref().filter(|c| !c.trim().is_empty())
        && let Some(words) = shlex::split(command)
    {
        check_known_launch(cfg, &words)?;
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
