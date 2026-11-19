use super::*;
use crate::remote::identity::Identity;
use crate::store::Store;

struct Pastas {
    raiz: PathBuf,
    home: PathBuf,
    dados: PathBuf,
    projeto: PathBuf,
}

impl Pastas {
    fn novas() -> Self {
        let raiz = std::env::temp_dir().join(format!("donna-pasta-{}", uuid::Uuid::new_v4()));
        let home = raiz.join("home");
        let dados = home.join(".donnabot");
        let projeto = home.join("projeto");
        for pasta in [&projeto, &dados] {
            std::fs::create_dir_all(pasta).unwrap();
        }
        Self { raiz, home, dados, projeto }
    }

    fn validar(&self, pasta: &Path, bot: Option<&str>) -> Result<PathBuf> {
        validar_em(pasta.to_str().unwrap(), bot, &self.home, &self.dados)
    }

    fn criar(&self, caminho: &Path) -> PathBuf {
        assert!(caminho.starts_with(&self.raiz));
        std::fs::create_dir_all(caminho).unwrap();
        caminho.to_path_buf()
    }
}

impl Drop for Pastas {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.raiz).unwrap();
    }
}

#[test]
fn recusa_texto_invalido_relativo_arquivo_raiz_home_e_pastas_protegidas() {
    let f = Pastas::novas();
    for texto in [
        String::new(),
        " ".into(),
        "projeto".into(),
        "~outro/projeto".into(),
        "a".repeat(1025),
        "é".repeat(513),
        format!("{}\n", f.projeto.display()),
        format!("{}\u{0085}", f.projeto.display()),
    ] {
        assert!(validar_em(&texto, Some("bot"), &f.home, &f.dados).is_err(), "{texto:?}");
    }
    let arquivo = f.raiz.join("arquivo");
    std::fs::write(&arquivo, "não é pasta").unwrap();
    for pasta in [Path::new("/"), &f.raiz, &f.home, &f.dados, &arquivo, &f.raiz.join("ausente")] {
        assert!(f.validar(pasta, Some("bot")).is_err(), "{pasta:?}");
    }
    for nome in [".ssh", ".config", ".codex", ".claude", ".local", ".cache", ".gnupg", ".qualquer"] {
        let pasta = f.criar(&f.home.join(nome).join("dentro"));
        assert!(f.validar(pasta.parent().unwrap(), Some("bot")).is_err());
        assert!(f.validar(&pasta, Some("bot")).is_err());
    }
    for sistema in SISTEMA {
        if Path::new(sistema).is_dir() {
            assert!(f.validar(Path::new(sistema), Some("bot")).is_err(), "{sistema}");
        }
    }
    assert!(f.validar(&f.criar(&f.projeto.join(".git")), Some("bot")).is_ok(), "oculta só no primeiro nível");
    assert!(f.validar(&f.criar(&f.home.join("etc-projeto")), Some("bot")).is_ok());
    assert!(validar_em("~/..", None, &f.home, &f.dados).is_err());
    for pasta in ["/home", "/home/outro", "/home/outro/projeto", "/Users/outro", "/var"] {
        // Não cria pastas de outros usuários: a variante de criação aplica a mesma fronteira.
        assert!(validar_criacao_em(Path::new(pasta), &f.home, &f.dados).is_err(), "{pasta}");
    }
    assert!(validar_criacao_em(&f.home.join(".projetos/novo"), &f.home, &f.dados).is_err());
    assert!(validar_criacao_em(&f.home.join("trabalho/novo"), &f.home, &f.dados).is_ok());
}

#[test]
fn aceita_apenas_as_excecoes_gerenciadas_e_guarda_o_resultado_canonico() {
    let f = Pastas::novas();
    let pessoal = f.criar(&f.dados.join("bots/bot/workspace"));
    let grupo = f.criar(&f.dados.join("grupos/grupo/pasta"));
    for pasta in [&pessoal, &grupo, &f.criar(&grupo.join("arquivos")), &f.criar(&pessoal.join("arquivos"))] {
        assert_eq!(f.validar(pasta, Some("bot")).unwrap(), std::fs::canonicalize(pasta).unwrap());
    }
    assert!(f.validar(&grupo, None).is_ok());
    assert!(f.validar(&pessoal, Some("outro")).is_err());
    assert!(f.validar(&pessoal, None).is_err());
    for relativa in
        ["bots", "bots/bot", "bots/bot/memoria", "grupos", "grupos/grupo", "grupos/grupo/pasta-extra", "token"]
    {
        let pasta = f.criar(&f.dados.join(relativa));
        assert!(f.validar(&pasta, Some("bot")).is_err(), "{relativa}");
    }
    assert_eq!(
        validar_em("~/projeto/../projeto", None, &f.home, &f.dados).unwrap(),
        std::fs::canonicalize(&f.projeto).unwrap()
    );
    assert!(validar_em("~", None, &f.home, &f.dados).is_err());
}

