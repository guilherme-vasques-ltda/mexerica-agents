//! Provas por propriedade do porteiro: milhares de pedidos e regras gerados com semente fixa
//! conferem as invariantes de segurança. O gerador é próprio e determinístico (sem dependência);
//! cada falha imprime o número do caso e a entrada que a causou, e a mesma semente a reproduz.
#![allow(clippy::unwrap_used)]
use super::{
    avaliacao::{Correspondencia, Decisao, Mediacao, Origem, Pedido, Validade, avaliar, compor, decisao_regra},
    floor::{Categoria, Efeito, categoria, categoria_mutacao, sinal_execucao_futura},
    gate::{GateOutcome, GateRequest, decide},
    rule::{ApprovalLevel, Effect, Rule},
};
use crate::{
    LockExt,
    agent::bot::Adapter,
    store::{Permission, Store},
};
use serde_json::{Value, json};
use std::path::Path;

const CASOS: usize = 20_000;
const CASOS_PORTEIRO: usize = 1_500;
const SEMENTE: u64 = 0x5EED_F00D_0F70_7274;

const NIVEIS: [ApprovalLevel; 3] = [ApprovalLevel::Ask, ApprovalLevel::AutoApprove, ApprovalLevel::FullAccess];
const EFEITOS: [Efeito; 13] = [
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
    Efeito::ComumComprovado,
    Efeito::Opaco,
];
const BOTS: [&str; 2] = ["bot-1", "bot-2"];
const FERRAMENTAS: [&str; 4] = ["execute", "edit", "mcp__srv__ls", "read"];
const ACOES: [&str; 3] = ["ls", "a", "b"];
const ALVOS: [&str; 3] = ["git status", "/tmp/x", "a"];
const VALORES: [&str; 14] = [
    "ls",
    "a",
    "b",
    "git status",
    "/tmp/x",
    "execute",
    "edit",
    "mcp__srv__ls",
    "read",
    "",
    " ls",
    "LS",
    "ls ",
    "/bin/ls",
];
const CAMPOS_RECONHECIDOS: [&str; 3] = ["acao", "alvo", "ferramenta"];
const CAMPOS_DESCONHECIDOS: [&str; 8] = ["Acao", "alvo ", "", "comando", "ferramenta\0", "acao.alvo", "ação", "tool"];
const NIVEIS_GRAVADOS: [&str; 3] = ["ask", "auto_approve", "full_access"];
const OPERADORES_DESCONHECIDOS: [&str; 6] = ["IGUAL", "igual ", "contem", "==", "", "regex"];

/// Gerador pseudoaleatório (splitmix64) com semente fixa.
struct Gerador(u64);

impl Gerador {
    fn proximo(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn ate(&mut self, limite: usize) -> usize {
        usize::try_from(self.proximo() % limite as u64).unwrap()
    }

    fn chance(&mut self, percentual: usize) -> bool {
        self.ate(100) < percentual
    }

    fn de<T: Copy>(&mut self, itens: &[T]) -> T {
        itens[self.ate(itens.len())]
    }

    fn embaralhar<T>(&mut self, itens: &mut [T]) {
        for i in (1..itens.len()).rev() {
            itens.swap(i, self.ate(i + 1));
        }
    }
}

#[derive(Debug, Clone)]
struct PedidoGerado {
    bot: String,
    ferramenta: String,
    acao: String,
    alvo: String,
    efeitos: Vec<Efeito>,
    origem: Origem,
    mediacao: Mediacao,
    validade: Validade,
    nivel: ApprovalLevel,
}

impl PedidoGerado {
    fn como_pedido(&self) -> Pedido<'_> {
        Pedido {
            bot: &self.bot,
            ferramenta: &self.ferramenta,
            acao: &self.acao,
            alvo: &self.alvo,
            efeitos: &self.efeitos,
            origem: self.origem,
            mediacao: self.mediacao,
            politica_e_autoridade: self.validade,
            nivel: self.nivel,
        }
    }
}

fn texto_ou_vazio(g: &mut Gerador, opcoes: &[&str]) -> String {
    if g.chance(8) { String::new() } else { g.de(opcoes).to_owned() }
}

fn efeitos_aleatorios(g: &mut Gerador) -> Vec<Efeito> {
    let tamanho = 1 + g.ate(3);
    match g.ate(4) {
        0 | 1 => vec![Efeito::ComumComprovado; tamanho],
        2 => (0..tamanho).map(|_| g.de(&EFEITOS)).collect(),
        _ if g.chance(50) => Vec::new(),
        _ => vec![Efeito::Opaco; tamanho],
    }
}

