//! Provas próprias D70: fronteiras puras e contratos persistidos, sem executor ativado.
#![allow(clippy::unwrap_used)]
use super::{
    avaliacao::{Decisao, Mediacao, Origem, Pedido, Validade, avaliar, avaliar_fechado, compor},
    floor::{Categoria, Efeito, FloorContext, categoria, categoria_mutacao, sinal_execucao_futura},
    inventario::{self, Aplicador, Estado},
    rule::{ApprovalLevel, Effect, Rule, validate_with_floor},
};
use crate::{LockExt, store::Store};
use serde_json::json;
use std::path::Path;

fn regra(efeito: Effect) -> Rule {
    Rule {
        id: "regra-propria".into(),
        effect: efeito,
        field: "acao".into(),
        op: "igual".into(),
        value: "consultar".into(),
        bot_id: None,
        tool: None,
        scope: None,
        enabled: true,
        revision: 1,
        created_at: 0,
        updated_at: 0,
    }
}

fn pedido() -> Pedido<'static> {
    Pedido {
        bot: "bot-proprio",
        ferramenta: "ferramenta-propria",
        acao: "consultar",
        alvo: "alvo-vinculado",
        efeitos: &[Efeito::ComumComprovado],
        origem: Origem::DonoDigitadoAutenticado,
        mediacao: Mediacao::Comprovada,
        politica_e_autoridade: Validade::Atual,
        nivel: ApprovalLevel::AutoApprove,
    }
}

#[test]
fn recusar_precede_permitir_independente_da_ordem() {
    let permitir = regra(Effect::Allow);
    let recusar = regra(Effect::Deny);
    assert_eq!(avaliar(&pedido(), &[permitir.clone(), recusar.clone()]), Decisao::Recusar);
    assert_eq!(avaliar(&pedido(), &[recusar, permitir]), Decisao::Recusar);
}

#[test]
fn categorias_d106_d211_e_confirmacao_local_perguntam_em_todos_os_niveis() {
    let efeitos = [
        Efeito::MoverDinheiro,
        Efeito::ApagarOuSobrescreverIrreversivelmente,
        Efeito::ForcePush,
        Efeito::ElevarPrivilegios,
        Efeito::AcessarSegredo,
        Efeito::AlterarPastaHubMotor,
        Efeito::AlterarApiHubMotor,
        Efeito::AlterarConfiguracaoHubMotor,
        Efeito::AlterarExecucaoFutura,
        Efeito::EnviarNoComputadorDoDono,
        Efeito::LoginComTouchIdEscolhido,
    ];
    for efeito in efeitos {
        assert!(categoria(efeito).is_some());
        for nivel in [ApprovalLevel::Ask, ApprovalLevel::AutoApprove, ApprovalLevel::FullAccess] {
            let mut p = pedido();
            p.nivel = nivel;
            p.efeitos = std::slice::from_ref(&efeito);
            assert_eq!(avaliar(&p, &[regra(Effect::Allow)]), Decisao::Perguntar, "{efeito:?} {nivel:?}");
        }
    }
}

#[test]
fn d211_cobre_familias_criacao_remocao_renomeacao_e_indirecao() {
    for caminho in [
        "projeto/.githooks/pre-commit",
        "projeto/.git/hooks/post-checkout",
        "projeto/.github/workflows/validar.yml",
        "projeto/package.json",
        "projeto/.claude/settings.json",
        "projeto/CLAUDE.md",
        "projeto/.vscode/tasks.json",
    ] {
        assert_eq!(sinal_execucao_futura(caminho), Some(Categoria::ExecucaoFutura));
        assert_eq!(categoria_mutacao(None, Some(caminho), false), Categoria::ExecucaoFutura);
        assert_eq!(categoria_mutacao(Some(caminho), None, false), Categoria::ExecucaoFutura);
        assert_eq!(categoria_mutacao(Some(caminho), Some("comum.txt"), false), Categoria::ExecucaoFutura);
        assert_eq!(categoria_mutacao(Some("comum.txt"), Some(caminho), false), Categoria::ExecucaoFutura);
        assert_eq!(categoria_mutacao(None, Some(caminho), true), Categoria::Desconhecida);
    }
    assert_eq!(sinal_execucao_futura("projeto/../destino"), Some(Categoria::Desconhecida));
    assert_eq!(categoria_mutacao(None, Some("comum.txt"), false), Categoria::Desconhecida);
}

