//! Motores que a Mexerica deixou de oferecer: agem sem pedir aprovação no cartão do Hub. O código de
//! cada motor fica no lugar, atrás de uma chave de desenvolvimento desligada; a lista é fechada e a
//! decisão é pura, testável sem tocar em variável de ambiente.
//!
//! A chave `MEXERICA_DEV_MOTORES_RETIRADOS=1` devolve a oferta antiga SÓ em build com
//! `debug_assertions` (os testes de ponta a ponta que ainda exercitam esses motores a ligam no
//! ambiente do próprio Hub de teste); builds de `release` nunca a honram.

/// Ids de motor que saíram da oferta, como estão em `agent::backends::HARNESSES`.
pub(crate) const RETIRADOS: &[&str] =
    &["kilo", "cline", "goose", "amp", "minimax", "pi", "qwen", "omp", "junie", "vibe"];

/// Chave de desenvolvimento que devolve a oferta antiga (nunca honrada em build de `release`).
const CHAVE_DEV: &str = "MEXERICA_DEV_MOTORES_RETIRADOS";

/// Decisão pura: `id` é retirado quando está na lista e a oferta antiga não foi liberada. O id vale por qualquer
/// nome do motor: o do harness, o do catálogo ACP (`qwen-code`, `pi-acp`, `mistral-vibe`…) e em qualquer caixa.
pub(crate) fn decidir(id: &str, liberado: bool) -> bool {
    !liberado && RETIRADOS.contains(&crate::agent::backends::id_canonico(id).as_str())
}

/// A oferta antiga liberada pela chave de desenvolvimento.
fn liberado() -> bool {
    cfg!(debug_assertions) && std::env::var_os(CHAVE_DEV).is_some_and(|v| v == "1")
}

/// O motor `id` saiu da oferta da Mexerica nesta versão.
pub(crate) fn retirado(id: &str) -> bool {
    decidir(id, liberado())
}

/// Mensagem de criação/edição: o dono escolhe outro motor.
pub(crate) const MSG_CRIAR: &str = "Este motor não está disponível nesta versão da Mexerica. Escolha outro motor.";

/// Mensagem de lançamento: quem já tem um bot no motor retirado troca nas configurações do bot.
pub(crate) fn msg_lancar(id: &str) -> String {
    let nome = crate::agent::backends::harness(id).map_or(id, |h| h.name);
    format!(
        "O motor {nome} não está disponível nesta versão da Mexerica. Escolha outro motor nas configurações do bot."
    )
}

/// Recusa um id de motor que a Mexerica não oferece; motores mantidos e a chave ligada passam.
pub(crate) fn exigir_oferecido(id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!retirado(id), "{MSG_CRIAR}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Motores que continuam oferecidos, incluindo o interno e o que a lista substituiu por
    /// filtro dedicado (`omp` agora é só um item dela).
    const MANTIDOS: &[&str] = &[
        "claude",
        "codex",
        "cursor",
        "opencode",
        "grok",
        "gemini",
        "copilot",
        "kimi",
        "droid",
        "auggie",
        "kiro",
        "devin",
        "qoder",
        "codebuddy",
        "antigravity",
        "cortex",
        "poolside",
        "mexerica",
    ];

    #[test]
    fn a_lista_e_exatamente_a_do_enunciado() {
        let mut lista = RETIRADOS.to_vec();
        lista.sort_unstable();
        assert_eq!(lista, ["amp", "cline", "goose", "junie", "kilo", "minimax", "omp", "pi", "qwen", "vibe"]);
        assert_eq!(RETIRADOS.len(), 10);
    }

    #[test]
    fn nenhum_motor_mantido_esta_na_lista() {
        for id in MANTIDOS {
            assert!(!RETIRADOS.contains(id), "{id} é mantido e não pode estar na lista");
            assert!(crate::agent::backends::harness(id).is_some() || *id == "mexerica", "{id} existe no Hub");
        }
    }

    #[test]
    fn nome_do_catalogo_ou_em_outra_caixa_e_o_mesmo_motor() {
        for id in [
            "qwen-code",
            "pi-acp",
            "amp-acp",
            "minimax-code",
            "mistral-vibe",
            "KILO",
            " Kilo ",
            "Qwen-Code",
            "GOOSE",
            "Cline",
            "JUNIE",
            "OMP",
        ] {
            assert!(decidir(id, false), "{id:?} nomeia um motor retirado");
            assert!(!decidir(id, true), "{id:?}: a chave devolve a oferta antiga");
        }
        for id in
            ["claude-acp", "codex-acp", "gemini", "github-copilot-cli", "OpenCode", "factory-droid", "antigravity-acp"]
        {
            assert!(!decidir(id, false), "{id:?} é de um motor mantido");
        }
    }

    #[test]
    fn com_a_chave_ligada_nada_e_retirado() {
        for id in RETIRADOS {
            assert!(!decidir(id, true), "{id}: a chave devolve a oferta antiga");
            assert!(decidir(id, false), "{id}: sem a chave, fica retirado");
        }
        assert!(!decidir("claude", false));
        assert!(!decidir("inexistente", false));
    }

    #[test]
    fn exigir_oferecido_recusa_retirado_e_passa_mantido() {
        for id in RETIRADOS {
            let erro = exigir_oferecido(id).unwrap_err().to_string();
            assert_eq!(erro, MSG_CRIAR, "{id}");
        }
        for id in ["claude", "codex", "opencode", "custom", ""] {
            exigir_oferecido(id).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }

    #[test]
    fn mensagem_de_lancar_usa_o_nome_do_harness() {
        assert!(msg_lancar("kilo").starts_with("O motor Kilo não está disponível nesta versão da Mexerica."));
        assert!(msg_lancar("omp").contains("oh-my-pi"));
        assert!(msg_lancar("desconhecido").starts_with("O motor desconhecido não está disponível"));
    }
}