#[cfg(unix)]
#[test]
fn links_nao_contornam_as_protecoes_nem_as_excecoes() {
    use std::os::unix::fs::symlink;
    let f = Pastas::novas();
    let segredo = f.criar(&f.home.join(".ssh/chaves"));
    let outro = f.criar(&f.dados.join("bots/outro/workspace"));
    for (i, alvo) in [Path::new("/"), &f.home, &f.dados, &segredo, &outro, Path::new("/etc")].into_iter().enumerate() {
        let link = f.projeto.join(format!("link-{i}"));
        symlink(alvo, &link).unwrap();
        assert!(f.validar(&link, Some("bot")).is_err(), "{alvo:?}");
    }
    f.criar(&f.dados.join("bots/bot"));
    let falso_workspace = f.dados.join("bots/bot/workspace");
    symlink(&segredo, &falso_workspace).unwrap();
    assert!(f.validar(&falso_workspace, Some("bot")).is_err());
    let interna = f.criar(&f.dados.join("interno"));
    f.criar(&f.dados.join("grupos/grupo"));
    let falsa_pasta = f.dados.join("grupos/grupo/pasta");
    symlink(&interna, &falsa_pasta).unwrap();
    assert!(f.validar(&falsa_pasta, None).is_err());
    let link = f.raiz.join("projeto-link");
    symlink(&f.projeto, &link).unwrap();
    assert_eq!(f.validar(&link, None).unwrap(), std::fs::canonicalize(&f.projeto).unwrap());
    let dados_link = f.raiz.join("dados-link");
    symlink(&f.dados, &dados_link).unwrap();
    assert!(validar_em(interna.to_str().unwrap(), None, &f.home, &dados_link).is_err());
    // A checagem não confia numa decisão anterior quando um link é trocado.
    std::fs::remove_file(&link).unwrap();
    symlink(&segredo, &link).unwrap();
    assert!(f.validar(&link, None).is_err());
    let controle = f.criar(&f.raiz.join("pasta\n"));
    let link = f.projeto.join("nome-sem-controle");
    symlink(&controle, &link).unwrap();
    assert!(f.validar(&link, None).is_err(), "o caminho canônico salvo também não pode conter controles");
}

fn bot(id: &str, cwd: &str) -> BotConfig {
    serde_json::from_value(json!({"id":id, "name":"Teste", "backend":"codex", "cwd":cwd})).unwrap()
}

#[test]
fn hub_valida_todos_os_bots_e_persiste_pastas_canonicas() {
    let f = Pastas::novas();
    for kind in [crate::store::BotKind::Agent, crate::store::BotKind::Group] {
        let mut cfg = bot("bot", "/");
        cfg.kind = kind;
        assert_eq!(
            crate::hub::validate(&mut cfg).unwrap_err().to_string(),
            "A pasta raiz não pode ser usada como pasta de trabalho."
        );
    }
    let mut cfg = bot("bot", f.projeto.join("../projeto").to_str().unwrap());
    crate::hub::validate(&mut cfg).unwrap();
    crate::agent::workspace::prepare(&mut cfg).unwrap();
    let store = Store::open(Path::new(":memory:")).unwrap();
    store.save_bot(&cfg).unwrap();
    assert_eq!(
        store.bot("bot").unwrap().unwrap().config.cwd,
        std::fs::canonicalize(&f.projeto).unwrap().to_str().unwrap()
    );
    let mut cfg = bot(&uuid::Uuid::new_v4().to_string(), "");
    crate::hub::validate(&mut cfg).unwrap();
    crate::agent::workspace::prepare(&mut cfg).unwrap();
    assert!(crate::agent::workspace::is_managed(&cfg));
    let legado = bot(&cfg.id, crate::agent::workspace::path(&cfg.id).to_str().unwrap());
    assert!(crate::agent::workspace::is_managed(&legado));
    assert!(aviso(&cfg).is_none());
    std::fs::remove_dir_all(crate::agent::workspace::path(&cfg.id).parent().unwrap()).unwrap();
}

