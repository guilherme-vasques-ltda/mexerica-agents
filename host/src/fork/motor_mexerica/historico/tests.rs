//! Provas da conversa guardada: cópia enxuta, sem credenciais, limites rígidos,
//! Recorte por volta e arquivo por bot.
use super::*;
use crate::fork::motor_mexerica::conversation_owner as owner;
use serde_json::json;

fn user(text: &str) -> Value {
    json!({"role": "user", "content": [{"type": "text", "text": text}], "timestamp": 1})
}

fn assistant(text: &str) -> Value {
    json!({
        "role": "assistant", "content": [{"type": "text", "text": text}], "api": "faux", "provider": "faux",
        "model": "faux-1", "stopReason": "stop", "usage": {"input": 1, "output": 2}, "timestamp": 2
    })
}

fn call(id: &str, arguments: &Value) -> Value {
    json!({"role": "assistant", "api": "faux", "provider": "faux", "model": "faux-1", "stopReason": "toolUse",
        "content": [{"type": "toolCall", "id": id, "name": "eco", "arguments": arguments}], "timestamp": 2})
}

fn result(id: &str, text: &str) -> Value {
    json!({"role": "toolResult", "toolCallId": id, "toolName": "eco", "isError": false,
        "content": [{"type": "text", "text": text}], "timestamp": 3})
}