fn pedido_aleatorio(g: &mut Gerador) -> PedidoGerado {
    PedidoGerado {
        bot: texto_ou_vazio(g, &BOTS),
        ferramenta: texto_ou_vazio(g, &FERRAMENTAS),
        acao: texto_ou_vazio(g, &ACOES),
        alvo: texto_ou_vazio(g, &ALVOS),
        efeitos: efeitos_aleatorios(g),
        origem: if g.chance(50) { Origem::DonoDigitadoAutenticado } else { Origem::Derivada },
        mediacao: match g.ate(10) {
            0 => Mediacao::ForaDoHub,
            1 => Mediacao::IncertaSemPontoSeguro,
            2 => Mediacao::IncertaComPontoSeguro,
            _ => Mediacao::Comprovada,
        },
        validade: if g.chance(92) { Validade::Atual } else { Validade::Invalida },
        nivel: g.de(&NIVEIS),
    }
}

/// Pedido que, sem as travas, chegaria a Permitir com regra ou Acesso total.
fn pedido_comum(g: &mut Gerador) -> PedidoGerado {
    PedidoGerado {
        bot: g.de(&BOTS).into(),
        ferramenta: g.de(&FERRAMENTAS).into(),
        acao: g.de(&ACOES).into(),
        alvo: g.de(&ALVOS).into(),
        efeitos: vec![Efeito::ComumComprovado],
        origem: if g.chance(50) { Origem::DonoDigitadoAutenticado } else { Origem::Derivada },
        mediacao: Mediacao::Comprovada,
        validade: Validade::Atual,
        nivel: ApprovalLevel::AutoApprove,
    }
}

fn nova_regra(efeito: Effect, campo: &str, operador: &str, valor: &str) -> Rule {
    Rule {
        id: "regra-gerada".into(),
        effect: efeito,
        field: campo.into(),
        op: operador.into(),
        value: valor.into(),
        bot_id: None,
        tool: None,
        scope: None,
        enabled: true,
        revision: 1,
        created_at: 0,
        updated_at: 0,
    }
}

fn regra_aleatoria(g: &mut Gerador) -> Rule {
    let efeito = if g.chance(50) { Effect::Allow } else { Effect::Deny };
    let campo = if g.chance(85) { g.de(&CAMPOS_RECONHECIDOS) } else { g.de(&CAMPOS_DESCONHECIDOS) };
    let operador = if g.chance(90) { "igual" } else { g.de(&OPERADORES_DESCONHECIDOS) };
    let valor = g.de(&VALORES);
    let mut regra = nova_regra(efeito, campo, operador, valor);
    regra.id = format!("regra-{}", g.ate(1_000_000));
    regra.bot_id = g.chance(25).then(|| g.de(&BOTS).to_owned());
    regra.tool = g.chance(25).then(|| g.de(&FERRAMENTAS).to_owned());
    regra.scope = g.chance(8).then(|| json!({"vinculo": "antigo"}));
    regra.enabled = g.chance(85);
    regra
}

fn regras_aleatorias(g: &mut Gerador, maximo: usize) -> Vec<Rule> {
    let quantas = g.ate(maximo + 1);
    (0..quantas).map(|_| regra_aleatoria(g)).collect()
}

fn restricao(decisao: Decisao) -> u8 {
    match decisao {
        Decisao::Permitir => 0,
        Decisao::Perguntar => 1,
        Decisao::ForaDaGarantia => 2,
        Decisao::Recusar => 3,
    }
}

// Oráculo independente, escrito só a partir do contrato: igualdade literal, campos
// reconhecidos; regra desligada ou de outro bot/ferramenta não participa.
fn valor_do_campo<'a>(regra: &Rule, pedido: &'a Pedido<'_>) -> &'a str {
    match regra.field.as_str() {
        "acao" => pedido.acao,
        "alvo" => pedido.alvo,
        _ => pedido.ferramenta,
    }
}

fn reconhecida(regra: &Rule) -> bool {
    regra.scope.is_none() && regra.op == "igual" && CAMPOS_RECONHECIDOS.contains(&regra.field.as_str())
}

fn participa(regra: &Rule, pedido: &Pedido<'_>) -> bool {
    regra.enabled
        && regra.bot_id.as_deref().is_none_or(|bot| bot == pedido.bot)
        && regra.tool.as_deref().is_none_or(|ferramenta| ferramenta == pedido.ferramenta)
}