#[tokio::test]
async fn bot_antigo_continua_carregando_avisa_no_chat_e_no_sync_sem_regravar_a_pasta() {
    let f = Pastas::novas();
    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg = bot("legado", "/");
    let rev = store.save_bot(&cfg).unwrap();
    let hub = Hub::new(store, "pasta-teste".into(), Identity::load_or_create(&f.raiz).unwrap(), "teste".into(), 0);
    hub.start().unwrap();
    let snapshot = hub.bots_json(0).unwrap();
    assert_eq!(snapshot.len(), 1);
    assert!(snapshot[0]["workspaceIssue"].as_str().unwrap().starts_with("Revise a pasta."));
    assert_eq!(snapshot[0]["cwd"], "/");
    assert_eq!(snapshot[0]["rev"], rev);
    avisar(&hub, &cfg);
    let entradas = hub.store.entries_since(0, 100).unwrap();
    assert_eq!(
        entradas.iter().filter(|e| e.data["text"].as_str().is_some_and(|t| t.starts_with("Revise a pasta."))).count(),
        1
    );
    let atualizado = hub
        .update_bot_by_user(&json!({"id":"legado", "cwd":"/",
        "permission":"ask", "permissionChosenByUser":true, "skills":[],
        "connectors":["conector"], "logins":["login"]}))
        .unwrap();
    assert_eq!(atualizado["permission"], "ask");
    assert_eq!(atualizado["skills"], json!([]));
    assert_eq!(atualizado["connectors"], json!(["conector"]));
    assert_eq!(atualizado["logins"], json!(["login"]));
    let atualizado = hub.update_bot_by_user(&json!({"id":"legado", "skills":[], "connectors":[]})).unwrap();
    assert_eq!(atualizado["skills"], json!([]));
    assert_eq!(atualizado["connectors"], json!([]));
    assert!(atualizado["workspaceIssue"].as_str().unwrap().starts_with("Revise a pasta."));
    assert_eq!(hub.store.bot("legado").unwrap().unwrap().config.cwd, "/");
    let corrigido = hub.update_bot_by_user(&json!({"id":"legado", "cwd":f.projeto})).unwrap();
    assert!(corrigido["workspaceIssue"].is_null());
    assert_eq!(corrigido["cwd"], std::fs::canonicalize(&f.projeto).unwrap().to_str().unwrap());
    assert!(hub.update_bot_by_user(&json!({"id":"legado", "cwd":"/"})).is_err(), "nova pasta insegura é recusada");
    let mut grupo = bot("grupo", "/");
    grupo.kind = crate::store::BotKind::Group;
    grupo.members = vec!["legado".into()];
    hub.store.save_bot(&grupo).unwrap();
    let grupo = hub.update_bot_by_user(&json!({"id":"grupo", "name":"Renomeado", "members":["legado"]})).unwrap();
    assert_eq!(grupo["name"], "Renomeado");
    assert_eq!(grupo["members"], json!(["legado"]));
    assert_eq!(grupo["cwd"], "/");
    assert!(grupo["workspaceIssue"].is_string());
    hub.shutdown().await;
}

#[tokio::test]
async fn patch_sem_mudanca_de_pasta_preserva_a_grafia_e_nao_induz_restart_por_cwd() {
    let f = Pastas::novas();
    let cwd = format!("{}/../projeto/", f.projeto.display());
    assert_ne!(cwd, std::fs::canonicalize(&f.projeto).unwrap().to_str().unwrap());
    let store = Store::open(Path::new(":memory:")).unwrap();
    store.save_bot(&bot("grafia", &cwd)).unwrap();
    let hub = Hub::new(store, "pasta-teste".into(), Identity::load_or_create(&f.raiz).unwrap(), "teste".into(), 0);
    hub.start().unwrap();
    for patch in [json!({"id":"grafia", "name":"Nome"}), json!({"id":"grafia", "cwd":cwd, "hidden":true})] {
        let updated = hub.update_bot_by_user(&patch).unwrap();
        assert_eq!(updated["cwd"], cwd, "Reconfigure não pode ver uma mudança incidental de cwd");
        assert!(updated["workspaceIssue"].is_null());
        assert_eq!(hub.store.bot("grafia").unwrap().unwrap().config.cwd, cwd);
    }
    hub.shutdown().await;
}

