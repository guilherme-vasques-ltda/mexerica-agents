//! Recusa políticas locais antes de iniciar tarefas de fundo.
use anyhow::Result;
#[cfg(any(test, target_os = "macos"))]
use anyhow::{Context, ensure};
#[cfg(any(test, target_os = "macos", target_os = "linux"))]
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(any(test, target_os = "macos"))]
fn claude_files(base: &Path) -> Result<()> {
    use std::io::ErrorKind;
    // O diretório de fragmentos e links simbólicos também são recusados, mesmo vazios ou quebrados.
    match std::fs::symlink_metadata(base) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            anyhow::bail!("Pasta de política Claude não pode ser verificada.");
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("consultando pasta de política Claude"),
    }
    for name in ["managed-settings.json", "managed-settings.d", "managed-mcp.json"] {
        match std::fs::symlink_metadata(base.join(name)) {
            Ok(_) => anyhow::bail!("Política local do Claude impede a tarefa de fundo."),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("consultando política local do Claude"),
        }
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
fn check_mdm(result: Result<bool>) -> Result<()> {
    ensure!(
        !result.context("não foi possível consultar as políticas gerenciadas")?,
        "Política gerenciada impede a tarefa de fundo."
    );
    Ok(())
}

#[cfg(target_os = "macos")]
const CODEX_QUERY: &str = r#"
ObjC.import('CoreFoundation');
const d=ObjC.wrap('com.openai.codex');
if (!$.CFPreferencesAppSynchronize(d)) throw Error('sincronização falhou');
const states=[];
for (const name of ['config_toml_base64','requirements_toml_base64']) {
  const k=ObjC.wrap(name);
  if ($.CFPreferencesAppValueIsForced(k,d)) {
    const value=$.CFPreferencesCopyAppValue(k,d);
    if (!value) throw Error('consulta incompleta');
    states.push(1); $.CFRelease(value);
  } else { states.push(0); }
}
states.join(',');
"#;

#[cfg(target_os = "macos")]
const CLAUDE_ENUMERATION: &str = r#"
function hasForced(keys, isForced) {
  const count=Number(keys.count);
  if (!Number.isSafeInteger(count) || count < 0) throw Error('contagem inválida');
  for (let i=0; i<count; i++) {
    if (isForced(keys.objectAtIndex(i))) return true;
  }
  return false;
}
"#;

#[cfg(target_os = "macos")]
const CLAUDE_QUERY: &str = r#"
ObjC.import('Foundation'); ObjC.import('CoreFoundation');
const name='com.anthropic.claudecode', domain=ObjC.wrap(name);
if (!$.CFPreferencesAppSynchronize(domain)) throw Error('sincronização falhou');
const defaults=$.NSUserDefaults.alloc.initWithSuiteName(name);
if (!defaults) throw Error('consulta indisponível');
const representation=defaults.dictionaryRepresentation;
if (!representation) throw Error('consulta incompleta');
const keys=representation.allKeys;
if (!keys) throw Error('chaves indisponíveis');
hasForced(keys, key => defaults.objectIsForcedForKeyInDomain(key,name)) ? 'gerenciada' : 'livre';
"#;

#[cfg(target_os = "macos")]
async fn enforce_query(script: &str, codex: bool) -> Result<()> {
    check_mdm(query_script(script, codex).await)
}

#[cfg(target_os = "macos")]
async fn query_script(script: &str, codex: bool) -> Result<bool> {
    let mut command = tokio::process::Command::new("/usr/bin/osascript");
    command.args(["-l", "JavaScript", "-e", script]).env_clear().kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
        .await
        .context("consulta de políticas excedeu o prazo")??;
    ensure!(output.status.success(), "consulta de políticas falhou");
    parse_query(codex, &output.stdout)
}

#[cfg(any(test, target_os = "macos"))]
fn parse_query(codex: bool, output: &[u8]) -> Result<bool> {
    match (codex, output) {
        (true, b"0,0\n") | (false, b"livre\n") => Ok(false),
        (true, b"1,0\n" | b"0,1\n" | b"1,1\n") | (false, b"gerenciada\n") => Ok(true),
        _ => anyhow::bail!("resposta desconhecida da consulta de políticas"),
    }
}

#[cfg(target_os = "macos")]
pub(crate) async fn enforce(codex: bool) -> Result<()> {
    if !codex {
        claude_files(Path::new("/Library/Application Support/ClaudeCode"))?;
    }
    let script = if codex { CODEX_QUERY.to_owned() } else { format!("{CLAUDE_ENUMERATION}\n{CLAUDE_QUERY}") };
    enforce_query(&script, codex).await
}

#[cfg(any(test, target_os = "linux"))]
pub(crate) fn reject_policy_path(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(crate::fork::background_notice::Problem::Policy.into()),
    }
}