fn concede(regra: &Rule, pedido: &Pedido<'_>) -> bool {
    let valor = valor_do_campo(regra, pedido);
    regra.effect == Effect::Allow
        && participa(regra, pedido)
        && reconhecida(regra)
        && !valor.is_empty()
        && valor == regra.value
}

fn pode_recusar(regra: &Rule, pedido: &Pedido<'_>) -> bool {
    let valor = valor_do_campo(regra, pedido);
    regra.effect == Effect::Deny
        && participa(regra, pedido)
        && (!reconhecida(regra) || valor.is_empty() || valor == regra.value)
}

#[test]
fn sem_efeito_comprovado_nunca_permite() {
    let mut g = Gerador(SEMENTE);
    let mut alcancaria = 0;
    for caso in 0..CASOS {
        let mut gerado = pedido_aleatorio(&mut g);
        gerado.efeitos = match g.ate(3) {
            0 => Vec::new(),
            1 => vec![Efeito::Opaco],
            _ => {
                let quantos = g.ate(3);
                let mut mistura: Vec<_> = (0..quantos).map(|_| g.de(&EFEITOS)).collect();
                mistura.insert(g.ate(mistura.len() + 1), Efeito::Opaco);
                mistura
            }
        };
        let mut regras = regras_aleatorias(&mut g, 6);
        // Regras que casam com o pedido: sem a trava de efeito, estas liberariam.
        regras.push(nova_regra(Effect::Allow, "ferramenta", "igual", &gerado.ferramenta));
        regras.push(nova_regra(Effect::Allow, "acao", "igual", &gerado.acao));
        for nivel in NIVEIS {
            gerado.nivel = nivel;
            let decisao = avaliar(&gerado.como_pedido(), &regras);
            assert_ne!(decisao, Decisao::Permitir, "caso {caso}, nível {nivel:?}: {gerado:?} {regras:?}");
        }
        let mut comum = gerado.clone();
        comum.efeitos = vec![Efeito::ComumComprovado];
        alcancaria += usize::from(avaliar(&comum.como_pedido(), &regras) == Decisao::Permitir);
    }
    assert!(alcancaria > CASOS / 20, "o gerador quase não alcança Permitir ({alcancaria}): a prova seria vazia");
}

#[test]
fn permitir_exige_prova_completa_e_regra_ou_acesso_total() {
    let mut g = Gerador(SEMENTE ^ 1);
    let mut permitidos = 0;
    for caso in 0..CASOS {
        let mut gerado = if g.chance(60) { pedido_comum(&mut g) } else { pedido_aleatorio(&mut g) };
        if g.chance(30) {
            gerado.nivel = g.de(&NIVEIS);
        }
        let regras = regras_aleatorias(&mut g, 8);
        let pedido = gerado.como_pedido();
        if avaliar(&pedido, &regras) != Decisao::Permitir {
            continue;
        }
        permitidos += 1;
        let contexto = format!("caso {caso}: {gerado:?} {regras:?}");
        assert_eq!(gerado.validade, Validade::Atual, "{contexto}");
        assert_eq!(gerado.mediacao, Mediacao::Comprovada, "{contexto}");
        let campos = [&gerado.bot, &gerado.ferramenta, &gerado.acao, &gerado.alvo];
        assert!(campos.iter().all(|campo| !campo.is_empty()), "{contexto}");
        assert!(
            !gerado.efeitos.is_empty() && gerado.efeitos.iter().all(|e| *e == Efeito::ComumComprovado),
            "{contexto}"
        );
        assert_ne!(gerado.nivel, ApprovalLevel::Ask, "{contexto}");
        assert!(!regras.iter().any(|r| pode_recusar(r, &pedido)), "{contexto}");
        assert!(
            gerado.nivel == ApprovalLevel::FullAccess || regras.iter().any(|r| concede(r, &pedido)),
            "Permitir sem regra explícita que o conceda e sem Acesso total. {contexto}"
        );
    }
    assert!(permitidos > CASOS / 20, "poucos Permitir gerados ({permitidos}): a prova seria vazia");
}