#[cfg(unix)]
#[test]
fn identidade_recusa_diretorios_sensiveis_mesmo_com_destino_fora_da_home() {
    let f = Pastas::novas();
    for nome in [".ssh", ".codex", ".claude", ".config", ".gnupg"] {
        let externa = f.criar(&f.raiz.join(format!("externa-{nome}")));
        std::os::unix::fs::symlink(&externa, f.home.join(nome)).unwrap();
        let dentro = f.criar(&externa.join("dentro"));
        // A grafia canônica externa não começa com HOME; só a identidade a protege.
        for caminho in [&externa, &dentro] {
            assert!(f.validar(caminho, None).is_err(), "{nome}: {caminho:?}");
            assert!(validar_criacao_em(&caminho.join("nova"), &f.home, &f.dados).is_err());
        }
    }
    assert!(f.validar(&f.projeto, None).is_ok(), "subpasta comum da home continua válida");
}

#[cfg(target_os = "macos")]
#[test]
fn alias_do_volume_data_nao_contorna_recusas_e_salva_caminho_principal() {
    let f = Pastas::novas();
    let alias = |pasta: &Path| Path::new("/System/Volumes/Data").join(pasta.strip_prefix("/").unwrap());
    for nome in [".ssh", ".codex", ".claude", ".config", ".gnupg"] {
        f.criar(&f.home.join(nome).join("dentro"));
    }
    let home = f.home.canonicalize().unwrap();
    let dados = f.dados.canonicalize().unwrap();
    let projeto = f.projeto.canonicalize().unwrap();
    for pasta in [&home, &dados, &home.join(".ssh/dentro"), Path::new("/Users"), Path::new("/")] {
        let caminho = alias(pasta);
        assert!(validar_em(caminho.to_str().unwrap(), None, &home, &dados).is_err(), "{caminho:?}");
        assert!(validar_criacao_em(&caminho, &home, &dados).is_err(), "{caminho:?}");
    }
    assert!(validar_criacao_em(&alias(Path::new("/Users/outro/novo")), &home, &dados).is_err());
    assert_eq!(validar_em(alias(&projeto).to_str().unwrap(), None, &home, &dados).unwrap(), projeto);
    assert_eq!(validar_criacao_em(&alias(&projeto.join("nova")), &home, &dados).unwrap(), projeto.join("nova"));
    assert!(validar_em("/System/Library", None, &home, &dados).is_err());
    assert!(validar_criacao_em(Path::new("/System/Library/nova"), &home, &dados).is_err());
    // A conferência de identidade reconhece o firmlink sem passar pela normalização.
    for protegida in [&home, &dados] {
        assert_eq!(
            relativa_protegida(&alias(&protegida.join("nova")), protegida).unwrap(),
            Some(PathBuf::from("nova"))
        );
        assert_eq!(
            relativa_protegida(&protegida.join("nova"), &alias(protegida)).unwrap(),
            Some(PathBuf::from("nova"))
        );
    }
    // A própria política também reconhece a home/dados por identidade, sem o prefixo textual.
    assert_eq!(
        validar_destino(alias(&home), None, &home, &dados).unwrap_err().to_string(),
        "A pasta pessoal inteira ou seus ancestrais não podem ser usados. Escolha uma subpasta de trabalho."
    );
    for nome in [".ssh", ".codex", ".claude", ".config", ".gnupg"] {
        assert_eq!(
            validar_destino(alias(&home.join(nome).join("dentro")), None, &home, &dados).unwrap_err().to_string(),
            "Pastas ocultas da pasta pessoal não podem ser usadas como pasta de trabalho."
        );
    }
    assert_eq!(
        validar_destino(alias(&dados.join("nova")), None, &home, &dados).unwrap_err().to_string(),
        "Esta pasta contém dados internos do Hub. Escolha uma pasta de trabalho."
    );
    assert_eq!(relativa_protegida(&alias(&projeto), &home).unwrap(), Some(PathBuf::from("projeto")));
}

