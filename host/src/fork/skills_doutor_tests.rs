use super::skills_doutor::{
    Diagnostico, NAO_VERIFICAVEL, POSSIVEL, Raizes, Tipo, VAZANDO, ancorado::Sonda, classificar, diagnosticar, digest,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// home/proj/app como pasta de trabalho; `fora` fica fora da home. Raiz canônica: sem links nos ancestrais.
fn cenario() -> (Tmp, Raizes, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("skills-doutor-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let home = root.join("home");
    let cwd = home.join("proj/app");
    let fora = root.join("fora");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&fora).unwrap();
    (Tmp(root), Raizes { home: Some(home), codex_home: None }, cwd, fora)
}

fn caminhos(d: &Diagnostico) -> Vec<&str> {
    d.achados.iter().map(|a| a.caminho.as_str()).collect()
}

fn sem_cobertura(d: &Diagnostico) -> bool {
    d.motivos.iter().all(|m| m.starts_with("fonte a confirmar") || m == "motor não mapeado")
}

#[test]
fn sem_vazamento_nao_acha_nada_e_nunca_afirma_limpo_com_fonte_a_confirmar() {
    let (_t, raizes, cwd, _) = cenario();
    // Pastas parecidas, mas que nenhum motor lê, não contam.
    fs::create_dir_all(raizes.home.as_ref().unwrap().join(".claude/projects")).unwrap();
    for motor in ["claude", "codex", "opencode", "devin", "gemini", "motor-novo"] {
        let d = diagnosticar(motor, &cwd, &raizes).unwrap();
        assert!(d.achados.is_empty(), "{motor}: {:?}", d.achados);
        assert!(!d.limpo(), "{motor}: fonte não confirmada não conta como coberta");
        assert!(sem_cobertura(&d), "{motor}: {:?}", d.motivos);
    }
    let d = diagnosticar("motor-novo", &cwd, &raizes).unwrap();
    assert!(!d.mapeado && d.motivos.contains(&"motor não mapeado".to_owned()));
}

#[test]
fn vazamento_na_home_aparece_so_para_o_motor_que_le_e_sem_caminho_absoluto() {
    let (_t, raizes, cwd, _) = cenario();
    let home = raizes.home.clone().unwrap();
    fs::create_dir_all(home.join(".codex/skills/x")).unwrap();
    fs::write(home.join(".codex/skills/x/SKILL.md"), "segredo").unwrap();
    let d = diagnosticar("codex", &cwd, &raizes).unwrap();
    assert_eq!(caminhos(&d), ["~/.codex/skills"]);
    assert_eq!((d.achados[0].tipo, d.achados[0].estado), (Tipo::Pasta, VAZANDO));
    assert_eq!(diagnosticar("claude", &cwd, &raizes).unwrap().achados, []);
    // Motor sem mapa procura em tudo (fail-closed).
    assert_eq!(diagnosticar("motor-novo", &cwd, &raizes).unwrap().achados.len(), 1);
    // CODEX_HOME troca a raiz do Codex; a fonte de projeto na home continua sondada.
    let outra = Raizes { home: raizes.home.clone(), codex_home: Some(home.join("ch")) };
    assert_eq!(caminhos(&diagnosticar("codex", &cwd, &outra).unwrap()), ["~/.codex/skills"]);
    fs::create_dir_all(home.join("ch")).unwrap();
    fs::write(home.join("ch/AGENTS.override.md"), "regra").unwrap();
    assert_eq!(caminhos(&diagnosticar("codex", &cwd, &outra).unwrap()), ["~/.codex/skills", "~/ch/AGENTS.override.md"]);
}

#[test]
fn fontes_oficiais_acrescentadas_aparecem() {
    let (_t, raizes, cwd, _) = cenario();
    let home = raizes.home.clone().unwrap();
    fs::write(cwd.join("AGENTS.override.md"), "x").unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(home.join(".claude/CLAUDE.md"), "x").unwrap();
    fs::create_dir_all(home.join(".gemini")).unwrap();
    fs::write(home.join(".gemini/GEMINI.md"), "x").unwrap();
    assert_eq!(caminhos(&diagnosticar("codex", &cwd, &raizes).unwrap()), ["./AGENTS.override.md"]);
    assert_eq!(caminhos(&diagnosticar("opencode", &cwd, &raizes).unwrap()), ["~/.claude/CLAUDE.md"]);
    assert_eq!(caminhos(&diagnosticar("gemini", &cwd, &raizes).unwrap()), ["~/.gemini", "~/.gemini/GEMINI.md"]);
}

#[test]
fn vazamento_em_ancestral_ate_a_home_em_caminho_relativo() {
    let (_t, raizes, cwd, _) = cenario();
    let home = raizes.home.clone().unwrap();
    fs::write(home.join("proj/CLAUDE.md"), "regra").unwrap();
    fs::write(home.join("AGENTS.md"), "regra").unwrap();
    // Acima da home não é olhado quando a pasta está dentro dela.
    fs::write(home.parent().unwrap().join("CLAUDE.md"), "regra").unwrap();
    let claude = diagnosticar("claude", &cwd, &raizes).unwrap();
    assert_eq!(caminhos(&claude), ["../CLAUDE.md"]);
    assert_eq!(claude.achados[0].tipo, Tipo::Arquivo);
    assert_eq!(caminhos(&diagnosticar("codex", &cwd, &raizes).unwrap()), ["~/AGENTS.md"]);
}

#[cfg(unix)]
#[test]
fn symlink_para_fora_e_relatado_sem_ser_seguido() {
    let (_t, raizes, cwd, fora) = cenario();
    fs::create_dir_all(fora.join("skills/y")).unwrap();
    fs::write(fora.join("skills/y/SKILL.md"), "segredo").unwrap();
    fs::create_dir_all(cwd.join(".claude")).unwrap();
    std::os::unix::fs::symlink(fora.join("skills"), cwd.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(&fora, cwd.join(".agents")).unwrap();
    // Symlink quebrado também conta: existe, mesmo sem alvo.
    std::os::unix::fs::symlink(fora.join("nada"), cwd.join("AGENTS.md")).unwrap();
    let antes = fs::read_dir(&fora).unwrap().count();
    let claude = diagnosticar("claude", &cwd, &raizes).unwrap();
    assert_eq!(caminhos(&claude), ["./.claude/skills"]);
    assert_eq!(claude.achados[0].tipo, Tipo::Symlink);
    let codex = diagnosticar("codex", &cwd, &raizes).unwrap();
    assert_eq!(caminhos(&codex), ["./.agents", "./AGENTS.md"]);
    assert!(codex.achados.iter().all(|a| a.tipo == Tipo::Symlink));
    assert_eq!(fs::read_dir(&fora).unwrap().count(), antes);
}

#[cfg(unix)]
#[test]
fn base_que_e_symlink_ou_esta_atras_de_um_nao_e_atravessada() {
    let (_t, raizes, _cwd, fora) = cenario();
    let home = raizes.home.clone().unwrap();
    // O que há "dentro" do alvo do link seria um vazamento se fosse seguido.
    fs::create_dir_all(fora.join("real/.claude/skills")).unwrap();
    fs::write(fora.join("real/CLAUDE.md"), "regra").unwrap();
    // cwd é link: a base inteira não é verificável.
    let link_cwd = home.join("link-app");
    std::os::unix::fs::symlink(fora.join("real"), &link_cwd).unwrap();
    let d = diagnosticar("claude", &link_cwd, &raizes).unwrap();
    assert!(d.achados.iter().any(|a| a.estado == NAO_VERIFICAVEL), "{:?}", d.achados);
    assert!(d.achados.iter().all(|a| a.estado != VAZANDO), "o alvo do link não pode ser olhado");
    assert!(!d.limpo());
    // Ancestral do cwd é link: tudo abaixo dele também é não verificável.
    std::os::unix::fs::symlink(fora.join("real"), home.join("proj-link")).unwrap();
    fs::create_dir_all(fora.join("real/app")).unwrap();
    let d = diagnosticar("claude", &home.join("proj-link/app"), &raizes).unwrap();
    assert!(d.achados.iter().any(|a| a.estado == NAO_VERIFICAVEL));
    assert!(d.achados.iter().all(|a| a.estado != VAZANDO));
    // Home é link.
    let casa = fora.join("casa-link");
    std::os::unix::fs::symlink(&home, &casa).unwrap();
    let raizes_link = Raizes { home: Some(casa.clone()), codex_home: None };
    let d = diagnosticar("claude", &casa.join("proj/app"), &raizes_link).unwrap();
    assert!(d.achados.iter().all(|a| a.estado == NAO_VERIFICAVEL), "{:?}", d.achados);
    assert_ne!(d.achados, []);
}

#[test]
fn raizes_fora_do_escopo_ou_desconhecidas_nao_afirmam_nada() {
    let (_t, raizes, cwd, fora) = cenario();
    fs::create_dir_all(fora.join("skills")).unwrap();
    fs::write(fora.join("CLAUDE.md"), "regra").unwrap();
    // CODEX_HOME fora da home e da pasta do bot, relativo ou com `..`: não verificável, não sondado.
    for ruim in [fora.clone(), PathBuf::from("relativo"), raizes.home.clone().unwrap().join("../fora")] {
        let r = Raizes { home: raizes.home.clone(), codex_home: Some(ruim) };
        let d = diagnosticar("codex", &cwd, &r).unwrap();
        assert!(d.achados.iter().any(|a| a.caminho == "CODEX_HOME" && a.estado == NAO_VERIFICAVEL));
        assert!(d.achados.iter().all(|a| a.estado != VAZANDO));
        assert!(!d.limpo());
    }
    // Pasta do bot fora da home: sobe nada além dela.
    let d = diagnosticar("claude", &fora, &raizes).unwrap();
    assert_eq!(caminhos(&d), ["./CLAUDE.md"]);
    assert!(d.motivos.iter().any(|m| m.contains("fora da home")));
    // Home desconhecida (motor separado sem conta isolada): só a pasta do bot, nunca limpo.
    let sem_home = Raizes { home: None, codex_home: None };
    let d = diagnosticar("claude", &cwd, &sem_home).unwrap();
    assert!(!d.limpo() && d.motivos.iter().any(|m| m.contains("home efetiva")));
}

#[test]
fn pasta_relativa_ou_com_volta_e_recusada() {
    let (_t, raizes, cwd, _) = cenario();
    assert!(diagnosticar("claude", Path::new("proj/app"), &raizes).is_err());
    assert!(diagnosticar("claude", &cwd.join("../app"), &raizes).is_err());
}

#[test]
fn digest_depende_do_conjunto_entregue_e_da_versao_fixa() {
    let a = vec![("a".to_owned(), "n:1:r:b".to_owned()), ("b".to_owned(), "n:1:r:c".to_owned())];
    let mut outra_versao = a.clone();
    outra_versao[0].1 = "n:2:r:b".to_owned();
    assert_eq!(digest(&a), digest(&a.clone()));
    assert_ne!(digest(&a), digest(&outra_versao));
    assert_ne!(digest(&a), digest(&a[..1]));
    assert_eq!(digest(&a).len(), 64);
}

#[test]
fn pasta_que_pode_ter_so_configuracao_e_possivel_e_a_fonte_nomeada_e_vazando() {
    let (_t, raizes, cwd, _) = cenario();
    let home = raizes.home.clone().unwrap();
    // Só configuração: não confirma regras nem skills.
    fs::create_dir_all(home.join(".config/opencode")).unwrap();
    fs::write(home.join(".config/opencode/opencode.json"), "{}").unwrap();
    fs::create_dir_all(cwd.join(".opencode")).unwrap();
    fs::create_dir_all(cwd.join(".gemini")).unwrap();
    let d = diagnosticar("opencode", &cwd, &raizes).unwrap();
    let mut achados = caminhos(&d);
    achados.sort_unstable();
    assert_eq!(achados, ["./.opencode", "~/.config/opencode"]);
    assert!(d.achados.iter().all(|a| a.estado == POSSIVEL && a.motivo.is_some()), "{:?}", d.achados);
    let d = diagnosticar("gemini", &cwd, &raizes).unwrap();
    assert_eq!((caminhos(&d), d.achados[0].estado), (vec!["./.gemini"], POSSIVEL));
    assert!(!d.limpo());
    // Fonte nomeada de skills ou regras continua confirmada.
    fs::create_dir_all(cwd.join(".claude/skills")).unwrap();
    let d = diagnosticar("opencode", &cwd, &raizes).unwrap();
    let skills = d.achados.iter().find(|a| a.caminho == "./.claude/skills").unwrap();
    assert_eq!((skills.estado, skills.motivo), (VAZANDO, None));
}

#[cfg(unix)]
#[test]
fn link_nao_seguido_e_possivel_nunca_vazamento_confirmado() {
    let (_t, raizes, cwd, fora) = cenario();
    fs::create_dir_all(fora.join("x")).unwrap();
    std::os::unix::fs::symlink(fora.join("x"), cwd.join(".claude")).unwrap();
    std::os::unix::fs::symlink(fora.join("nada"), cwd.join("AGENTS.md")).unwrap();
    for motor in ["claude", "codex"] {
        let d = diagnosticar(motor, &cwd, &raizes).unwrap();
        assert_ne!(d.achados, []);
        assert!(d.achados.iter().all(|a| a.estado == POSSIVEL), "{motor}: {:?}", d.achados);
    }
}

#[test]
fn falha_de_acesso_e_nao_verificavel_com_motivo() {
    let a = classificar("./x", ".claude/skills", Sonda::Falhou(2)).unwrap();
    assert_eq!((a.caminho.as_str(), a.tipo, a.estado), ("./x/.claude/skills", Tipo::Indeterminado, NAO_VERIFICAVEL));
    assert!(a.motivo.is_some());
    let a = classificar("~", "AGENTS.md", Sonda::Falhou(1)).unwrap();
    assert_eq!((a.caminho.as_str(), a.estado), ("~/AGENTS.md", NAO_VERIFICAVEL));
    assert!(classificar("~", "AGENTS.md", Sonda::Nada).is_none());
    // Tipo incompatível ou especial: possível, com motivo, nunca vazamento confirmado.
    for (rel, tipo) in [
        ("AGENTS.md", Tipo::Pasta),
        (".claude/skills", Tipo::Arquivo),
        ("AGENTS.md", Tipo::Indeterminado),
        (".claude/skills", Tipo::Indeterminado),
    ] {
        let n = rel.split('/').count();
        let a = classificar("~", rel, Sonda::Achou(tipo, n)).unwrap();
        assert_eq!(a.estado, POSSIVEL, "{rel} {tipo:?}");
        assert!(a.motivo.is_some());
    }
    // Tipo esperado: vazando.
    assert_eq!(classificar("~", "AGENTS.md", Sonda::Achou(Tipo::Arquivo, 1)).unwrap().estado, VAZANDO);
    assert_eq!(classificar("~", ".claude/skills", Sonda::Achou(Tipo::Pasta, 2)).unwrap().estado, VAZANDO);
}

#[test]
fn tipo_incompativel_no_disco_e_possivel() {
    let (_t, raizes, cwd, _) = cenario();
    fs::create_dir_all(cwd.join("AGENTS.md")).unwrap();
    fs::create_dir_all(cwd.join(".claude")).unwrap();
    fs::write(cwd.join(".claude/skills"), "x").unwrap();
    for motor in ["claude", "codex"] {
        let d = diagnosticar(motor, &cwd, &raizes).unwrap();
        assert_ne!(d.achados, []);
        assert!(d.achados.iter().all(|a| a.estado == POSSIVEL && a.motivo.is_some()), "{motor}: {:?}", d.achados);
    }
}

#[cfg(unix)]
#[test]
fn pasta_sem_permissao_vira_nao_verificavel() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, raizes, cwd, _) = cenario();
    fs::create_dir_all(cwd.join(".claude")).unwrap();
    fs::set_permissions(cwd.join(".claude"), fs::Permissions::from_mode(0o0)).unwrap();
    // Como root a permissão não barra a leitura: o caso fica coberto pelo teste de `classificar`.
    let barrado = fs::read_dir(cwd.join(".claude")).is_err();
    let d = diagnosticar("claude", &cwd, &raizes).unwrap();
    fs::set_permissions(cwd.join(".claude"), fs::Permissions::from_mode(0o755)).unwrap();
    if barrado {
        let a = d.achados.iter().find(|a| a.caminho == "./.claude").unwrap();
        assert_eq!((a.estado, a.tipo), (NAO_VERIFICAVEL, Tipo::Indeterminado), "{:?}", d.achados);
    }
}

#[test]
fn pasta_efetiva_do_motor_e_a_sondada_nao_a_do_cadastro() {
    let (_t, raizes, cwd, _) = cenario();
    // Vazamento na pasta do cadastro (a home do Hub) não pode aparecer quando o motor roda na raiz dele.
    fs::write(cwd.join("CLAUDE.md"), "regra").unwrap();
    let raiz_do_motor = raizes.home.clone().unwrap().join("raiz-do-motor-inexistente");
    let d = diagnosticar("claude", &raiz_do_motor, &raizes).unwrap();
    assert!(d.achados.is_empty(), "{:?}", d.achados);
    assert!(!d.limpo());
}