#[test]
fn recusar_vence_em_qualquer_ordem_e_com_quaisquer_outras_regras() {
    let mut g = Gerador(SEMENTE ^ 2);
    for caso in 0..CASOS {
        let gerado = if g.chance(50) { pedido_comum(&mut g) } else { pedido_aleatorio(&mut g) };
        let pedido = gerado.como_pedido();
        let mut regras = regras_aleatorias(&mut g, 8);
        // Oráculo: se alguma regra aleatória já deve recusar, o resultado é Recusar.
        if regras.iter().any(|r| pode_recusar(r, &pedido)) || gerado.validade == Validade::Invalida {
            assert_eq!(avaliar(&pedido, &regras), Decisao::Recusar, "caso {caso}: {gerado:?} {regras:?}");
        }
        // Recusar construída: casa com certeza (Sim) ou por incerteza (regra não reconhecida).
        let mut recusa = match g.ate(3) {
            0 if !gerado.acao.is_empty() => nova_regra(Effect::Deny, "acao", "igual", &gerado.acao),
            1 if !gerado.alvo.is_empty() => nova_regra(Effect::Deny, "alvo", "igual", &gerado.alvo),
            2 => nova_regra(Effect::Deny, g.de(&CAMPOS_DESCONHECIDOS), "igual", "qualquer"),
            _ if gerado.ferramenta.is_empty() => nova_regra(Effect::Deny, "ferramenta", "regex", "qualquer"),
            _ => nova_regra(Effect::Deny, "ferramenta", "igual", &gerado.ferramenta),
        };
        recusa.bot_id = g.chance(50).then(|| gerado.bot.clone());
        regras.push(recusa);
        for _ in 0..3 {
            g.embaralhar(&mut regras);
            let decisao = avaliar(&pedido, &regras);
            assert_eq!(decisao, Decisao::Recusar, "caso {caso}: {gerado:?} {regras:?}");
        }
    }
}

#[test]
fn ordem_das_regras_nao_muda_a_decisao() {
    let mut g = Gerador(SEMENTE ^ 3);
    for caso in 0..CASOS {
        let gerado = if g.chance(60) { pedido_comum(&mut g) } else { pedido_aleatorio(&mut g) };
        let pedido = gerado.como_pedido();
        let mut regras = regras_aleatorias(&mut g, 8);
        let esperada = avaliar(&pedido, &regras);
        let tem_motivo = decisao_regra(&pedido, &regras, esperada).is_some();
        for _ in 0..3 {
            g.embaralhar(&mut regras);
            let decisao = avaliar(&pedido, &regras);
            assert_eq!(decisao, esperada, "caso {caso}: {gerado:?} {regras:?}");
            let motivo = decisao_regra(&pedido, &regras, decisao);
            assert_eq!(motivo.is_some(), tem_motivo, "caso {caso}: {gerado:?} {regras:?}");
            match (decisao, motivo) {
                (Decisao::Recusar, Some((regra, certeza))) => {
                    assert_eq!(regra.effect, Effect::Deny);
                    assert_ne!(certeza, Correspondencia::Nao);
                }
                (Decisao::Permitir, Some((regra, certeza))) => {
                    assert_eq!(regra.effect, Effect::Allow);
                    assert_eq!(certeza, Correspondencia::Sim);
                }
                _ => {}
            }
        }
    }
}

#[test]
fn acrescentar_recusar_nunca_afrouxa_e_acrescentar_permitir_so_libera_o_que_perguntava() {
    let mut g = Gerador(SEMENTE ^ 4);
    for caso in 0..CASOS {
        let gerado = if g.chance(60) { pedido_comum(&mut g) } else { pedido_aleatorio(&mut g) };
        let pedido = gerado.como_pedido();
        let regras = regras_aleatorias(&mut g, 6);
        let antes = avaliar(&pedido, &regras);
        let contexto = format!("caso {caso}: {gerado:?} {regras:?}");

        let mut com_recusa = regras.clone();
        let mut extra = regra_aleatoria(&mut g);
        extra.effect = Effect::Deny;
        com_recusa.insert(g.ate(com_recusa.len() + 1), extra);
        assert!(restricao(avaliar(&pedido, &com_recusa)) >= restricao(antes), "{contexto}: {com_recusa:?}");

        let mut com_permissao = regras.clone();
        let mut extra = regra_aleatoria(&mut g);
        extra.effect = Effect::Allow;
        com_permissao.insert(g.ate(com_permissao.len() + 1), extra);
        let depois = avaliar(&pedido, &com_permissao);
        assert!(
            depois == antes || (antes == Decisao::Perguntar && depois == Decisao::Permitir),
            "{contexto}: Allow extra {com_permissao:?} mudou {antes:?} para {depois:?}"
        );
    }
}