#[test]
fn regra_ou_alvo_opaco_nunca_autoriza_e_recusar_incerta_fecha() {
    let mut r = regra(Effect::Allow);
    r.op = "comparador-anterior".into();
    assert!(r.interpretacao().is_none());
    assert_eq!(avaliar(&pedido(), &[r.clone()]), Decisao::Perguntar);
    r.effect = Effect::Deny;
    assert_eq!(avaliar(&pedido(), &[r.clone()]), Decisao::Recusar);
    r.bot_id = Some("outro-bot".into());
    assert_eq!(avaliar(&pedido(), &[r.clone()]), Decisao::Perguntar);
    r.bot_id = None;
    r.tool = Some("outra-ferramenta".into());
    assert_eq!(avaliar(&pedido(), &[r]), Decisao::Perguntar);
    let mut p = pedido();
    p.efeitos = &[Efeito::Opaco];
    p.nivel = ApprovalLevel::FullAccess;
    assert_eq!(avaliar(&p, &[regra(Effect::Allow)]), Decisao::Perguntar);
    p.efeitos = &[];
    assert_eq!(avaliar(&p, &[]), Decisao::Perguntar);
    p.alvo = "";
    assert_eq!(avaliar(&p, &[]), Decisao::Recusar);
}

#[test]
fn nivel_e_origem_nao_afrouxam_chao_ou_negativa() {
    let permitir = [regra(Effect::Allow)];
    let mut p = pedido();
    assert_eq!(avaliar(&p, &permitir), Decisao::Permitir);
    p.nivel = ApprovalLevel::Ask;
    assert_eq!(avaliar(&p, &permitir), Decisao::Perguntar);
    p.nivel = ApprovalLevel::FullAccess;
    assert_eq!(avaliar(&p, &[]), Decisao::Permitir);
    p.origem = Origem::Derivada;
    assert_eq!(avaliar(&p, &permitir), Decisao::Permitir);
    assert_eq!(avaliar(&p, &[regra(Effect::Deny)]), Decisao::Recusar);
    p.nivel = ApprovalLevel::AutoApprove;
    assert_eq!(avaliar(&p, &[]), Decisao::Perguntar);
    assert_eq!(avaliar(&p, &permitir), Decisao::Permitir);
    p.politica_e_autoridade = Validade::Invalida;
    assert_eq!(avaliar(&p, &permitir), Decisao::Recusar);
    // Nem texto externo que se declare humano cria autoridade válida.
    p.origem = Origem::DonoDigitadoAutenticado;
    assert_eq!(avaliar(&p, &permitir), Decisao::Recusar);
}

#[test]
fn mediacao_incerta_so_pergunta_com_vinculo_e_ponto_seguro() {
    let mut p = pedido();
    p.mediacao = Mediacao::IncertaComPontoSeguro;
    assert_eq!(avaliar(&p, &[regra(Effect::Allow)]), Decisao::Perguntar);
    assert_eq!(avaliar(&p, &[regra(Effect::Deny)]), Decisao::Recusar);
    p.mediacao = Mediacao::IncertaSemPontoSeguro;
    assert_eq!(avaliar(&p, &[]), Decisao::Recusar);
    p.mediacao = Mediacao::ForaDoHub;
    assert_eq!(avaliar(&p, &[]), Decisao::ForaDaGarantia);
    assert_eq!(compor(Decisao::ForaDaGarantia, Decisao::Permitir), Decisao::Recusar);
}

#[test]
fn autoridade_invalida_erro_ou_nova_recusa_fecham() {
    let mut p = pedido();
    p.politica_e_autoridade = Validade::Invalida;
    assert_eq!(avaliar(&p, &[regra(Effect::Allow)]), Decisao::Recusar);
    assert_eq!(avaliar_fechado(Err(()), &[]), Decisao::Recusar);
    p.politica_e_autoridade = Validade::Atual;
    assert_eq!(avaliar_fechado(Ok(&p), &[regra(Effect::Allow)]), Decisao::Permitir);
    assert_eq!(avaliar(&p, &[regra(Effect::Allow), regra(Effect::Deny)]), Decisao::Recusar);
    // Isto prova só nova avaliação pura, não revalidação efetiva de um cartão.
}

#[test]
fn composicao_i341_mantem_a_maior_restricao() {
    for (esquerda, direita, esperado) in [
        (Decisao::Permitir, Decisao::Permitir, Decisao::Permitir),
        (Decisao::Permitir, Decisao::Perguntar, Decisao::Perguntar),
        (Decisao::Perguntar, Decisao::Perguntar, Decisao::Perguntar),
        (Decisao::Perguntar, Decisao::Recusar, Decisao::Recusar),
        (Decisao::Permitir, Decisao::Recusar, Decisao::Recusar),
    ] {
        assert_eq!(compor(esquerda, direita), esperado);
        assert_eq!(compor(direita, esquerda), esperado);
    }
}

