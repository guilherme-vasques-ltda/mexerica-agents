//! D273: UI generativa no chat. G1a (contrato, storage, procedência) + G1b (`flow`: ferramenta
//! autenticada, publicação, resposta por CAS e entrega à fila de Sessões), escolha v1 e gráfico de leitura v1
//! (`charts`, `chart_store`: só mostra, sem resposta), na conversa capturada e na extra selada do Pessoal (C6). As APIs de storage não autenticam execução: só `flow` as chama.
pub(crate) mod chart_store;
pub(crate) mod charts;
pub(crate) mod contract;
pub(crate) mod flow;
pub(crate) mod storage;

use serde_json::Value;

/// Marca conservadora de procedência: nunca torna uma resposta prova autoral.
/// Somente o produtor interno insere esse objeto; não depende do rótulo escolhido.
pub(crate) fn is_choice_response(data: &Value) -> bool {
    data.get("choice_response").is_some()
}

/// Chaves server-owned: a marca de procedência da resposta e as projeções do cartão e do gráfico (G1b).
const OWNED: [&str; 3] = ["choice_response", "genuiChoice", "genuiChart"];

/// Atualização genérica não cria, troca nem remove as chaves emitidas pelo Hub.
pub(crate) fn preserve_response(before: &Value, mut after: Value) -> Value {
    if let Some(object) = after.as_object_mut() {
        for key in OWNED {
            object.remove(key);
        }
    }
    for key in OWNED {
        if let Some(mark) = before.get(key) {
            // Uma atualização inválida também não pode apagar a procedência.
            if !after.is_object() {
                after = serde_json::json!({});
            }
            after[key] = mark.clone();
        }
    }
    after
}

#[cfg(test)]
mod tests;