#[test]
fn nivel_nao_desfaz_recusa_e_pedir_liberacao_nunca_permite() {
    let mut g = Gerador(SEMENTE ^ 5);
    for caso in 0..CASOS {
        let mut gerado = if g.chance(60) { pedido_comum(&mut g) } else { pedido_aleatorio(&mut g) };
        let regras = regras_aleatorias(&mut g, 8);
        let mut por_nivel = Vec::new();
        for nivel in NIVEIS {
            gerado.nivel = nivel;
            por_nivel.push(avaliar(&gerado.como_pedido(), &regras));
        }
        let contexto = format!("caso {caso}: {gerado:?} {regras:?} {por_nivel:?}");
        assert_ne!(por_nivel[0], Decisao::Permitir, "{contexto}");
        if por_nivel.contains(&Decisao::Recusar) {
            assert!(por_nivel.iter().all(|d| *d == Decisao::Recusar), "{contexto}");
        }
        // Subir o nível só pode trocar Perguntar por Permitir, nunca o contrário.
        assert!(restricao(por_nivel[0]) >= restricao(por_nivel[1]), "{contexto}");
        assert!(restricao(por_nivel[1]) >= restricao(por_nivel[2]), "{contexto}");
    }
}

#[test]
fn compor_e_comutativa_associativa_e_devolve_a_maior_restricao() {
    let todas = [Decisao::Recusar, Decisao::Perguntar, Decisao::Permitir, Decisao::ForaDaGarantia];
    let forca = |d: Decisao| match d {
        Decisao::Permitir => 0,
        Decisao::Perguntar => 1,
        Decisao::Recusar | Decisao::ForaDaGarantia => 2,
    };
    for a in todas {
        assert_eq!(forca(compor(a, a)), forca(a), "{a:?}");
        for b in todas {
            let r = compor(a, b);
            assert_eq!(r, compor(b, a), "{a:?} {b:?}");
            assert_eq!(forca(r), forca(a).max(forca(b)), "{a:?} {b:?}");
            assert_eq!(r == Decisao::Permitir, a == Decisao::Permitir && b == Decisao::Permitir, "{a:?} {b:?}");
            assert_ne!(r, Decisao::ForaDaGarantia, "{a:?} {b:?}");
            for c in todas {
                assert_eq!(compor(compor(a, b), c), compor(a, compor(b, c)), "{a:?} {b:?} {c:?}");
            }
        }
    }
}

#[test]
fn regra_nao_reconhecida_nunca_casa_como_sim() {
    let mut g = Gerador(SEMENTE ^ 6);
    for caso in 0..CASOS {
        let gerado = pedido_comum(&mut g);
        let pedido = gerado.como_pedido();
        let mut desconhecida = nova_regra(Effect::Allow, g.de(&CAMPOS_RECONHECIDOS), "igual", "");
        match g.ate(3) {
            0 => g.de(&CAMPOS_DESCONHECIDOS).clone_into(&mut desconhecida.field),
            1 => g.de(&OPERADORES_DESCONHECIDOS).clone_into(&mut desconhecida.op),
            _ => desconhecida.scope = Some(json!({"vinculo": g.ate(10)})),
        }
        // O valor é igual ao do pedido: só campo, operador ou escopo tornam a regra desconhecida.
        desconhecida.value = valor_do_campo(&desconhecida, &pedido).to_owned();
        assert!(desconhecida.interpretacao().is_none(), "caso {caso}: {desconhecida:?}");
        let contexto = format!("caso {caso}: {gerado:?} {desconhecida:?}");

        // Sem outra regra, só uma Permitir reconhecida poderia liberar: nada libera.
        let allow = desconhecida.clone();
        assert_eq!(avaliar(&pedido, std::slice::from_ref(&allow)), Decisao::Perguntar, "{contexto}");
        let mut regras = regras_aleatorias(&mut g, 4);
        regras.retain(|r| r.effect == Effect::Deny || !reconhecida(r));
        regras.push(allow);
        assert_ne!(avaliar(&pedido, &regras), Decisao::Permitir, "{contexto} {regras:?}");

        // Recusar desconhecida que participa nega, e nunca como Sim: a incerteza fecha.
        let mut deny = desconhecida;
        deny.effect = Effect::Deny;
        assert_eq!(avaliar(&pedido, std::slice::from_ref(&deny)), Decisao::Recusar, "{contexto}");
        let (_, certeza) = decisao_regra(&pedido, std::slice::from_ref(&deny), Decisao::Recusar).unwrap();
        assert_eq!(certeza, Correspondencia::Incerta, "{contexto}");
    }
}