#[test]
fn igualdade_literal_nao_executa_nem_interpreta_texto_de_comando() {
    let mut r = regra(Effect::Allow);
    r.value = "consultar; algo-indireto".into();
    assert_eq!(avaliar(&pedido(), &[r]), Decisao::Perguntar);
    let mut desconhecida = regra(Effect::Deny);
    desconhecida.field = "formato-anterior".into();
    assert_eq!(avaliar(&pedido(), &[desconhecida]), Decisao::Recusar);
    assert_eq!(avaliar(&pedido(), &[regra(Effect::Allow)]), Decisao::Permitir);
}

#[test]
fn validacao_propria_rejeita_semantica_nao_demonstrada_sem_perder_json() {
    let contexto = FloorContext::default();
    let mut r = regra(Effect::Allow);
    validate_with_floor(&r, &contexto).unwrap();
    r.scope = Some(json!({"vinculoLegado":{"destino":"preservar"}}));
    let bruto = serde_json::to_value(&r).unwrap();
    let relida: Rule = serde_json::from_value(bruto.clone()).unwrap();
    assert_eq!(serde_json::to_value(&relida).unwrap(), bruto);
    assert!(validate_with_floor(&relida, &contexto).is_err());
    assert!(relida.sentence().contains("não libera ações automaticamente"));
    r.scope = None;
    r.value = "\n".into();
    assert!(validate_with_floor(&r, &contexto).is_err());
}

#[test]
fn inventario_estrutural_falha_ao_retirar_qualquer_categoria_ou_superficie() {
    inventario::verificar(inventario::CATEGORIAS, inventario::SUPERFICIES).unwrap();
    assert_eq!(inventario::CATEGORIAS.len(), 10);
    assert_eq!(inventario::SUPERFICIES.len(), 24);
    for retirar in 0..inventario::CATEGORIAS.len() {
        let mut categorias = inventario::CATEGORIAS.to_vec();
        categorias.remove(retirar);
        assert!(inventario::verificar(&categorias, inventario::SUPERFICIES).is_err());
    }
    for retirar in 0..inventario::SUPERFICIES.len() {
        let superficies: Vec<_> = inventario::SUPERFICIES
            .iter()
            .enumerate()
            .filter(|(indice, _)| *indice != retirar)
            .map(|(_, s)| inventario::Superficie {
                id: s.id,
                ponto: s.ponto,
                motores: s.motores,
                estado: s.estado,
                limite: s.limite,
            })
            .collect();
        assert!(inventario::verificar(inventario::CATEGORIAS, &superficies).is_err());
    }
}

#[test]
fn nove_servidores_proxies_acp_e_limites_estao_declarados_sem_prova() {
    use Aplicador::{
        AbrirSse, AcpClaude, AcpCodex, Chat, ComputadorLocal, ComputadoresDono, Conectores, Equipe, Gestao, Http,
        Memoria, PostSse, ProcessoStdio, RequisicaoStdio, Rotinas,
    };
    for id in [
        Chat,
        Equipe,
        Gestao,
        Memoria,
        Rotinas,
        Conectores,
        ComputadorLocal,
        ComputadoresDono,
        ProcessoStdio,
        RequisicaoStdio,
        Http,
        AbrirSse,
        PostSse,
        AcpClaude,
        AcpCodex,
    ] {
        let s = inventario::SUPERFICIES.iter().find(|s| s.id == id).unwrap();
        assert_eq!(s.estado, Estado::PlanejadoSemProva);
        assert_ne!(s.ponto, "");
        assert_ne!(s.motores, "");
        assert_ne!(s.limite, "");
    }
    for categoria in inventario::CATEGORIAS {
        let pontos = inventario::aplicadores(*categoria);
        assert!(pontos.contains(&RequisicaoStdio));
        assert!(pontos.contains(&Http));
        assert!(pontos.contains(&PostSse));
    }
    assert!(!inventario::CATEGORIAS.iter().any(|c| format!("{c:?}").contains("Memoria")));
}

#[test]
fn estado_publico_reflete_o_porteiro_sem_prova_de_efeito() {
    // O inventário segue sem prova de interceptação de efeito; o contrato
    // declara só a fronteira ativada: pedidos de permissão do motor.
    assert!(!std::hint::black_box(inventario::RUNTIME_ATIVO));
    let contrato = super::levels::contract();
    assert_eq!(contrato["runtimeActive"], true);
    assert_eq!(contrato["application"], "permission_requests");
    for motor in ["claude", "codex"] {
        assert_eq!(contrato["disponibilidadePorMotor"][motor]["available"], false);
    }
    assert_eq!(
        super::api::METHODS,
        ["donnaRules", "donnaApprovalSet", "donnaRuleSave", "donnaRuleSetEnabled", "donnaRuleDelete"]
    );
}