#[cfg(unix)]
#[test]
fn motor_separado_so_libera_workspace_canonico_do_proprio_bot() {
    let f = Pastas::novas();
    // Dentro dos dados protegidos: sem a exceção, todos estes caminhos são recusados.
    let trabalho = f.criar(&f.dados.join("trabalho"));
    let proprio = f.criar(&trabalho.join("bots/a/workspace"));
    let outro = f.criar(&trabalho.join("bots/b/workspace"));
    let grupo = f.criar(&trabalho.join("grupos/g/pasta"));
    let validar = |pasta: &Path, bot, separado: bool| {
        let real = std::fs::canonicalize(pasta).unwrap();
        validar_destino_em(real, bot, &f.home, &f.dados, separado.then_some(trabalho.as_path()))
    };
    assert_eq!(validar(&proprio, Some("a"), true).unwrap(), proprio.canonicalize().unwrap());
    for pasta in [&proprio, &outro, &trabalho, &grupo, Path::new("/var")] {
        assert!(validar(pasta, Some("a"), false).is_err(), "legado: {pasta:?}");
    }
    for pasta in [&outro, &trabalho, &grupo, Path::new("/var")] {
        assert!(validar(pasta, Some("a"), true).is_err(), "separado: {pasta:?}");
    }
    assert!(validar(&proprio, None, true).is_err());
    let link = f.projeto.join("link-trabalho");
    std::os::unix::fs::symlink(&trabalho, &link).unwrap();
    assert!(validar(&link, Some("a"), true).is_err());
    // Nem o nome textual de workspace gerenciado transforma o destino de um link em exceção.
    std::fs::remove_dir(&proprio).unwrap();
    std::os::unix::fs::symlink(&trabalho, &proprio).unwrap();
    assert!(validar(&proprio, Some("a"), true).is_err());
    // A raiz real da F3 permanece protegida sem o modo e exige o bot certo com ele.
    let real = resolver_raiz(&crate::fork::motor_roots::workspace("a")).unwrap();
    let work = crate::fork::motor_roots::work();
    assert!(validar_destino_em(real.clone(), Some("a"), &f.home, &f.dados, Some(&work)).is_ok());
    assert!(validar_destino_em(real.clone(), Some("b"), &f.home, &f.dados, Some(&work)).is_err());
    assert!(validar_destino_em(real, Some("a"), &f.home, &f.dados, None).is_err());
}

#[cfg(unix)]
#[test]
fn d520_recusa_familias_privadas_e_aliases_preservando_override_atual() {
    let f = Pastas::novas();
    for familia in [".mexerica", ".donnabot", ".codync", ".MEXERICA", ".DONNABOT", ".CODYNC"] {
        for sufixo in ["", "-dev", "-dev-teste", "-backup"] {
            let nome = format!("{familia}{sufixo}");
            // .donnabot já existe na fixture; as demais apontam para fora da home.
            let protegida = if nome == ".donnabot" {
                f.dados.clone()
            } else {
                let externa = f.criar(&f.raiz.join(format!("externa-{nome}")));
                std::os::unix::fs::symlink(&externa, f.home.join(&nome)).unwrap();
                externa
            };
            let dentro = f.criar(&protegida.join("privado"));
            for pasta in [&protegida, &dentro, &f.home.join(&nome)] {
                assert!(f.validar(pasta, None).is_err(), "{nome}: {pasta:?}");
                assert!(validar_criacao_em(&pasta.join("nova"), &f.home, &f.dados).is_err());
            }
        }
    }
    // MEXERICA_HOME seleciona o override; raízes antigas continuam negadas.
    let dados = f.criar(&f.raiz.join("override-fora-da-home"));
    let segredo = f.criar(&dados.join("privado"));
    assert!(validar_em(segredo.to_str().unwrap(), None, &f.home, &dados).is_err());
    assert!(validar_criacao_em(&dados.join("novo"), &f.home, &dados).is_err());
    let workspace = f.criar(&dados.join("bots/bot/workspace"));
    assert!(validar_em(workspace.to_str().unwrap(), Some("bot"), &f.home, &dados).is_ok());
    assert!(f.validar(&f.projeto, None).is_ok());
}