#[test]
fn igualdade_e_literal_variacao_do_texto_nunca_vira_permissao() {
    let mut g = Gerador(SEMENTE ^ 7);
    for caso in 0..CASOS {
        let mut gerado = pedido_comum(&mut g);
        let campo = g.de(&CAMPOS_RECONHECIDOS);
        let base = match campo {
            "acao" => gerado.acao.clone(),
            "alvo" => gerado.alvo.clone(),
            _ => gerado.ferramenta.clone(),
        };
        let regra = nova_regra(Effect::Allow, campo, "igual", &base);
        assert_eq!(avaliar(&gerado.como_pedido(), std::slice::from_ref(&regra)), Decisao::Permitir, "caso {caso}");
        let variacao = match g.ate(5) {
            0 => format!("{base} "),
            1 => format!(" {base}"),
            2 => base.to_uppercase() + "x",
            3 => format!("/usr/bin/{base}"),
            _ => format!("{base}\u{200b}"),
        };
        match campo {
            "acao" => gerado.acao = variacao,
            "alvo" => gerado.alvo = variacao,
            _ => gerado.ferramenta = variacao,
        }
        let decisao = avaliar(&gerado.como_pedido(), std::slice::from_ref(&regra));
        assert_eq!(decisao, Decisao::Perguntar, "caso {caso}: {gerado:?} {regra:?}");
    }
}

fn texto_aleatorio(g: &mut Gerador) -> String {
    const PEDACOS: [&str; 16] = [
        "mcp__",
        "srv",
        "__",
        "ls",
        "..",
        "$x",
        "\u{60}",
        ".git",
        "hooks",
        "../",
        "\u{0}",
        "ç",
        "{",
        "\"",
        " ",
        "mcp__srv__ls",
    ];
    let quantos = g.ate(4);
    (0..quantos).map(|_| g.de(&PEDACOS)).collect()
}

fn json_aleatorio(g: &mut Gerador, profundidade: usize) -> Value {
    const CHAVES: [&str; 16] = [
        "toolCall",
        "rawInput",
        "kind",
        "title",
        "_meta",
        "claudeCode",
        "toolName",
        "mexericaTool",
        "server",
        "serverName",
        "tool",
        "arguments",
        "command",
        "path",
        "locations",
        "url",
    ];
    match if profundidade == 0 { g.ate(5) } else { g.ate(7) } {
        0 => Value::Null,
        1 => json!(g.chance(50)),
        2 => json!(i64::try_from(g.proximo() >> 2).unwrap() - (1 << 61)),
        3 => json!(f64::from(u32::try_from(g.proximo() >> 40).unwrap()) / 7.0),
        4 => Value::String(texto_aleatorio(g)),
        5 => {
            let quantos = g.ate(4);
            Value::Array((0..quantos).map(|_| json_aleatorio(g, profundidade - 1)).collect())
        }
        _ => {
            let quantos = g.ate(5);
            Value::Object(
                (0..quantos).map(|_| (g.de(&CHAVES).to_owned(), json_aleatorio(g, profundidade - 1))).collect(),
            )
        }
    }
}

fn params_aleatorios(g: &mut Gerador) -> Value {
    if g.chance(25) {
        return json_aleatorio(g, 3);
    }
    let mut chamada = serde_json::Map::new();
    if g.chance(80) {
        let tipo = if g.chance(70) { json!(g.de(&["execute", "edit", "read", "other"])) } else { json_aleatorio(g, 1) };
        chamada.insert("kind".into(), tipo);
    }
    for (chave, chance, profundidade) in [("title", 50, 1), ("rawInput", 70, 3), ("_meta", 40, 3), ("locations", 30, 2)]
    {
        if g.chance(chance) {
            chamada.insert(chave.into(), json_aleatorio(g, profundidade));
        }
    }
    json!({"sessionId": "sessao-1", "toolCall": chamada})
}