/// Cada chamada guardada tem um resultado só, do mesmo nome e na mesma volta; nada fica pela metade.
fn assert_paired(messages: &[Value]) {
    let mut open: Vec<(String, String)> = Vec::new();
    for message in messages {
        if message["role"] == "toolResult" {
            let id = message["toolCallId"].as_str().unwrap().to_owned();
            let name = message["toolName"].as_str().unwrap().to_owned();
            let position = open
                .iter()
                .position(|call| *call == (id.clone(), name.clone()))
                .unwrap_or_else(|| panic!("resultado {id} sem a chamada aberta na mesma volta"));
            open.remove(position);
            continue;
        }
        assert!(open.is_empty(), "chamadas sem resultado: {open:?}");
        open = message["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "toolCall")
            .map(|block| (block["id"].as_str().unwrap().to_owned(), block["name"].as_str().unwrap().to_owned()))
            .collect();
        let mut seen: Vec<&String> = Vec::new();
        for (id, _) in &open {
            assert!(!seen.contains(&id), "id de chamada repetido no mesmo lote: {id}");
            seen.push(id);
        }
    }
    assert!(open.is_empty(), "chamadas sem resultado: {open:?}");
}

fn texts_of(messages: &[Value]) -> Vec<String> {
    messages.iter().map(|m| m["content"][0]["text"].as_str().unwrap_or_default().to_owned()).collect()
}

fn texts(history: &History) -> Vec<String> {
    history.snapshot().iter().map(|m| m["content"][0]["text"].as_str().unwrap_or_default().to_owned()).collect()
}

#[test]
fn copia_enxuta_so_leva_texto_e_chamadas_de_ferramenta() {
    let raw = json!({
        "role": "assistant", "api": "faux", "provider": "faux", "model": "faux-1", "stopReason": "toolUse",
        "usage": {"input": 1}, "timestamp": 3, "responseId": "resp-segredo", "errorMessage": "diagnostico",
        "content": [
            {"type": "thinking", "thinking": "raciocinio privado", "thinkingSignature": "assinatura"},
            {"type": "text", "text": "vou chamar"},
            {"type": "toolCall", "id": "c1", "name": "eco", "arguments": {"texto": "oi"}, "thoughtSignature": "x"},
        ],
    });
    let clean = sanitize(&raw, &[]).unwrap();
    let wire = clean.to_string();
    for banned in
        ["raciocinio privado", "assinatura", "resp-segredo", "diagnostico", "thoughtSignature", "errorMessage"]
    {
        assert!(!wire.contains(banned), "{banned} não pode ser guardado: {wire}");
    }
    assert_eq!(clean["content"].as_array().unwrap().len(), 2);
    assert_eq!(clean["content"][1]["arguments"], json!({"texto": "oi"}));

    let result = json!({"role": "toolResult", "toolCallId": "c1", "toolName": "eco", "isError": false, "timestamp": 4,
        "content": [{"type": "text", "text": "eco:oi"}, {"type": "image", "data": "AAAA"}], "details": {"interno": true}});
    let clean = sanitize(&result, &[]).unwrap();
    assert_eq!(clean["content"], json!([{"type": "text", "text": "eco:oi"}]));
    assert!(clean.get("details").is_none());

    for ignored in [
        json!({"role": "assistant", "content": [{"type": "text", "text": "x"}], "stopReason": "error", "errorMessage": format!("chave {} falhou", ["s", "k-abcdefghijklmnop"].concat())}),
        json!({"role": "assistant", "content": [{"type": "text", "text": "x"}], "stopReason": "aborted"}),
        json!({"role": "system", "content": "prompt"}),
        json!({"role": "user", "content": [{"type": "image", "data": "AAAA"}]}),
    ] {
        assert!(sanitize(&ignored, &[]).is_none(), "{ignored}");
    }
}

#[test]
fn o_teto_de_blocos_conta_somente_blocos_de_texto() {
    let images: Vec<Value> = (0..MAX_BLOCKS).map(|_| json!({"type": "image", "data": "AAAA"})).collect();
    let mut user_content = images.clone();
    user_content.push(json!({"type": "text", "text": "depois das imagens"}));
    let user_message = json!({"role": "user", "content": user_content, "timestamp": 1});
    let clean = sanitize(&user_message, &[]).expect("o teto de blocos não é gasto por imagens");
    assert_eq!(clean["content"], json!([{"type": "text", "text": "depois das imagens"}]));

    let mut result_content = images;
    result_content.push(json!({"type": "text", "text": "eco:feito"}));
    let tool_result = json!({"role": "toolResult", "toolCallId": "c1", "toolName": "eco", "isError": false,
        "content": result_content, "timestamp": 3});
    let clean = sanitize(&tool_result, &[]).expect("o teto de blocos não é gasto por imagens");
    assert_eq!(clean["content"], json!([{"type": "text", "text": "eco:feito"}]));
}

#[test]
fn credenciais_do_chat_e_das_ferramentas_saem_antes_de_guardar() {
    let known = vec!["chave-do-outro-provedor-123456".to_owned()];
    // Montadas em tempo de execução: o scanner de segredos do CI não pode achar credencial no código-fonte.
    let akia = ["AKI", "AABCDEFGHIJKLMNOP"].concat();
    let aws = format!("{akia} e {}", ["AIza", "Sy", "A-abcdefghijklmnopqrstuvwxyz012345"].concat());
    let jwt = format!("jwt {}", ["eyJhbGciOiJIUzI1NiJ9", "eyJzdWIiOiIxMjM0NTY3ODkwIn0", "abcdefghijklmnop"].join("."));
    let use_sk = format!("use {} para entrar", ["s", "k-", "abcdefghijklmnopqrstuvwx"].concat());
    let bearer = ["Authorization: Bear", "er abcdefghijklmnopqrstuvwxyz0123"].concat();
    let token = format!("token {}", ["gh", "p_", "abcdefghijklmnopqrstuvwxyz", "0123456789"].concat());
    let valor = ["=zzz-", "valor-secreto"].concat();
    let senha = ["senha: Tr0ub4dor&3 e api_", "key", valor.as_str()].concat();
    let pem = ["-----BEGIN RSA PRIV", "ATE KEY-----\nMIIBOgIBAAJBAKj\n-----END RSA PRIV", "ATE KEY-----"].concat();
    let samples = [
        use_sk.as_str(),
        bearer.as_str(),
        token.as_str(),
        aws.as_str(),
        jwt.as_str(),
        senha.as_str(),
        pem.as_str(),
        "chave-do-outro-provedor-123456 colada no chat",
    ];
    for sample in samples {
        for message in [
            user(sample),
            json!({"role": "toolResult", "toolCallId": "c", "toolName": "t", "isError": false, "content": [{"type": "text", "text": sample}]}),
            json!({"role": "assistant", "api": "a", "provider": "p", "model": "m", "stopReason": "toolUse",
                "content": [{"type": "toolCall", "id": "c", "name": "t", "arguments": {"auth": {"valor": sample}}}]}),
        ] {
            let wire = sanitize(&message, &known).unwrap().to_string();
            for secret in [
                ["sk", "-abcdefghijklmnopqrstuvwx"].concat().as_str(),
                "abcdefghijklmnopqrstuvwxyz0123",
                ["gh", "p_abcdefghijklmnopqrstuvwxyz0123456789"].concat().as_str(),
                akia.as_str(),
                ["AI", "zaSyA-abcdefghijklmnopqrstuvwxyz012345"].concat().as_str(),
                "eyJhbGciOiJIUzI1NiJ9",
                "Tr0ub4dor&3",
                "zzz-valor-secreto",
                "MIIBOgIBAAJBAK",
                "chave-do-outro-provedor-123456",
            ] {
                assert!(!wire.contains(secret), "{secret} vazou de {sample:?}: {wire}");
            }
        }
    }
    let kept = sanitize(&user("senha do wifi azul e o prazo é sexta"), &known).unwrap();
    assert!(kept.to_string().contains("o prazo é sexta"), "texto comum não é apagado");
}

#[test]
fn mensagem_gigante_e_cortada_no_teto_por_mensagem() {
    let huge = "ç".repeat(MAX_MESSAGE_BYTES);
    let clean = sanitize(&user(&huge), &[]).unwrap();
    let text = clean["content"][0]["text"].as_str().unwrap();
    assert!(size(&clean) <= MAX_MESSAGE_BYTES, "o teto vale para o JSON inteiro: {}", size(&clean));
    assert!(text.ends_with(TRUNCATED.trim_start()));
    let args = json!({"a": "x".repeat(MAX_MESSAGE_BYTES * 2)});
    let call = json!({"role": "assistant", "api": "a", "provider": "p", "model": "m", "stopReason": "toolUse",
        "content": [{"type": "toolCall", "id": "c", "name": "t", "arguments": args}]});
    let clean = sanitize(&call, &[]).unwrap();
    assert_eq!(
        clean["content"][0]["arguments"],
        json!({}),
        "argumentos acima do teto viram objeto vazio, a chamada fica"
    );
    assert!(size(&clean) <= MAX_MESSAGE_BYTES);
}

#[test]
fn coleta_do_turno_nunca_passa_do_teto_e_nunca_guarda_chamada_pela_metade() {
    let mut log = TurnLog::new(Vec::new());
    log.push(&user("pedido"));
    let block = "x".repeat(MAX_MESSAGE_BYTES / 2);
    for i in 0..20 {
        log.push(&call(&format!("c{i}"), &json!({"n": i})));
        log.push(&result(&format!("c{i}"), &block));
    }
    let (kept, dropped) = log.take();
    let bytes = array_bytes(kept.iter().map(size).sum(), kept.len());
    assert!(bytes <= MAX_HISTORY_BYTES, "{bytes}");
    assert!(dropped, "a coleta descartou grupos: o processo vivo precisa da cópia");
    assert_eq!(kept[0]["role"], "user", "o pedido fica");
    assert_eq!(kept.last().unwrap()["toolCallId"], "c19", "o grupo mais novo fica; saem os mais antigos");
    assert_paired(&kept);

    let mut many = TurnLog::new(Vec::new());
    for i in 0..(MAX_HISTORY_MESSAGES + 500) {
        many.push(&user(&i.to_string()));
    }
    let (kept, dropped) = many.take();
    assert_eq!(kept.len(), MAX_HISTORY_MESSAGES);
    assert!(dropped);

    let mut orphan = TurnLog::new(Vec::new());
    orphan.push(&user("pedido"));
    orphan.push(&result("nao-houve-chamada", "solto"));
    let (kept, dropped) = orphan.take();
    assert_eq!(kept.len(), 1, "resultado sem a chamada não entra");
    assert!(dropped);
}

#[test]
fn recorte_tira_a_volta_mais_antiga_inteira_e_marca_a_ressincronizacao() {
    let history = History::default();
    history.remember((
        vec![sanitize(&user("primeira"), &[]).unwrap(), sanitize(&assistant("primeira"), &[]).unwrap()],
        false,
    ));
    assert!(!history.take_resync(), "abaixo do teto nada sai");
    let grande = "x".repeat(MAX_HISTORY_BYTES / 2 - 4096);
    for _ in 0..2 {
        history.remember((vec![user(&grande), assistant(&grande)], false));
    }
    let kept = history.snapshot();
    let bytes = array_bytes(kept.iter().map(size).sum(), kept.len());
    assert!(bytes <= MAX_HISTORY_BYTES, "o teto é rígido: {bytes}");
    assert_eq!(kept[0]["role"], "user", "o corte para numa mensagem de usuário");
    assert!(!texts(&history).contains(&"primeira".to_owned()), "a volta mais antiga saiu primeiro");
    assert!(history.take_resync(), "o processo vivo ainda tem a volta cortada: precisa da cópia nova");
    assert!(!history.take_resync(), "uma vez só");
}

#[test]
fn volta_sozinha_acima_do_teto_guarda_o_pedido_e_nao_trava_a_proxima() {
    let history = History::default();
    let mut turn = vec![sanitize(&user("pedido enorme"), &[]).unwrap()];
    for i in 0..10 {
        turn.push(json!({"role": "assistant", "api": "a", "provider": "p", "model": "m", "stopReason": "stop",
            "content": [{"type": "text", "text": format!("{i}{}", "y".repeat(MAX_MESSAGE_BYTES - 2048))}], "timestamp": 1}));
    }
    history.remember((turn, false));
    let kept = history.snapshot();
    assert!(array_bytes(kept.iter().map(size).sum(), kept.len()) <= MAX_HISTORY_BYTES);
    assert_eq!(kept[0]["content"][0]["text"], "pedido enorme");
    assert!(kept.len() >= 2, "o que couber do turno fica");
    assert!(kept.last().unwrap()["content"][0]["text"].as_str().unwrap().starts_with('9'), "e é o mais novo");

    history.remember((vec![sanitize(&user("proxima"), &[]).unwrap(), sanitize(&assistant("ok"), &[]).unwrap()], false));
    let texts = texts(&history);
    assert!(texts.iter().any(|t| t == "proxima") && texts.iter().any(|t| t == "ok"), "a conversa segue: {texts:?}");
    let kept = history.snapshot();
    assert!(array_bytes(kept.iter().map(size).sum(), kept.len()) <= MAX_HISTORY_BYTES);
}

#[test]
fn coleta_que_descartou_marca_a_ressincronizacao_mesmo_sem_recorte_no_historico() {
    let history = History::default();
    history.remember((vec![sanitize(&user("pequena"), &[]).unwrap()], true));
    assert!(history.take_resync(), "o descarte da coleta também manda a cópia ao processo vivo");
}

#[test]
fn volta_com_chamada_e_resultados_sai_inteira_nunca_pela_metade() {
    let history = History::default();
    let block = "z".repeat(MAX_MESSAGE_BYTES / 2);
    let mut turn = vec![sanitize(&user("pedido"), &[]).unwrap()];
    for i in 0..20 {
        turn.push(sanitize(&call(&format!("c{i}"), &json!({})), &[]).unwrap());
        turn.push(sanitize(&result(&format!("c{i}"), &block), &[]).unwrap());
    }
    history.remember((turn, false));
    let kept = history.snapshot();
    assert!(array_bytes(kept.iter().map(size).sum(), kept.len()) <= MAX_HISTORY_BYTES);
    assert_eq!(kept[0]["role"], "user");
    assert_paired(&kept);
    assert!(history.take_resync());
}

#[test]
fn credenciais_nos_campos_dos_argumentos_nos_metadados_e_no_uso_ficam_de_fora() {
    let known = vec!["chave-do-cofre-do-provedor".to_owned()];
    let message = json!({"role": "assistant", "api": "faux chave-do-cofre-do-provedor", "provider": "p token=abc12345678",
        "model": "m", "stopReason": "toolUse", "timestamp": 2,
        "usage": {"input": 3, "responseNote": "chave-do-cofre-do-provedor", "cost": {"total": 0.5, "memo": "senha: Zx9-valor"}},
        "content": [{"type": "toolCall", "id": "c1", "name": "eco", "arguments": {
            "password": "Zx9-valor-9", "Api-Key": "zzzz-1234-abcd", "usuario": "ana",
            "headers": {"Authorization": "Basic dXNlcjpwYXNz", "cookie": "sid=abcd1234efgh", "x": "chave-do-cofre-do-provedor"},
            "lista": [{"client_secret": "segredo-do-cliente-77"}]}}]});
    let clean = sanitize(&message, &known).unwrap();
    let wire = clean.to_string();
    for secret in [
        "chave-do-cofre-do-provedor",
        "abc12345678",
        "Zx9-valor",
        "zzzz-1234-abcd",
        "dXNlcjpwYXNz",
        "abcd1234efgh",
        "segredo-do-cliente-77",
    ] {
        assert!(!wire.contains(secret), "{secret} ficou na cópia: {wire}");
    }
    assert_eq!(clean["api"], "", "rótulo com texto livre não é guardado");
    assert_eq!(clean["provider"], "");
    assert_eq!(clean["model"], "m");
    assert_eq!(clean["stopReason"], "toolUse");
    let zero = json!({"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0.5});
    assert_eq!(clean["usage"]["input"], 3);
    assert_eq!(clean["usage"]["cost"], zero, "uso e custo: só os campos permitidos, sempre os mesmos");
    assert_eq!(clean["content"][0]["arguments"]["usuario"], "ana", "dado comum fica");

    let long = "n".repeat(MAX_LABEL + 1);
    assert!(sanitize(&call(&long, &json!({})), &[]).is_none(), "id acima do teto: a mensagem não é guardada");
}

#[test]
fn rotulos_ids_e_uso_so_guardam_campos_permitidos_e_o_que_a_reducao_nao_altera() {
    let key = "chave-do-cofre-do-provedor".to_owned();
    let known = vec![key.clone()];
    // Um rótulo que parece identificador mas é a chave do cofre: a redação o alteraria, então não é guardado.
    let mut message = assistant("ok");
    message["provider"] = key.clone().into();
    message["model"] = format!("m-{key}").into();
    let clean = sanitize(&message, &known).unwrap();
    assert_eq!(clean["provider"], "");
    assert_eq!(clean["model"], "");
    assert_eq!(clean["api"], "faux", "rótulo comum fica");
    // Id ou nome de ferramenta com credencial: a mensagem inteira não é guardada.
    assert!(sanitize(&call(&key, &json!({})), &known).is_none());
    assert!(sanitize(&result(&key, "x"), &known).is_none());
    // Motivo de parada fora dos conhecidos não vira conversa.
    let mut odd = assistant("ok");
    odd["stopReason"] = "qualquer-coisa".into();
    assert!(sanitize(&odd, &[]).is_none());
    let mut length = assistant("ok");
    length["stopReason"] = "length".into();
    assert_eq!(sanitize(&length, &[]).unwrap()["stopReason"], "length");
    // O uso só tem os campos permitidos, sempre números, e nunca leva texto nem chaves a mais.
    let mut crowded = assistant("ok");
    crowded["usage"] =
        json!({"input": 1.0, "output": -4, "cacheRead": "texto", "extra": 9, "cost": {"total": 0.25, "x": 1}});
    let usage = &sanitize(&crowded, &[]).unwrap()["usage"];
    assert_eq!(usage["input"], 1);
    assert_eq!(usage["output"], 0, "negativo vira 0");
    assert_eq!(usage["cacheRead"], 0, "texto vira 0");
    assert!(usage.get("extra").is_none() && usage["cost"].get("x").is_none());
    assert_eq!(usage["cost"]["total"], 0.25);
}

#[test]
fn mensagem_que_o_json_escapa_continua_dentro_do_teto_e_e_guardada() {
    // Cada aspas vira duas no JSON: o corte bruto sozinho passaria do teto.
    let quotes = "\"".repeat(MAX_MESSAGE_BYTES);
    let clean = sanitize(&user(&quotes), &[]).expect("encolhe em vez de descartar a mensagem");
    assert!(size(&clean) <= MAX_MESSAGE_BYTES, "{}", size(&clean));
    let controls = "\u{1}".repeat(MAX_MESSAGE_BYTES);
    let clean = sanitize(&user(&controls), &[]).expect("controles viram \\u0001 no JSON: seis bytes cada");
    assert!(size(&clean) <= MAX_MESSAGE_BYTES, "{}", size(&clean));
}

#[test]
fn a_medida_e_o_array_json_em_bytes_utf8() {
    let messages: Vec<Value> = ["a", "ç", "日本語", "😀"].iter().map(|t| sanitize(&user(t), &[]).unwrap()).collect();
    let sum: usize = messages.iter().map(size).sum();
    assert_eq!(array_bytes(sum, messages.len()), Value::Array(messages.clone()).to_string().len());
    assert_eq!(array_bytes(0, 0), 2, "o array vazio é []");
    assert_eq!(array_bytes(sum, 1), 2 + sum);
}

/// O Hub e o motor medem os MESMOS bytes: cada mensagem cruza a fronteira como o texto JSON que o Hub mediu, e o
/// Receptor real do motor (`parseHistory`) mede esse texto, sem serializar de novo. O número 1e20 é impresso
/// Com 4 bytes pelo Hub e com 21 pelo Node: uma nova serialização no receptor recusaria este histórico, que o Hub
/// Guardou exatamente no teto.
#[test]
fn o_receptor_do_motor_aceita_o_historico_que_o_hub_fechou_exatamente_no_teto() {
    if !crate::fork::motor_mexerica::tests::require_motor() {
        return;
    }
    let weird = "aspas \" barra \\ nova\nlinha tab\t ctrl\u{1}\u{1f} ç ñ 日本語 😀 \u{7f} \u{2028} fim";
    let mut messages = vec![
        sanitize(&call("c1", &json!({"n": 1e20, "texto": weird})), &[]).unwrap(),
        sanitize(&result("c1", "feito"), &[]).unwrap(),
    ];
    let empty = size(&sanitize(&user(""), &[]).unwrap());
    for _ in 0..10 {
        messages.push(sanitize(&user(&"e".repeat(200_000 - empty)), &[]).unwrap());
    }
    let used: usize = messages.iter().map(size).sum();
    let last = MAX_HISTORY_BYTES - 2 - messages.len() - used;
    messages.push(sanitize(&user(&"e".repeat(last - empty)), &[]).unwrap());
    let used: usize = messages.iter().map(size).sum();
    assert_eq!(array_bytes(used, messages.len()), MAX_HISTORY_BYTES, "o Hub fecha o array exatamente no teto");

    let receive = |messages: &[Value]| {
        let wire: Vec<String> = messages.iter().map(Value::to_string).collect();
        let history = crate::fork::motor_mexerica::entry_path().with_file_name("src").join("history.ts");
        let mut node = std::process::Command::new(crate::fork::motor_mexerica::node_path().unwrap())
            .args([
                "-e",
                "import(process.argv[1]).then(m=>{let s='';process.stdin.setEncoding('utf8');process.stdin.on('data',d=>s+=d).on('end',()=>{try{console.log('ok '+m.parseHistory(JSON.parse(s)).length)}catch(e){console.log('erro '+e.message)}})})",
            ])
            .arg(history)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut node.stdin.take().unwrap(), serde_json::to_string(&wire).unwrap().as_bytes())
            .unwrap();
        String::from_utf8(node.wait_with_output().unwrap().stdout).unwrap().trim().to_owned()
    };
    assert_eq!(receive(&messages), format!("ok {}", messages.len()), "o receptor aceita o que o Hub fechou no teto");
    let mut over = messages.clone();
    over.push(sanitize(&user("x"), &[]).unwrap());
    assert!(receive(&over).starts_with("erro "), "um item a mais passa do teto e o receptor recusa");
}

#[test]
fn resultado_so_entra_para_chamada_aberta_uma_vez_e_com_o_mesmo_nome() {
    let mut log = TurnLog::new(Vec::new());
    log.push(&user("pedido"));
    log.push(&call("c1", &json!({})));
    log.push(&result("c1", "primeiro"));
    log.push(&result("c1", "segundo"));
    let mut renamed = result("c2", "outro nome");
    renamed["toolName"] = "outra".into();
    log.push(&call("c2", &json!({})));
    log.push(&renamed);
    let (kept, dropped) = log.take();
    assert!(dropped, "o que não casa com uma chamada aberta é descartado e a cópia fica marcada");
    let roles: Vec<&str> = kept.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(
        roles,
        ["user", "assistant", "toolResult", "assistant", "toolResult"],
        "sem o segundo resultado do mesmo id; a chamada c2, cujo resultado de outro nome foi recusado, ganha o resultado desconhecido"
    );
    assert_eq!(kept[2]["content"][0]["text"], "primeiro");

    let twice = json!({"role": "assistant", "api": "faux", "provider": "faux", "model": "faux-1", "stopReason": "toolUse",
        "timestamp": 2, "content": [
            {"type": "toolCall", "id": "d", "name": "eco", "arguments": {}},
            {"type": "toolCall", "id": "d", "name": "eco", "arguments": {}}]});
    assert!(sanitize(&twice, &[]).is_none(), "o mesmo id duas vezes na mesma mensagem não é guardado");
}

#[test]
fn volta_de_ferramentas_que_nao_terminou_ganha_resultado_desconhecido() {
    let two = json!({"role": "assistant", "api": "faux", "provider": "faux", "model": "faux-1", "stopReason": "toolUse",
        "timestamp": 2, "content": [
            {"type": "toolCall", "id": "a", "name": "eco", "arguments": {}},
            {"type": "toolCall", "id": "b", "name": "eco", "arguments": {}}]});
    let mut log = TurnLog::new(Vec::new());
    log.push(&user("pedido"));
    log.push(&two);
    log.push(&result("a", "so um chegou"));
    let (kept, dropped) = log.take();
    assert!(dropped, "a volta aberta marca a cópia para a ressincronização");
    assert_paired(&kept);
    let roles: Vec<&str> = kept.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant", "toolResult", "toolResult"]);
    assert_eq!(kept[2]["content"][0]["text"], "so um chegou", "o resultado que chegou fica como veio");
    assert_eq!(kept[3]["toolCallId"], "b");
    assert_eq!(kept[3]["isError"], true);
    assert!(kept[3]["content"][0]["text"].as_str().unwrap().contains("resultado desconhecido"));

    let mut log = TurnLog::new(Vec::new());
    log.push(&user("pedido"));
    log.push(&call("c1", &json!({})));
    log.push(&result("c1", "ok"));
    let (kept, dropped) = log.take();
    assert!(!dropped && kept.len() == 3, "a volta fechada fica inteira");

    let mut log = TurnLog::new(Vec::new());
    log.push(&user("pedido"));
    log.push(&call("c1", &json!({})));
    log.push(&user("outra mensagem chegou antes do resultado"));
    let (kept, dropped) = log.take();
    assert!(dropped, "uma mensagem nova fecha a volta: a chamada sem resultado é fechada na hora");
    assert_paired(&kept);
    assert_eq!(kept[2]["toolCallId"], "c1");
    assert_eq!(kept[3]["role"], "user");
}

#[tokio::test]
async fn a_limpeza_periodica_roda_enquanto_ha_conversas_guardadas() {
    drop(create(&owner("bot-limpeza", "id"), "sessao").unwrap());
    assert!(sweeper_running());
}

#[test]
fn credencial_trocada_apaga_a_copia_so_do_provedor_que_a_recebeu() {
    let history = History::default();
    assert!(history.bind_credential("faux", "geracao-1"));
    history.remember((vec![sanitize(&user("antiga"), &[]).unwrap()], false));
    assert!(history.bind_credential("faux", "geracao-1"), "a mesma geração segue");
    assert!(history.bind_credential("faux2", "outra"), "provedor novo recebe a conversa e entra no registro");
    assert!(history.bind_credential("outro", ""), "sem credencial não há o que vincular");
    assert_eq!(texts(&history), ["antiga"]);

    assert!(!history.bind_credential("faux", "geracao-2"), "a credencial do provedor mudou");
    assert!(history.snapshot().is_empty(), "a cópia é apagada");
    assert!(history.take_resync(), "o processo vivo recebe a lista vazia");
    assert!(history.bind_credential("faux2", "outra"), "recomeça: só a geração nova é a conhecida");
    assert!(history.bind_credential("faux", "geracao-2"));
}

#[test]
fn coleta_sem_protecao_nao_guarda_nada() {
    let mut log = TurnLog::unprotected();
    log.push(&user("segredo-do-cliente"));
    log.push(&assistant("resposta"));
    let (kept, dropped) = log.take();
    assert!(kept.is_empty() && !dropped);

    let history = History::default();
    history.remember((vec![sanitize(&user("antes"), &[]).unwrap()], false));
    history.drop_unprotected();
    assert!(history.snapshot().is_empty(), "sem proteção a cópia do turno não fica, nem a anterior");
}

#[test]
fn arquivo_so_reabre_para_o_mesmo_dono_e_apaga_so_a_sessao_invalidada() {
    let mut archive = Archive::new();
    let account_a = owner("bot-a", "space|conta-1|rev-1");
    let history = insert(&mut archive, &account_a, "sessao-1").unwrap();
    history.remember((vec![sanitize(&user("guardada"), &[]).unwrap()], false));
    let again = lookup(&mut archive, &account_a, "sessao-1").expect("o mesmo dono reabre a própria sessão");
    assert_eq!(texts(&again), ["guardada"]);
    for (other, why) in [
        (owner("bot-b", "space|conta-1|rev-1"), "outro bot nunca reabre"),
        (owner("bot-a", "space|conta-2|rev-1"), "outra conta nunca reabre"),
        (owner("bot-a", "outro-space|conta-1|rev-1"), "outro Space nunca reabre"),
        (owner("bot-a", "space|conta-1|rev-2"), "outra revisão nunca reabre"),
    ] {
        assert!(lookup(&mut archive, &other, "sessao-1").is_none(), "{why}");
    }
    assert!(lookup(&mut archive, &account_a, "sessao-desconhecida").is_none());

    // Duas conversas independentes do mesmo bot (a principal e uma extra) e um bot de nome parecido.
    let extra = insert(&mut archive, &account_a, "sessao-2").unwrap();
    extra.remember((vec![sanitize(&user("outra conversa"), &[]).unwrap()], false));
    insert(&mut archive, &owner("bot-ab", "id"), "sessao-1").unwrap();

    forget(&mut archive, "bot-a", "sessao-1");
    assert!(lookup(&mut archive, &account_a, "sessao-1").is_none(), "a sessão invalidada perde a cópia");
    assert!(history.snapshot().is_empty(), "e quem ainda a segura não a vê mais");
    let kept = lookup(&mut archive, &account_a, "sessao-2").expect("a outra conversa do bot fica");
    assert_eq!(texts(&kept), ["outra conversa"]);
    assert!(lookup(&mut archive, &owner("bot-ab", "id"), "sessao-1").is_some(), "o prefixo não pega o bot vizinho");
}

#[test]
fn o_prazo_sem_uso_conta_da_ultima_atividade_e_vale_na_hora() {
    let mut archive = Archive::new();
    let ttl = Duration::from_secs(3600);
    let stale = Duration::from_secs(7200);
    insert(&mut archive, "bot-prazo", "parada").unwrap().backdate(stale);
    insert(&mut archive, "bot-prazo", "ativa").unwrap().backdate(stale);
    drop(lookup(&mut archive, "bot-prazo", "ativa")); // a atividade (reabertura) renova o prazo
    purge_with(&mut archive, 0, ttl);
    assert!(
        !archive.contains_key(&("bot-prazo".to_owned(), "parada".to_owned())),
        "sem atividade além do prazo: expirou"
    );
    assert!(archive.contains_key(&("bot-prazo".to_owned(), "ativa".to_owned())), "a reabertura renovou o prazo: fica");

    let history = History::default();
    history.backdate(stale);
    assert!(history.idle() >= stale);
    history.remember((Vec::new(), false));
    assert!(history.idle() < ttl, "guardar um turno é atividade");
    history.backdate(stale);
    history.touch();
    assert!(history.idle() < ttl, "fechar a sessão também");
}

#[test]
fn arquivo_tem_teto_de_verdade_mesmo_com_sessoes_vivas() {
    let mut archive = Archive::new();
    let alive: Vec<_> =
        (0..MAX_SESSIONS).map(|i| insert(&mut archive, "bot-lote", &format!("s{i}")).unwrap()).collect();
    assert_eq!(archive.len(), MAX_SESSIONS);
    assert!(insert(&mut archive, "bot-lote", "uma-a-mais").is_err(), "todas as vagas vivas: recusa, não passa do teto");
    assert_eq!(archive.len(), MAX_SESSIONS);

    drop(alive);
    let nova = insert(&mut archive, "bot-lote", "agora-cabe").unwrap();
    assert!(archive.len() <= MAX_SESSIONS, "{}", archive.len());
    assert!(lookup(&mut archive, "bot-lote", "agora-cabe").is_some());
    drop(nova);
    for i in 0..(MAX_SESSIONS * 2) {
        insert(&mut archive, "bot-lote", &format!("lote{i}")).unwrap();
        assert!(archive.len() <= MAX_SESSIONS);
    }
}

#[test]
fn has_content_falha_fechado_com_mutex_envenenado_e_restart_nao_pre_vincula_proprio() {
    let history = History::default();
    assert!(!history.has_content());
    history.remember((vec![sanitize(&user("algo"), &[]).unwrap()], false));
    assert!(history.has_content());

    history.restart_for("proprio-abcdef12", "geracao-1");
    assert!(!history.has_content());
    // Depois do recomeço em endereço próprio, outro provedor conversa primeiro: o endereço próprio
    // não pode ficar pré-vinculado como recebedor antes de realmente atender um turno.
    history.remember((vec![sanitize(&user("privado-no-faux"), &[]).unwrap()], false));
    assert!(!history.received_by("proprio-abcdef12"));

    let poisoned = Arc::new(History::default());
    let clone = poisoned.clone();
    let _ = std::panic::catch_unwind(move || {
        let _guard = clone.state.lock().unwrap();
        panic!("envenena o mutex");
    });
    assert!(poisoned.has_content(), "falha de lock deve presumir conteúdo (fail-closed)");
    assert!(poisoned.would_reset_credential("faux", "geracao-1"));
}
