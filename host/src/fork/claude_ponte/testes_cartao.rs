//! Provas da projeção segura do cartão de ferramenta genérica: a entrada integral não sai da ponte,
//! o segredo não aparece no texto mostrado e o que não cabe inteiro não pode ser aprovado.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::traducao::{Traducao, projecao};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

fn sha256(entrada: &Value) -> String {
    Sha256::digest(entrada.to_string().as_bytes()).iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[test]
fn chave_de_nome_sensivel_fica_oculta_na_projecao() {
    let p = projecao("mcp__loja__pagar", &json!({"senha": "segredo-da-chave", "acao": "pagar"}));
    assert!(p.texto.contains("«oculto»"), "valor sensível ocultado: {}", p.texto);
    assert!(!p.texto.contains("segredo-da-chave"), "o valor da chave sensível não aparece: {}", p.texto);
    assert!(p.texto.contains("\"acao\": \"pagar\""), "o resto segue visível: {}", p.texto);
}

#[test]
fn par_nome_valor_sensivel_esconde_o_valor_inclusive_em_lista() {
    let p = projecao(
        "mcp__loja__criar",
        &json!({"env": [
            {"name": "API_TOKEN", "value": "segredo-do-par"},
            {"name": "MODO", "value": "producao"}]}),
    );
    assert!(!p.texto.contains("segredo-do-par"), "valor do par sensível ocultado: {}", p.texto);
    assert!(p.texto.contains("producao"), "par comum segue visível: {}", p.texto);
}

#[test]
fn url_perde_usuario_senha_consulta_e_fragmento() {
    let p = projecao("mcp__web__abrir", &json!({"url": "https://usuario:senha123@example.invalid/p?token=abc#topo"}));
    assert!(p.texto.contains("https://example.invalid/p?…"), "só esquema, host e caminho: {}", p.texto);
    assert!(!p.texto.contains("usuario:senha123"), "userinfo não aparece: {}", p.texto);
    assert!(!p.texto.contains("token=abc"), "a consulta não aparece: {}", p.texto);
}

#[test]
fn token_com_prefixo_conhecido_em_texto_livre_fica_oculto() {
    let p = projecao("mcp__loja__ligar", &json!({"nota": "chave sk-abc123def456ghi789 use com cuidado"}));
    assert!(!p.texto.contains("sk-abc123def456ghi789"), "token com prefixo conhecido ocultado: {}", p.texto);
    assert!(p.texto.contains("use com cuidado"), "o texto comum segue visível: {}", p.texto);
}

#[test]
fn sequencia_longa_misturando_letras_e_digitos_fica_oculta() {
    let p = projecao("mcp__loja__salvar", &json!({"mistura": "Ab3".repeat(15), "so_letras": "a".repeat(40)}));
    assert!(!p.texto.contains("Ab3Ab3"), "sequência longa misturada ocultada: {}", p.texto);
    assert!(p.texto.contains(&"a".repeat(40)), "sem dígito não é segredo: {}", p.texto);
}

#[test]
fn entrada_pequena_e_inocente_aparece_inteira_e_pode_ser_aprovada() {
    let p = projecao("mcp__loja__pagar", &json!({"acao": "pagar", "pedido": 7}));
    assert!(p.cabe, "entrada pequena cabe inteira: {}", p.texto);
    assert!(p.texto.contains("\"acao\": \"pagar\""), "corpo inteiro no texto: {}", p.texto);
    assert!(p.texto.contains("\"pedido\": 7"), "valor numérico no texto: {}", p.texto);
}

#[test]
fn entrada_grande_nao_cabe_e_o_texto_termina_com_o_aviso() {
    let enorme = json!({"corpo": "x".repeat(20_000), "destino": "https://api.exemplo.invalid/fim"});
    let p = projecao("mcp__loja__enviar", &enorme);
    assert!(!p.cabe, "entrada grande não cabe inteira");
    assert!(p.texto.chars().count() <= 1800, "texto dentro do limite: {}", p.texto.chars().count());
    assert!(
        p.texto.ends_with("argumentos longos demais para conferir: só é possível negar"),
        "o texto termina com o aviso: {}",
        p.texto
    );
}

#[test]
fn sha256_da_projecao_e_da_entrada_inteira_e_muda_com_o_trecho_cortado() {
    let base = json!({"corpo": "x".repeat(20_000), "destino": "https://api.exemplo.invalid/fim"});
    let outra = json!({"corpo": "x".repeat(20_000), "destino": "https://api.exemplo.invalid/outro"});
    let p = projecao("mcp__loja__enviar", &base);
    assert_eq!(p.sha256, sha256(&base), "o resumo cobre a entrada integral");
    let q = projecao("mcp__loja__enviar", &outra);
    assert_ne!(p.sha256, q.sha256, "duas entradas que só diferem no trecho cortado têm resumos distintos");
}

#[test]
fn raw_input_projetado_so_tem_as_chaves_da_projecao_e_sem_segredo() {
    let t = Traducao::new("/tmp");
    let entrada = json!({"destino": "https://api.exemplo.invalid/pedidos/7", "acao": "apagar",
        "api_key": "sk-segredo-123"});
    let (chamada, aprovavel) = t.chamada("toolu_1", "mcp__loja__apagar_pedido", &entrada);
    assert!(aprovavel, "entrada pequena pode ser conferida inteira");
    let raw = chamada["rawInput"].as_object().unwrap();
    for chave in ["servidor", "ferramenta", "resumo", "completo", "sha256"] {
        assert!(raw.contains_key(chave), "rawInput com a chave {chave}");
    }
    for proibida in ["command", "path", "file_path", "server", "serverName", "tool"] {
        assert!(!raw.contains_key(proibida), "rawInput sem chave de ação {proibida}");
    }
    let tudo = chamada.to_string();
    assert!(!tudo.contains("sk-segredo-123"), "nenhum segredo da entrada no cartão: {tudo}");
    assert_eq!(chamada["rawInput"]["servidor"], "loja");
    assert_eq!(chamada["rawInput"]["ferramenta"], "apagar_pedido");
    assert_eq!(chamada["rawInput"]["completo"], true);
    // Ferramenta com projeção própria segue igual: entrada integral e aprovável.
    let bash = json!({"command": "echo oi"});
    let (chamada, aprovavel) = t.chamada("toolu_2", "Bash", &bash);
    assert!(aprovavel);
    assert_eq!(chamada["rawInput"], bash, "a aprovação vale para a mesma entrada");
}

#[test]
fn atualizacao_de_chamada_generica_tambem_so_leva_a_projecao() {
    let mut t = Traducao::new("/tmp");
    let entrada = json!({"api_key": "sk-segredo-123", "acao": "apagar"});
    let saida = t.evento(&json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m1",
        "content": [{"type": "tool_use", "id": "toolu_1", "name": "mcp__loja__apagar_pedido",
            "input": entrada}]}}));
    let atualizacao = saida
        .iter()
        .find(|u| u["sessionUpdate"] == "tool_call_update" && u["toolCallId"] == "toolu_1")
        .expect("atualização da chamada anunciada");
    let raw = atualizacao["rawInput"].as_object().unwrap();
    for chave in ["servidor", "ferramenta", "resumo", "completo", "sha256"] {
        assert!(raw.contains_key(chave), "atualização com a chave {chave}");
    }
    assert!(!saida.iter().any(|u| u.to_string().contains("sk-segredo-123")), "segredo não sai no histórico");
}