// Projeção geral gravada em nivel (ask, auto_approve ou full_access): hoje só ask é alcançável pela
// interface, mas o porteiro deve continuar fechado quando outro nível passar a existir.
fn loja_com_regras(com_recusa: bool, nivel: &str) -> Store {
    let loja = Store::open(Path::new(":memory:")).unwrap();
    let mut regras = vec![
        nova_regra(Effect::Allow, "ferramenta", "igual", "execute"),
        nova_regra(Effect::Allow, "ferramenta", "igual", "edit"),
        nova_regra(Effect::Allow, "ferramenta", "igual", "mcp__srv__ls"),
        nova_regra(Effect::Allow, "acao", "igual", "execute"),
        nova_regra(Effect::Allow, "alvo", "igual", "ls"),
    ];
    if com_recusa {
        regras.push(nova_regra(Effect::Deny, "ferramenta", "igual", "mcp__srv__ls"));
    }
    for regra in regras {
        let revisao = loja.donna_rules().unwrap().revision;
        loja.donna_rule_save(regra, revisao, true).unwrap();
    }
    loja.db.locked().execute("UPDATE donna_approvals SET level=?1 WHERE subject=''", [nivel]).unwrap();
    // Formato guardado por versão anterior: aparece ao avaliador e não pode liberar nada.
    loja.db
        .locked()
        .execute(
            "INSERT INTO donna_rules VALUES('legado','allow','formato-anterior','comparador-anterior','execute',NULL,NULL,NULL,9,1,10,20)",
            [],
        )
        .unwrap();
    loja
}

fn pedido_do_porteiro<'a>(params: &'a Value, recorded: Option<&'a Value>, g: &mut Gerador) -> GateRequest<'a> {
    GateRequest {
        params,
        recorded,
        bot_id: g.de(&BOTS),
        adapter: g.de(&[Adapter::Claude, Adapter::Codex, Adapter::Mexerica, Adapter::Other]),
        permission: g.de(&[Permission::Ask, Permission::Auto]),
        origin: g.de(&[Origem::DonoDigitadoAutenticado, Origem::Derivada]),
    }
}

