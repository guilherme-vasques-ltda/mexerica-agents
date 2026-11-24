use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Usage {
    totals: Option<[u64; 4]>,
    invalid: bool,
}

impl Usage {
    pub(super) fn observe(&mut self, message: &Value) {
        if self.invalid || message["role"] != "assistant" {
            return;
        }
        let mut total = self.totals.unwrap_or_default();
        for (total, key) in total.iter_mut().zip(["input", "cacheRead", "cacheWrite", "output"]) {
            let Some(sum) = message["usage"][key].as_u64().and_then(|count| total.checked_add(count)) else {
                self.invalid = true;
                self.totals = None;
                return;
            };
            *total = sum;
        }
        self.totals = Some(total);
    }

    pub(super) fn include(self, reply: &mut Value) {
        if let Some([input, read, write, output]) = self.totals {
            reply["usage"] = json!({
                "inputTokens": input, "cachedReadTokens": read, "cachedWriteTokens": write, "outputTokens": output,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrega_respostas_distintas_sem_contar_resultados_de_ferramentas() {
        let mut usage = Usage::default();
        for _ in 0..2 {
            usage.observe(&json!({"role":"assistant", "usage":{"input":10,"cacheRead":3,"cacheWrite":4,"output":5}}));
            usage.observe(&json!({"role":"toolResult"}));
        }
        let mut reply = json!({});
        usage.include(&mut reply);
        assert_eq!(
            reply["usage"],
            json!({"inputTokens":20,"cachedReadTokens":6,"cachedWriteTokens":8,"outputTokens":10})
        );
    }

    #[test]
    fn resposta_parcial_nao_registra_total_menor_que_o_real() {
        let mut usage = Usage::default();
        usage.observe(&json!({"role":"assistant", "usage":{"input":10,"cacheRead":3,"cacheWrite":4,"output":5}}));
        usage.observe(&json!({"role":"assistant", "usage":{"input":10,"output":5}}));
        let mut reply = json!({});
        usage.include(&mut reply);
        assert_eq!(reply, json!({}));
    }
}