/// Variável de ambiente (honrada só em builds com `debug_assertions`) que
/// aponta a raiz das pastas de política — `/etc` em produção — a uma pasta
/// do teste, para o binário que os testes e2e sobem.
#[cfg(unix)]
pub(crate) const POLICY_ROOT_ENV: &str = "DONNABOT_TEST_POLICY_ROOT";

/// Único ponto que resolve as raízes de política `/etc/codex` e
/// `/etc/claude-code`. Em produção a raiz é `/etc` fixo; testes unitários
/// usam a costura `#[cfg(test)]` e testes e2e a `POLICY_ROOT_ENV`.
#[cfg(unix)]
pub(crate) fn policy_path(codex: bool) -> PathBuf {
    policy_root().join(if codex { "codex" } else { "claude-code" })
}

#[cfg(unix)]
fn policy_root() -> PathBuf {
    #[cfg(test)]
    if let Some(root) = TEST_POLICY_ROOT.read().expect("raiz de política de teste").clone() {
        return root;
    }
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os(POLICY_ROOT_ENV).filter(|root| !root.is_empty()) {
        return PathBuf::from(root);
    }
    PathBuf::from("/etc")
}

#[cfg(all(test, unix))]
static TEST_POLICY_ROOT: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);
#[cfg(all(test, unix))]
static TEST_POLICY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Aponta a raiz de política a `dir` até o guard sair de escopo. O cadeado
/// serializa os testes que mudam a raiz entre si e com o de `POLICY_ROOT_ENV`.
#[cfg(all(test, unix))]
pub(crate) fn test_policy_root(dir: &Path) -> PolicyRootGuard {
    let guard = TEST_POLICY_LOCK.lock().expect("serialização da raiz de política");
    *TEST_POLICY_ROOT.write().expect("raiz de política de teste") = Some(dir.to_path_buf());
    PolicyRootGuard { _guard: guard }
}