#[test]
fn porteiro_com_json_aleatorio_sem_regra_de_recusar_so_pergunta() {
    for nivel in NIVEIS_GRAVADOS {
        let loja = loja_com_regras(false, nivel);
        let mut g = Gerador(SEMENTE ^ 8);
        for caso in 0..CASOS_PORTEIRO {
            let params = params_aleatorios(&mut g);
            let gravado = g.chance(50).then(|| json_aleatorio(&mut g, 2));
            let req = pedido_do_porteiro(&params, gravado.as_ref(), &mut g);
            let (adapter, permission) = (req.adapter, req.permission);
            let desfecho = decide(&loja, &req);
            assert_eq!(
                desfecho,
                GateOutcome::Ask,
                "nível {nivel}, caso {caso}: {adapter:?} {permission:?} {params} {gravado:?}"
            );
        }
        let eventos: i64 = loja
            .db
            .locked()
            .query_row("SELECT count(*) FROM donna_rule_events WHERE kind='gate_decision'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(eventos, 0, "Ask não gera evento de decisão automática");
    }
}

#[test]
fn porteiro_com_json_aleatorio_e_regra_de_recusar_nunca_libera() {
    for nivel in NIVEIS_GRAVADOS {
        let loja = loja_com_regras(true, nivel);
        let mut g = Gerador(SEMENTE ^ 9);
        let mut recusados = 0;
        for caso in 0..CASOS_PORTEIRO {
            let params = params_aleatorios(&mut g);
            let gravado = g.chance(50).then(|| json_aleatorio(&mut g, 2));
            let req = pedido_do_porteiro(&params, gravado.as_ref(), &mut g);
            let (adapter, permission) = (req.adapter, req.permission);
            let desfecho = decide(&loja, &req);
            assert_ne!(
                desfecho,
                GateOutcome::Allow,
                "nível {nivel}, caso {caso}: {adapter:?} {permission:?} {params} {gravado:?}"
            );
            recusados += usize::from(desfecho == GateOutcome::Deny);
        }
        assert!(recusados > 0, "o gerador nunca alcançou a regra de Recusar: a prova seria vazia");
    }
}

#[test]
fn porteiro_recusa_a_ferramenta_comprovada_mesmo_com_lixo_em_volta() {
    for nivel in NIVEIS_GRAVADOS {
        let loja = loja_com_regras(true, nivel);
        let mut g = Gerador(SEMENTE ^ 0xA);
        for caso in 0..CASOS_PORTEIRO {
            let lixo = json_aleatorio(&mut g, 2);
            let (adapter, chamada) = match g.ate(4) {
                0 => (
                    Adapter::Codex,
                    json!({"kind": "execute", "title": lixo, "rawInput": {"server": "srv", "tool": "ls", "arguments": lixo}, "locations": lixo}),
                ),
                1 => (
                    Adapter::Claude,
                    json!({"kind": "other", "title": lixo, "_meta": {"claudeCode": {"toolName": "mcp__srv__ls"}}, "rawInput": lixo}),
                ),
                2 => (
                    Adapter::Mexerica,
                    json!({"kind": "other", "title": lixo, "_meta": {"mexericaTool": {"server": "srv", "tool": "ls"}}, "rawInput": lixo}),
                ),
                _ => (Adapter::Other, json!({"kind": "mcp__srv__ls", "title": lixo, "rawInput": lixo})),
            };
            let params = json!({"sessionId": "sessao-1", "toolCall": chamada, "extra": lixo});
            let gravado = g.chance(50).then(|| json_aleatorio(&mut g, 2));
            let mut req = pedido_do_porteiro(&params, gravado.as_ref(), &mut g);
            req.adapter = adapter;
            let desfecho = decide(&loja, &req);
            assert_eq!(desfecho, GateOutcome::Deny, "nível {nivel}, caso {caso}: {adapter:?} {params} {gravado:?}");
        }
    }
}

#[test]
fn porteiro_nao_aceita_identidade_forjada_por_argumento() {
    for nivel in NIVEIS_GRAVADOS {
        let loja = loja_com_regras(true, nivel);
        let mut g = Gerador(SEMENTE ^ 0xB);
        for caso in 0..CASOS_PORTEIRO {
            let adapter = g.de(&[Adapter::Claude, Adapter::Mexerica, Adapter::Other]);
            let forjado = match g.ate(3) {
                0 => json!({"server": "srv", "tool": "ls", "arguments": {"command": "ls"}}),
                1 => json!({"serverName": "srv", "tool": "ls", "command": "ls"}),
                _ => json!({"server": "srv", "tool": json_aleatorio(&mut g, 1), "command": json_aleatorio(&mut g, 1)}),
            };
            let params =
                json!({"toolCall": {"kind": g.de(&["execute", "edit", "read", "other"]), "rawInput": forjado}});
            let mut req = pedido_do_porteiro(&params, None, &mut g);
            req.adapter = adapter;
            let desfecho = decide(&loja, &req);
            assert_eq!(desfecho, GateOutcome::Ask, "nível {nivel}, caso {caso}: {adapter:?} {params}");
        }
    }
}

fn caminho_aleatorio(g: &mut Gerador) -> String {
    const PARTES: [&str; 14] = [
        "a",
        "src",
        ".git",
        "hooks",
        ".github",
        "workflows",
        ".githooks",
        ".claude",
        ".vscode",
        "package.json",
        "CLAUDE.md",
        "..",
        "$HOME",
        "\u{60}x\u{60}",
    ];
    let separador = if g.chance(80) { "/" } else { "\\" };
    let quantas = g.ate(5);
    (0..quantas).map(|_| g.de(&PARTES)).collect::<Vec<_>>().join(separador)
}

#[test]
fn mutacao_nunca_devolve_categoria_que_libere() {
    let mut g = Gerador(SEMENTE ^ 0xC);
    for caso in 0..CASOS {
        let origem = g.chance(70).then(|| caminho_aleatorio(&mut g));
        let destino = g.chance(50).then(|| caminho_aleatorio(&mut g));
        let indireto = g.chance(20);
        let resultado = categoria_mutacao(origem.as_deref(), destino.as_deref(), indireto);
        let contexto = format!("caso {caso}: {origem:?} {destino:?} indireto={indireto} => {resultado:?}");
        assert!(matches!(resultado, Categoria::ExecucaoFutura | Categoria::Desconhecida), "{contexto}");
        if indireto || (origem.is_none() && destino.is_none()) {
            assert_eq!(resultado, Categoria::Desconhecida, "{contexto}");
            continue;
        }
        let sinais: Vec<_> = origem.iter().chain(destino.iter()).filter_map(|c| sinal_execucao_futura(c)).collect();
        assert!(sinais.iter().all(|s| matches!(s, Categoria::ExecucaoFutura | Categoria::Desconhecida)), "{contexto}");
        assert_eq!(resultado == Categoria::ExecucaoFutura, sinais.contains(&Categoria::ExecucaoFutura), "{contexto}");
    }
}

#[test]
fn so_o_efeito_comum_comprovado_fica_fora_de_categoria() {
    for efeito in EFEITOS {
        let classe = categoria(efeito);
        assert_eq!(classe.is_none(), efeito == Efeito::ComumComprovado, "{efeito:?}");
        assert_eq!(classe == Some(Categoria::Desconhecida), efeito == Efeito::Opaco, "{efeito:?}");
    }
}
