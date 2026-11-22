//! Identidade de cache da conversa no fornecedor: um valor aleatório, opaco e criado só para isso, que o motor manda
//! como identidade de sessão do pedido (a chave de cache do fornecedor e os cabeçalhos de afinidade). Não é o id da sessão
//! que a ponte usa para controle, nem o id do bot, nem texto da pessoa: vai no corpo e em cabeçalhos para o fornecedor,
//! então não pode identificar nada. Vale para UMA conversa, UMA conta e UM fornecedor: troca de conta (geração da
//! credencial) ou de fornecedor descarta o valor e cria outro, senão a conversa de outra conta reaproveitaria o cache.
#[derive(Default)]
pub(crate) struct IdentidadeDeCache {
    fornecedor: String,
    geracao: String,
    valor: String,
}

impl IdentidadeDeCache {
    /// O valor que o alvo já tem, ou um novo, sem gravar o alvo: quem grava é [`IdentidadeDeCache::fixar`], depois de
    /// o motor aceitar a troca. Uma troca de provedor que falha não invalida o cache do provedor anterior.
    pub(crate) fn propor(&self, fornecedor: &str, geracao: &str) -> String {
        if !self.valor.is_empty() && self.fornecedor == fornecedor && self.geracao == geracao {
            return self.valor.clone();
        }
        uuid::Uuid::new_v4().simple().to_string()
    }

    /// Grava o alvo com o valor aceito pelo motor.
    pub(crate) fn fixar(&mut self, fornecedor: &str, geracao: &str, valor: &str) {
        if self.fornecedor == fornecedor && self.geracao == geracao && self.valor == valor {
            return;
        }
        self.fornecedor.clear();
        self.fornecedor.push_str(fornecedor);
        self.geracao.clear();
        self.geracao.push_str(geracao);
        self.valor.clear();
        self.valor.push_str(valor);
    }

    /// `propor` e `fixar` de uma vez, para quando o processo já está preso ao alvo (o turno em curso).
    pub(crate) fn para(&mut self, fornecedor: &str, geracao: &str) -> String {
        let valor = self.propor(fornecedor, geracao);
        self.fixar(fornecedor, geracao, &valor);
        valor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_mesmo_alvo_repete_o_valor_e_os_outros_o_trocam() {
        let mut cache = IdentidadeDeCache::default();
        let primeiro = cache.para("openai", "geracao-1");
        assert_eq!(cache.para("openai", "geracao-1"), primeiro, "a mesma conversa, conta e fornecedor repetem o valor");
        assert_ne!(cache.para("anthropic", "geracao-1"), primeiro, "outro fornecedor troca o valor");
        let terceiro = cache.para("openai", "geracao-2");
        assert_ne!(terceiro, primeiro, "outra conta troca o valor");
        assert_eq!(cache.para("openai", "geracao-2"), terceiro, "o alvo atual mantém o valor dele");
        assert_ne!(cache.para("openai", "geracao-1"), primeiro, "voltar a um alvo antigo cria outro valor");
    }

    #[test]
    fn propor_nao_grava_o_alvo_antes_do_aceite() {
        let mut cache = IdentidadeDeCache::default();
        let atual = cache.para("openai", "geracao-1");
        // Uma troca proposta para outro provedor e não aceita não invalida o alvo atual.
        let proposta = cache.propor("anthropic", "geracao-1");
        assert_ne!(proposta, atual);
        assert_eq!(cache.propor("openai", "geracao-1"), atual, "o alvo anterior continua com o valor dele");
        cache.fixar("anthropic", "geracao-1", &proposta);
        assert_eq!(cache.propor("anthropic", "geracao-1"), proposta, "o alvo aceito passa a valer");
    }

    #[test]
    fn o_valor_e_opaco_e_nao_carrega_o_alvo() {
        let mut cache = IdentidadeDeCache::default();
        let valor = cache.para("openai", "geracao-secreta-1234");
        assert_eq!(valor.len(), 32, "valor de 32 caracteres, dentro do limite do fornecedor");
        assert!(valor.chars().all(|c| c.is_ascii_hexdigit()), "só hexadecimal: nada de id, caminho ou texto");
        assert!(!valor.contains("openai") && !valor.contains("geracao"));
    }

    #[test]
    fn conversas_diferentes_nao_compartilham_valor() {
        let mut uma = IdentidadeDeCache::default();
        let mut outra = IdentidadeDeCache::default();
        assert_ne!(uma.para("openai", "geracao-1"), outra.para("openai", "geracao-1"));
    }
}