#[test]
fn armazenamento_preserva_edicao_desligada_revisoes_eventos_e_limites() {
    let loja = Store::open(Path::new(":memory:")).unwrap();
    let inicial = loja.donna_rules().unwrap();
    let mut nova = regra(Effect::Allow);
    nova.enabled = false;
    let salva = loja.donna_rule_save(nova, inicial.revision, true).unwrap();
    assert_eq!(salva.revision, 1);
    assert!(!salva.enabled);
    let mut edicao = salva.clone();
    edicao.enabled = true;
    edicao.value = "outra-acao".into();
    let editada = loja.donna_rule_save(edicao, salva.revision, false).unwrap();
    assert!(!editada.enabled);
    assert_eq!(editada.created_at, salva.created_at);
    assert_eq!(editada.revision, 2);
    assert!(loja.donna_rule_enabled(&editada.id, true, 1).is_err());
    let ligada = loja.donna_rule_enabled(&editada.id, true, 2).unwrap();
    assert!(ligada.enabled);
    assert!(loja.donna_rule_delete(&ligada.id, 2).is_err());
    loja.donna_rule_delete(&ligada.id, ligada.revision).unwrap();
    assert!(loja.donna_rules().unwrap().rules.is_empty());
    let c = loja.db.locked();
    let eventos: i64 =
        c.query_row("SELECT count(*) FROM donna_rule_events WHERE rule_id=?1", [&salva.id], |r| r.get(0)).unwrap();
    assert_eq!(eventos, 4);
}

#[test]
fn legado_persistido_opaco_permanece_visivel_e_nao_autoriza() {
    let loja = Store::open(Path::new(":memory:")).unwrap();
    {
        let c = loja.db.locked();
        c.execute("INSERT INTO donna_rules VALUES(?1,'allow','formato-anterior','comparador-anterior','valor',NULL,NULL,?2,9,1,10,20)",
            rusqlite::params!["legado-preservado", "{\"vinculo\":\"antigo\"}"]).unwrap();
    }
    let snapshot = loja.donna_rules().unwrap();
    let antiga = &snapshot.rules[0];
    assert_eq!(antiga.id, "legado-preservado");
    assert_eq!((antiga.revision, antiga.created_at, antiga.updated_at), (9, 10, 20));
    assert_eq!(antiga.scope, Some(json!({"vinculo":"antigo"})));
    assert_eq!(avaliar(&pedido(), &snapshot.rules), Decisao::Perguntar);
    assert!(loja.donna_rule_enabled(&antiga.id, true, antiga.revision).is_err());
    let desligada = loja.donna_rule_enabled(&antiga.id, false, antiga.revision).unwrap();
    assert_eq!(desligada.scope, antiga.scope);
    assert!(!desligada.enabled);
}

#[test]
fn projecao_existente_idempotente_preserva_pedir_explicito_sem_full_access() {
    let loja = Store::open(Path::new(":memory:")).unwrap();
    let inicial = loja.donna_rules().unwrap();
    let geral = inicial.approvals.iter().find(|a| a.bot_id.is_none()).unwrap();
    assert!(loja.donna_approval_set(None, ApprovalLevel::FullAccess, geral.revision).is_err());
    assert!(loja.donna_approval_set(None, ApprovalLevel::AutoApprove, geral.revision).is_err());
    let pedir = loja.donna_approval_set(None, ApprovalLevel::Ask, geral.revision).unwrap();
    assert!(loja.donna_approval_set(None, ApprovalLevel::Ask, geral.revision).is_err());
    {
        let c = loja.db.locked();
        super::migrate::migrate(&c).unwrap();
        super::migrate::migrate(&c).unwrap();
    }
    let depois = loja.donna_rules().unwrap();
    let geral = depois.approvals.iter().find(|a| a.bot_id.is_none()).unwrap();
    assert_eq!(geral.level, ApprovalLevel::Ask);
    assert_eq!(geral.revision, pedir.revision);
    assert!(geral.permission_chosen_by_user);
    assert_eq!(depois.revision, inicial.revision + 1);
    assert!(depois.approvals.iter().all(|a| a.level != ApprovalLevel::FullAccess));
}

#[test]
fn duas_gravacoes_na_mesma_revisao_tem_um_conflito() {
    let loja = std::sync::Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let revisao = loja.donna_rules().unwrap().revision;
    let tarefas: Vec<_> = (0..2)
        .map(|_| {
            let loja = loja.clone();
            std::thread::spawn(move || loja.donna_rule_save(regra(Effect::Allow), revisao, true).is_ok())
        })
        .collect();
    let resultados: Vec<_> = tarefas.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(resultados.iter().filter(|ok| **ok).count(), 1);
    assert_eq!(loja.donna_rules().unwrap().rules.len(), 1);
}
