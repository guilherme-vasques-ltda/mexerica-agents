//! MEX-1292 (fatia 1): "doutor" de skills, só leitura. As skills de um bot vêm só do harness da
//! Mexerica (catálogo do Hub, seleção do dono, versão fixa). Este módulo mede o conjunto entregue
//! e aponta as pastas e arquivos nativos dos motores que ainda poderiam vazar para o bot.
//! Nunca lê conteúdo, nunca segue symlink e nunca apaga nada; bloquear fica para a fatia seguinte.
//! Toda sondagem parte de diretórios abertos sem seguir links (`motor_exec::fd`), só dentro da home
//! efetiva do motor ou da pasta do bot (a de lançamento); o que não dá para provar vira "não verificável"
//! e o que só pode conter regras vira "possível", nunca "limpo".
use crate::{api::devices::Caller, hub::Hub};
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::{Component, Path, PathBuf},
};

/// A fonte nomeada existe (arquivo ou pasta de skills/regras): o motor a lê.
pub(crate) const VAZANDO: &str = "nativa_vazando";
/// Existe algo que PODE conter regras ou skills, sem conteúdo confirmado (pasta inteira lida por cautela,
/// link não seguido, componente do caminho que é link).
pub(crate) const POSSIVEL: &str = "nativa_possivel";
/// A conferência falhou (permissão, erro de leitura): não se sabe se existe.
pub(crate) const NAO_VERIFICAVEL: &str = "nao_verificavel";

/// De onde o caminho relativo parte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Raiz {
    /// Só na home efetiva do motor.
    Home,
    /// `CODEX_HOME` quando definido; senão `~/.codex`.
    CodexHome,
    /// Pasta de trabalho do bot e cada ancestral até a home (inclusive).
    Projeto,
}
use Raiz::{CodexHome, Home, Projeto};

type Entradas = &'static [(Raiz, &'static str)];

/// O que cada motor lê de skills e regras nativas. Mantido junto de docs/features/skills-e-motores.md,
/// que traz o link da doc oficial de cada linha. Fonte sem confirmação na doc fica em `NAO_COBERTAS`.
const CLAUDE: Entradas = &[
    (Home, ".claude/skills"),
    (Home, ".claude/CLAUDE.md"),
    (Home, ".claude/rules"),
    (Home, ".claude/commands"),
    (Projeto, ".claude/skills"),
    (Projeto, "CLAUDE.md"),
    (Projeto, ".claude/CLAUDE.md"),
    (Projeto, "CLAUDE.local.md"),
    (Projeto, ".claude/rules"),
    (Projeto, ".claude/commands"),
];
const CODEX: Entradas = &[
    (CodexHome, "skills"),
    (CodexHome, "AGENTS.md"),
    (CodexHome, "AGENTS.override.md"),
    (Home, ".agents/skills"),
    (Projeto, ".agents/skills"),
    (Projeto, ".codex/skills"),
    (Projeto, "AGENTS.md"),
    (Projeto, "AGENTS.override.md"),
];
const OPENCODE: Entradas = &[
    (Home, ".config/opencode"),
    (Home, ".claude/CLAUDE.md"),
    (Home, ".claude/skills"),
    (Home, ".agents/skills"),
    (Projeto, ".opencode"),
    (Projeto, "AGENTS.md"),
    (Projeto, "CLAUDE.md"),
    (Projeto, ".claude/skills"),
    (Projeto, ".agents/skills"),
];
const DEVIN: Entradas = &[(Projeto, ".dsh/skills"), (Projeto, ".agents/skills"), (Projeto, "AGENTS.md")];
const GEMINI: Entradas = &[(Home, ".gemini/GEMINI.md"), (Projeto, "GEMINI.md"), (Projeto, ".gemini")];