#[cfg(all(test, unix))]
pub(crate) struct PolicyRootGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(all(test, unix))]
impl Drop for PolicyRootGuard {
    fn drop(&mut self) {
        *TEST_POLICY_ROOT.write().expect("raiz de política de teste") = None;
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn enforce(codex: bool) -> impl Future<Output = Result<()>> {
    std::future::ready(reject_policy_path(&policy_path(codex)))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
pub(crate) fn enforce(_codex: bool) -> impl Future<Output = Result<()>> {
    std::future::ready(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn osascript_retoma_resultado_pelo_stdout() {
        assert!(!query_script("'0,0';", true).await.unwrap());
        assert!(query_script("'1,0';", true).await.unwrap());
        assert!(!query_script("'livre';", false).await.unwrap());
        assert!(query_script("throw Error('falha');", true).await.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn chave_forcada_simulada_impede_spawn() {
        let witness = std::env::temp_dir().join(format!("donnabot-mdm-{}", uuid::Uuid::new_v4()));
        let simulate = |forced: bool| {
            format!(
                r"{CLAUDE_ENUMERATION}
ObjC.import('Foundation');
const keys=$.NSArray.arrayWithArray(['outra-chave','chave-simulada']);
hasForced(keys, key => {forced} && ObjC.unwrap(key) === 'chave-simulada') ? 'gerenciada' : 'livre';"
            )
        };
        let spawn = || {
            assert!(std::process::Command::new("/usr/bin/touch").arg(&witness).status().unwrap().success());
        };
        let clean = query_script(&simulate(false), false).await.unwrap();
        assert!(!clean);
        let allowed = enforce_query(&simulate(false), false).await;
        if allowed.is_ok() {
            spawn();
        }
        assert!(witness.exists(), "controle positivo não iniciou o processo simulado");
        std::fs::remove_file(&witness).unwrap();
        let managed = query_script(&simulate(true), false).await.unwrap();
        assert!(managed, "a consulta não identificou a chave forçada");
        let denied = enforce_query(&simulate(true), false).await;
        if denied.is_ok() {
            spawn();
        }
        assert!(denied.is_err());
        assert!(!witness.exists());
        for invalid in ["NaN", "-1", "1.5"] {
            let script = format!(
                r"{CLAUDE_ENUMERATION}
hasForced({{count:{invalid}, objectAtIndex: _ => 'hooks'}}, _ => false) ? 'gerenciada' : 'livre';"
            );
            assert!(check_mdm(query_script(&script, false).await).is_err());
        }
    }

    #[test]
    fn consulta_mdm_recusa_presenca_e_erro() {
        assert!(check_mdm(Ok(false)).is_ok());
        assert!(check_mdm(Ok(true)).is_err());
        assert!(check_mdm(Err(anyhow::anyhow!("consulta falhou"))).is_err());
        assert!(check_mdm(parse_query(true, b"0,0\n")).is_ok());
        assert!(check_mdm(parse_query(true, b"1,0\n")).is_err());
        assert!(check_mdm(parse_query(true, b"0,1\n")).is_err());
        assert!(check_mdm(parse_query(true, b"erro\n")).is_err());
    }

    #[test]
    fn politica_local_impede_hook_antes_do_spawn() {
        let root = std::env::temp_dir().join(format!("claude-politica-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let witness = root.join("efeito");
        for name in ["managed-settings.json", "managed-settings.d", "managed-mcp.json"] {
            let path = root.join(name);
            let is_directory = name == "managed-settings.d";
            if is_directory {
                std::fs::create_dir(&path).unwrap();
            }
            let policy = if is_directory { path.join("hook.json") } else { path.clone() };
            let script = format!("printf efeito > '{}'", witness.display());
            let body = serde_json::json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":script}]}]}});
            std::fs::write(&policy, body.to_string()).unwrap();
            let execute = || {
                let raw = std::fs::read_to_string(&policy).unwrap();
                let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
                let command = parsed["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str().unwrap();
                assert!(std::process::Command::new("sh").args(["-c", command]).status().unwrap().success());
            };
            execute();
            assert!(witness.exists(), "controle positivo não executou o hook");
            std::fs::remove_file(&witness).unwrap();
            if claude_files(&root).is_ok() {
                execute();
            }
            assert!(!witness.exists(), "hook foi executado com {name}");
            std::fs::remove_file(policy).unwrap();
            if is_directory {
                std::fs::remove_dir(path).unwrap();
            }
        }
        assert!(claude_files(&root).is_ok());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("alvo-inexistente"), root.join("managed-settings.json")).unwrap();
            assert!(claude_files(&root).is_err());
            std::fs::remove_file(root.join("managed-settings.json")).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }

    /// A raiz de política só cede à variável de ambiente em build com
    /// `debug_assertions`; em release ela é ignorada e `/etc` continua fixo.
    #[cfg(unix)]
    #[test]
    fn raiz_de_politica_so_cede_a_variavel_em_debug() {
        let _serial = TEST_POLICY_LOCK.lock().expect("serialização da raiz de política");
        let dir = std::env::temp_dir().join(format!("politica-env-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        // SAFETY: só este teste e os que seguram TEST_POLICY_LOCK tocam na
        // variável ou na raiz; nenhum deles corre enquanto o cadeado está aqui.
        unsafe { std::env::set_var(POLICY_ROOT_ENV, &dir) };
        let resolved = policy_root();
        // SAFETY: mesma serialização do set_var acima.
        unsafe { std::env::remove_var(POLICY_ROOT_ENV) };
        std::fs::remove_dir(&dir).unwrap();
        #[cfg(debug_assertions)]
        assert_eq!(resolved, dir);
        #[cfg(not(debug_assertions))]
        assert_eq!(resolved, Path::new("/etc"));
    }

    /// A recusa de política continua valendo com a raiz falsa: cada forma de
    /// política gerenciada do Claude e o `/etc/codex` até vazio bloqueiam a
    /// tarefa de fundo antes do spawn.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn raiz_falsa_continua_recusando_politicas() {
        let root = std::env::temp_dir().join(format!("politica-recusa-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let _root = test_policy_root(&root);
        let claude = root.join("claude-code");
        assert!(enforce(false).await.is_ok(), "raiz limpa não bloqueia");
        assert!(enforce(true).await.is_ok(), "raiz limpa não bloqueia");
        for name in ["managed-settings.json", "managed-settings.d", "managed-mcp.json"] {
            std::fs::create_dir(&claude).unwrap();
            let artifact = claude.join(name);
            if name == "managed-settings.d" {
                std::fs::create_dir(&artifact).unwrap();
            } else {
                std::fs::write(&artifact, "{}").unwrap();
            }
            assert!(enforce(false).await.is_err(), "{name} aceito");
            assert!(
                crate::fork::background_isolation::Isolation::prepare(&mut Vec::new(), false, false).is_err(),
                "{name} aceito no isolamento"
            );
            std::fs::remove_dir_all(&claude).unwrap();
        }
        let target = root.join("alvo");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &claude).unwrap();
        assert!(enforce(false).await.is_err(), "link simbólico aceito");
        std::fs::remove_file(&claude).unwrap();
        std::os::unix::fs::symlink(root.join("alvo-inexistente"), &claude).unwrap();
        assert!(enforce(false).await.is_err(), "link quebrado aceito");
        std::fs::remove_file(&claude).unwrap();
        std::fs::create_dir(root.join("codex")).unwrap();
        assert!(enforce(true).await.is_err(), "/etc/codex vazio aceito");
        assert!(
            crate::fork::background_isolation::Isolation::prepare(&mut Vec::new(), true, true).is_err(),
            "/etc/codex vazio aceito no isolamento"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