const CLAUDE_NAO_COBERTAS: &[&str] = &[
    "Claude Code: política gerenciada (/etc/claude-code e equivalentes) fica fora da home e da pasta do bot",
    "Claude Code: skills de plugins, de --add-dir e aninhadas em subpastas",
];
const CODEX_NAO_COBERTAS: &[&str] = &[
    "Codex: nomes extras de project_doc_fallback_filenames no config.toml",
    "Codex: .codex/skills e .agents/skills sem confirmação por versão na doc oficial",
];
const OPENCODE_NAO_COBERTAS: &[&str] = &[
    "OpenCode: arquivos e URLs do array `instructions` do opencode.json",
    "OpenCode: pastas nativas de skills (a doc oficial lista só as do Claude Code)",
];
const DEVIN_NAO_COBERTAS: &[&str] = &["Devin: nenhuma fonte confirmada em doc oficial"];
const GEMINI_NAO_COBERTAS: &[&str] = &[
    "Gemini/Antigravity: GEMINI.md just-in-time em subpastas",
    "Gemini/Antigravity: skills, comandos e regras dentro de .gemini/ (a doc oficial não lista)",
];

struct Mapa {
    mapeado: bool,
    entradas: Vec<(Raiz, &'static str)>,
    nao_cobertas: Vec<&'static str>,
}

/// Pastas lidas inteiras por cautela: podem ter só configuração, então a presença não confirma regras.
fn por_cautela(rel: &str) -> bool {
    matches!(rel, ".config/opencode" | ".opencode" | ".gemini")
}

/// Motor sem mapa: fail-closed — procura em todos os lugares conhecidos e nunca fica limpo.
fn mapa(motor: &str) -> Mapa {
    let uma = |entradas: Entradas, nao_cobertas: &[&'static str]| Mapa {
        mapeado: true,
        entradas: entradas.to_vec(),
        nao_cobertas: nao_cobertas.to_vec(),
    };
    match motor {
        "claude" => uma(CLAUDE, CLAUDE_NAO_COBERTAS),
        "codex" => uma(CODEX, CODEX_NAO_COBERTAS),
        "opencode" => uma(OPENCODE, OPENCODE_NAO_COBERTAS),
        "devin" => uma(DEVIN, DEVIN_NAO_COBERTAS),
        "gemini" | "antigravity" | "agy" => uma(GEMINI, GEMINI_NAO_COBERTAS),
        _ => {
            let mut vistos = BTreeSet::new();
            Mapa {
                mapeado: false,
                entradas: [CLAUDE, CODEX, OPENCODE, DEVIN, GEMINI]
                    .concat()
                    .into_iter()
                    .filter(|e| vistos.insert(*e))
                    .collect(),
                nao_cobertas: Vec::new(),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tipo {
    Pasta,
    Arquivo,
    /// Não seguido: o alvo pode estar fora da pasta e não é olhado.
    Symlink,
    /// Existe algo que não pôde ser conferido (permissão etc.).
    Indeterminado,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Achado {
    /// Nunca absoluto: `~/…` na home do motor, `./…` na pasta do bot, `../…` nos ancestrais dela.
    pub(crate) caminho: String,
    pub(crate) tipo: Tipo,
    /// `nativa_vazando` (fonte confirmada), `nativa_possivel` (pode conter regras ou skills, sem confirmar)
    /// ou `nao_verificavel` (a conferência falhou); todos tiram o `limpo`.
    pub(crate) estado: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) motivo: Option<&'static str>,
}

/// Raízes efetivas do motor. `None` = não se sabe qual é: nada é afirmado sobre elas.
pub(crate) struct Raizes {
    pub(crate) home: Option<PathBuf>,
    pub(crate) codex_home: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct Diagnostico {
    pub(crate) mapeado: bool,
    pub(crate) achados: Vec<Achado>,
    /// Por que o resultado não pode ser afirmado limpo, além dos achados.
    pub(crate) motivos: Vec<String>,
}

impl Diagnostico {
    pub(crate) fn limpo(&self) -> bool {
        self.achados.is_empty() && self.motivos.is_empty()
    }
}

fn tem_volta(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

/// Pasta de trabalho e ancestrais até a home (inclusive). Fora da home só a própria pasta: o resto
/// não é nem da home nem do bot e não é tocado.
fn bases(cwd: &Path, home: Option<&Path>) -> (Vec<PathBuf>, Option<&'static str>) {
    let Some(home) = home else {
        return (vec![cwd.to_path_buf()], Some("home efetiva do motor não verificável"));
    };
    if !cwd.starts_with(home) {
        return (vec![cwd.to_path_buf()], Some("pasta do bot fora da home: pastas acima não verificáveis"));
    }
    let mut out = Vec::new();
    let mut atual = Some(cwd);
    while let Some(dir) = atual {
        out.push(dir.to_path_buf());
        if dir == home {
            break;
        }
        atual = dir.parent();
    }
    (out, None)
}

/// Caminho para o cliente, sem nome de usuário nem pasta pessoal.
fn exibir(path: &Path, home: Option<&Path>, cwd: &Path) -> String {
    let resto = |r: &Path, prefixo: &str| {
        if r.as_os_str().is_empty() { prefixo.to_owned() } else { format!("{prefixo}/{}", r.display()) }
    };
    if let Ok(r) = path.strip_prefix(cwd) {
        return resto(r, ".");
    }
    if let Some(home) = home
        && let Ok(r) = path.strip_prefix(home)
        && (path == home || !cwd.starts_with(path))
    {
        return resto(r, "~");
    }
    match cwd.strip_prefix(path) {
        Ok(r) => vec![".."; r.components().count()].join("/"),
        Err(_) => "(fora do escopo)".to_owned(),
    }
}

#[cfg(unix)]
pub(crate) mod ancorado {
    //! `openat`/`fstatat` com `O_NOFOLLOW`/`AT_SYMLINK_NOFOLLOW`: nenhum componente segue link e cada passo
    //! é relativo a um diretório já aberto, então trocar um nome por link entre a conferência e o uso falha.
    use super::Tipo;
    use crate::fork::motor_exec::fd::Directory;
    use std::{
        ffi::{CString, OsStr},
        io::ErrorKind,
        os::fd::AsRawFd,
        path::Path,
    };

    pub(super) enum Base {
        Aberta(Directory),
        Ausente,
        /// Link, não-diretório, permissão ou outro erro em algum componente.
        NaoVerificavel,
    }

    pub(super) fn abrir(path: &Path) -> Base {
        match Directory::open(path) {
            Ok(dir) => Base::Aberta(dir),
            Err(error)
                if error
                    .chain()
                    .filter_map(|e| e.downcast_ref::<std::io::Error>())
                    .any(|e| e.kind() == ErrorKind::NotFound) =>
            {
                Base::Ausente
            }
            Err(_) => Base::NaoVerificavel,
        }
    }

    /// `Ok(None)`: não existe. `Err`: existe algo ou falhou de um jeito que não prova ausência.
    fn tipo_em(dir: &Directory, nome: &str) -> Result<Option<Tipo>, ()> {
        let nome = CString::new(nome).map_err(|_| ())?;
        // SAFETY: `stat` é só dados; zerado é um valor válido e fstatat o preenche.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fd vivo, nome terminado em NUL de um único componente, sem seguir o último link.
        let rc = unsafe { libc::fstatat(dir.0.as_raw_fd(), nome.as_ptr(), &raw mut st, libc::AT_SYMLINK_NOFOLLOW) };
        if rc != 0 {
            let erro = std::io::Error::last_os_error();
            return if erro.kind() == ErrorKind::NotFound || erro.raw_os_error() == Some(libc::ENOTDIR) {
                Ok(None)
            } else {
                Err(())
            };
        }
        let formato = st.st_mode & libc::S_IFMT;
        Ok(Some(if formato == libc::S_IFLNK {
            Tipo::Symlink
        } else if formato == libc::S_IFDIR {
            Tipo::Pasta
        } else if formato == libc::S_IFREG {
            Tipo::Arquivo
        } else {
            Tipo::Indeterminado
        }))
    }

    /// Resultado de sondar um caminho relativo.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Sonda {
        Nada,
        /// Tipo do que existe e quantos componentes de `rel` formam o caminho achado (parou no primeiro link).
        Achou(Tipo, usize),
        /// A conferência falhou no componente de ordem `n` (1 = primeiro): não prova ausência nem presença.
        Falhou(usize),
    }

    pub(super) fn sondar(base: &Directory, rel: &str) -> Sonda {
        let partes: Vec<&str> = rel.split('/').collect();
        let mut aberta: Option<Directory> = None;
        for (i, parte) in partes.iter().enumerate() {
            let dir = aberta.as_ref().unwrap_or(base);
            let ultimo = i + 1 == partes.len();
            match tipo_em(dir, parte) {
                Ok(None) => return Sonda::Nada,
                Err(()) => return Sonda::Falhou(i + 1),
                Ok(Some(Tipo::Pasta)) if !ultimo => match dir.child(OsStr::new(parte), None) {
                    Ok(filha) => aberta = Some(filha),
                    Err(_) => return Sonda::Falhou(i + 1),
                },
                Ok(Some(Tipo::Arquivo | Tipo::Indeterminado)) if !ultimo => return Sonda::Nada,
                Ok(Some(tipo)) => return Sonda::Achou(tipo, i + 1),
            }
        }
        Sonda::Nada
    }
}

#[cfg(not(unix))]
pub(crate) mod ancorado {
    //! Sem travessia ancorada fora do Unix: nada é afirmado.
    use super::Tipo;
    use std::path::Path;

    pub(super) struct Directory;
    pub(super) enum Base {
        Aberta(Directory),
        #[allow(dead_code)]
        Ausente,
        NaoVerificavel,
    }
    pub(super) fn abrir(_: &Path) -> Base {
        Base::NaoVerificavel
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Sonda {
        Nada,
        #[allow(dead_code)]
        Achou(Tipo, usize),
        #[allow(dead_code)]
        Falhou(usize),
    }
    pub(super) fn sondar(_: &Directory, _: &str) -> Sonda {
        Sonda::Nada
    }
}
use ancorado::{Base, Sonda};

/// Tipo que o motor lê em cada fonte: arquivo para `*.md`, pasta para as demais.
#[allow(clippy::case_sensitive_file_extension_comparisons)] // Os nomes das fontes são sensíveis a maiúsculas.
fn esperado(rel: &str) -> Tipo {
    if rel.ends_with(".md") { Tipo::Arquivo } else { Tipo::Pasta }
}

/// Só a fonte nomeada, encontrada por inteiro e com o tipo esperado, é `nativa_vazando`. Falha de
/// conferência é `nao_verificavel`; pasta lida por cautela, link (não seguido), componente intermediário
/// que é link, tipo incompatível e arquivo especial são `nativa_possivel`.
pub(crate) fn classificar(onde: &str, rel: &str, sonda: Sonda) -> Option<Achado> {
    let partes: Vec<&str> = rel.split('/').collect();
    let caminho = |n: usize| format!("{onde}/{}", partes[..n].join("/"));
    match sonda {
        Sonda::Nada => None,
        Sonda::Falhou(n) => Some(Achado {
            caminho: caminho(n),
            tipo: Tipo::Indeterminado,
            estado: NAO_VERIFICAVEL,
            motivo: Some("a conferência falhou (permissão ou erro de leitura): não se sabe se existe"),
        }),
        Sonda::Achou(tipo, n) => {
            let (estado, motivo) = if n < partes.len() || tipo == Tipo::Symlink {
                (POSSIVEL, Some("link não seguido: o conteúdo não foi confirmado"))
            } else if tipo != esperado(rel) {
                (POSSIVEL, Some("tipo diferente do esperado (arquivo ou pasta): não se sabe se o motor lê"))
            } else if por_cautela(rel) {
                (POSSIVEL, Some("pasta lida inteira pelo motor: pode ter só configuração, sem regras nem skills"))
            } else {
                (VAZANDO, None)
            };
            Some(Achado { caminho: caminho(n), tipo, estado, motivo })
        }
    }
}

/// Pastas nativas que o motor leria e que existem, mais tudo que não pôde ser provado.
/// Ordem estável, sem duplicatas.
pub(crate) fn diagnosticar(motor: &str, cwd: &Path, raizes: &Raizes) -> Result<Diagnostico> {
    ensure!(cwd.is_absolute() && !tem_volta(cwd), "invalid_field: pasta de trabalho do bot precisa ser absoluta.");
    let Mapa { mapeado, entradas, nao_cobertas } = mapa(motor);
    let mut motivos = Vec::new();
    if !mapeado {
        motivos.push("motor não mapeado".to_owned());
    }
    motivos.extend(nao_cobertas.iter().map(|fonte| format!("fonte a confirmar, não coberta: {fonte}")));
    let mut achados = Vec::new();
    let home = raizes.home.as_deref().filter(|h| h.is_absolute() && !tem_volta(h));
    let (projetos, aviso) = bases(cwd, home);
    motivos.extend(aviso.map(str::to_owned));
    let usa_codex_home = entradas.iter().any(|(raiz, _)| *raiz == CodexHome);
    let codex_home = match &raizes.codex_home {
        Some(p)
            if p.is_absolute() && !tem_volta(p) && (home.is_some_and(|h| p.starts_with(h)) || p.starts_with(cwd)) =>
        {
            Some(p.clone())
        }
        Some(_) => {
            if usa_codex_home {
                achados.push(Achado {
                    caminho: "CODEX_HOME".to_owned(),
                    tipo: Tipo::Indeterminado,
                    estado: NAO_VERIFICAVEL,
                    motivo: Some("fora da home e da pasta do bot, relativo ou com '..'"),
                });
            }
            None
        }
        None => home.map(|h| h.join(".codex")),
    };
    let mut alvos = BTreeSet::new();
    for (raiz, rel) in entradas {
        match raiz {
            Home => alvos.extend(home.map(|h| (h.to_path_buf(), rel))),
            CodexHome => alvos.extend(codex_home.iter().map(|c| (c.clone(), rel))),
            Projeto => alvos.extend(projetos.iter().map(|base| (base.clone(), rel))),
        }
    }
    let mut abertas: BTreeMap<PathBuf, Base> = BTreeMap::new();
    let mut vistos = BTreeSet::new();
    for (base, rel) in &alvos {
        let aberta = abertas.entry(base.clone()).or_insert_with(|| ancorado::abrir(base));
        let onde = exibir(base, home, cwd);
        match &*aberta {
            Base::Ausente => {}
            Base::NaoVerificavel => {
                if vistos.insert(onde.clone()) {
                    achados.push(Achado {
                        caminho: onde,
                        tipo: Tipo::Indeterminado,
                        estado: NAO_VERIFICAVEL,
                        motivo: Some("pasta base é link, não é pasta comum ou não pôde ser aberta sem seguir links"),
                    });
                }
            }
            Base::Aberta(dir) => {
                if let Some(achado) = classificar(&onde, rel, ancorado::sondar(dir, rel))
                    && vistos.insert(achado.caminho.clone())
                {
                    achados.push(achado);
                }
            }
        }
    }
    Ok(Diagnostico { mapeado, achados, motivos })
}

/// Digest do que a entrega anuncia: (id, versão fixa) das skills íntegras, em ordem de id.
pub(crate) fn digest(entregues: &[(String, String)]) -> String {
    let mut h = Sha256::new();
    for (id, versao) in entregues {
        h.update(id.as_bytes());
        h.update(b"\t");
        h.update(versao.as_bytes());
        h.update(b"\n");
    }
    h.finalize().iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// O que o harness entrega agora, pela mesma conferência da entrega. O digest só existe quando toda
/// skill selecionada entra no perfil; com qualquer problema (ou registro ilegível) fica `null`.
/// Com motor em usuário separado, a entrega só vale se a publicação da skill para o motor também der certo
/// (permissão, limite de 100 MiB). O doutor é só leitura e não publica, então não consegue provar essa etapa:
/// o digest fica `null` e o motivo aparece em `problemas`. A conferência local (registro e texto) segue valendo.
pub(crate) fn harness(store: &crate::store::Store, selecao: &[String], versao: Option<&str>) -> Value {
    harness_em(store, selecao, versao, super::motor_exec::configured())
}

const ENTREGA_NAO_PROVADA: &str = "motor separado: a publicação para o motor não é provada pelo doutor (só leitura)";

pub(crate) fn harness_em(
    store: &crate::store::Store,
    selecao: &[String],
    versao: Option<&str>,
    separado: bool,
) -> Value {
    match super::skill_catalog::conferir(store, selecao) {
        Ok(c) => {
            let mut problemas: Vec<Value> =
                c.problemas.iter().map(|(id, motivo)| json!({"id": id, "motivo": motivo})).collect();
            let prova_entrega = separado && !c.entregues.is_empty();
            if prova_entrega {
                problemas.push(json!({"id": "*", "motivo": ENTREGA_NAO_PROVADA}));
            }
            json!({
                "verificado": true,
                "digest": (c.problemas.is_empty() && !prova_entrega).then(|| digest(&c.entregues)),
                "selecaoVersao": versao,
                "skills": c.entregues.iter().map(|(id, v)| json!({"id": id, "versao": v})).collect::<Vec<_>>(),
                "problemas": problemas,
            })
        }
        Err(error) => {
            tracing::warn!(%error, "doutor de skills: registro não pôde ser conferido");
            json!({
                "verificado": false,
                "digest": null,
                "selecaoVersao": versao,
                "skills": [],
                "problemas": [{"id": "*", "motivo": "registro de skills não pôde ser conferido"}],
            })
        }
    }
}

/// Home e `CODEX_HOME` que o motor do bot realmente usa: os da conta isolada quando há, senão os do Hub.
/// Motor em usuário separado sem conta isolada tem home desconhecida para o Hub: `None`.
fn raizes_do_bot(hub: &Hub, cfg: &crate::store::BotConfig) -> Raizes {
    let conta = match super::engine_accounts::managed::resolve_for_bot(&hub.store, cfg) {
        Ok(conta) => conta,
        Err(error) => {
            tracing::warn!(%error, "doutor de skills: conta do bot indisponível");
            return Raizes { home: None, codex_home: None };
        }
    };
    let env =
        |nome: &str| conta.as_ref().and_then(|c| c.env.iter().find(|(k, _)| k == nome)).map(|(_, v)| PathBuf::from(v));
    let sem_conta = conta.is_none();
    Raizes {
        home: env("HOME").or_else(|| (!super::motor_exec::configured()).then(dirs::home_dir).flatten()),
        codex_home: env("CODEX_HOME").or_else(|| {
            let do_hub = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
            do_hub.filter(|_| sem_conta)
        }),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pedido {
    bot_id: String,
}

/// Mesma autoridade de dono da gestão de contas; só leitura.
pub(crate) fn call(hub: &Hub, caller: &Caller, body: &Value) -> Result<Value> {
    let autoridade = super::engine_setup::access::resolve(hub, caller)?;
    let pedido: Pedido =
        serde_json::from_value(body.clone()).map_err(|_| anyhow!("invalid_field: informe somente botId."))?;
    let row = hub
        .store
        .bot(&pedido.bot_id)?
        .filter(|row| !row.deleted)
        .ok_or_else(|| anyhow!("not_found: bot inexistente."))?;
    let cfg = &row.config;
    ensure!(
        !cfg.backend.is_empty() && !cfg.cwd.is_empty(),
        "invalid_field: grupos não têm motor nem pasta de trabalho."
    );
    let harness = harness(&hub.store, &cfg.skills, row.skills_version.as_deref());
    // A pasta sondada é a de execução do motor, pela mesma função do lançamento.
    let cwd = Path::new(&cfg.cwd);
    let efetivo = super::motor_exec::launch_cwd(cwd);
    let mut diagnostico = diagnosticar(&cfg.backend, efetivo, &raizes_do_bot(hub, cfg))?;
    if efetivo != cwd {
        diagnostico.motivos.push(
            "a pasta do bot é a home ou a pasta de dados do Hub: o motor roda na raiz do motor, que é a pasta sondada"
                .to_owned(),
        );
    }
    // Teste-porta: revogação depois da captura e antes da resposta.
    super::donna_fixa::pedido::antes_de_gravar(&hub.store);
    autoridade.require_current(hub)?;
    Ok(json!({
        "botId": cfg.id,
        "motor": cfg.backend,
        "motorMapeado": diagnostico.mapeado,
        "harness": harness,
        "limpo": diagnostico.limpo(),
        "motivosNaoLimpo": diagnostico.motivos,
        "nativas": diagnostico.achados,
    }))
}
