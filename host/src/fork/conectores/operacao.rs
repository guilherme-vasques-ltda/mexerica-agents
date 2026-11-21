//! Contrato fechado de uma chamada; IDs de transporte nunca são concessões.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Write as _};

pub(super) fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Objetos ordenados recursivamente; números preservam a representação do parser
/// (1 e 1.0 não ganham equivalência permissiva); arrays mantêm ordem.
pub(crate) fn digest(value: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: BTreeMap<_, _> = map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
                serde_json::to_value(sorted).expect("objeto JSON representável")
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    Sha256::digest(canonical(value).to_string().as_bytes()).iter().fold(String::with_capacity(64), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// Teto da descrição de uma ferramenta, em bytes. Acima dele o texto não chega ao modelo nem pode ser aprovado.
pub(crate) const DESCRICAO_MAX: usize = 8 * 1024;

/// 2: apresentação com conta de saída e rótulos curados. App que só conhece o 1 não
/// exibiu a conta: não mostra botões e o Hub recusa sua decisão/edição.
pub(crate) const DECISION_PROTOCOL: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Estado {
    Aguardando,
    Autorizada,
    EmEnvio,
    Recusada,
    Expirada,
    Cancelada,
    Concluida,
    FalhaComprovada,
    Incerta,
}
impl Estado {
    pub(crate) fn terminal(self) -> bool {
        !matches!(self, Self::Aguardando | Self::Autorizada | Self::EmEnvio)
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Aguardando => "aguardando",
            Self::Autorizada => "autorizada",
            Self::EmEnvio => "em_envio",
            Self::Recusada => "recusada",
            Self::Expirada => "expirada",
            Self::Cancelada => "cancelada",
            Self::Concluida => "concluida",
            Self::FalhaComprovada => "falha_comprovada",
            Self::Incerta => "incerta",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SnapshotAcesso {
    pub instance: String,
    /// Binding.generation canônica, UUID opaco copiado sem normalizar ou converter.
    pub generation: String,
    pub profile: String,
    /// Binding.revision canônica; a revisão inicial zero é válida.
    pub config_revision: u64,
    pub curation_revision: u64,
    pub access_revision: u64,
    pub bot: String,
    pub rules: BTreeMap<String, Regra>,
    /// Conta de saída: só IDs opacos da fonte canônica. Ausente em ledger antigo
    /// (leitura do recibo); a admissão nova exige `Some`. Número/e-mail nunca ficam aqui.
    #[serde(default)]
    pub conta: Option<VinculoConta>,
}

/// Vínculo durável e opaco da conta de saída: ID canônico e encarnação (repareamento muda).
/// Sem hash da identidade legível: número de telefone teria digest reversível por força bruta.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VinculoConta {
    pub conta: String,
    pub encarnacao: String,
}

/// Conta de saída legível. Só a fonte canônica do Hub a constrói (sem `Deserialize`):
/// app, modelo, import, argumentos e rótulo do bot não declaram conta nem identidade.
/// Fica em memória no Hub e no privado cifrado; nunca em Entry, voz, logs ou ledger.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ContaSaida {
    pub vinculo: VinculoConta,
    pub rotulo: String,
    /// Número/e-mail/login fornecido pelo produtor confiável daquele serviço.
    pub identidade: String,
}
impl ContaSaida {
    pub(super) fn validate(&self) -> Result<()> {
        let curto = |v: &str, max: usize| !v.trim().is_empty() && v.len() <= max && !v.chars().any(char::is_control);
        ensure!(
            curto(&self.vinculo.conta, 128) && curto(&self.vinculo.encarnacao, 128),
            "conta_saida_nao_identificada: vínculo canônico da conta ausente"
        );
        ensure!(
            curto(&self.identidade, 320) && curto(&self.rotulo, 80),
            "conta_saida_nao_identificada: identidade da conta de saída ausente"
        );
        Ok(())
    }
    pub(super) fn presentation(&self) -> Value {
        json!({"rotulo":self.rotulo,"identidade":self.identidade})
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Regra {
    pub schema: Value,
    pub schema_digest: String,
    /// Subconjunto fechado: campos string obrigatórios, limites e valores fixos.
    /// Schemas fora desta gramática não se tornam permissivos.
    pub fields: BTreeMap<String, Campo>,
    /// Ação curada em português (ex.: "Enviar mensagem no `WhatsApp`"); título do cartão.
    pub presentation: String,
    /// Descrição aprovada, byte a byte: a única que o modelo recebe. A do fornecedor só passa se for idêntica.
    /// Default só para ler ledger antigo; vazia recusa a admissão nova.
    #[serde(default)]
    pub descricao: String,
    pub evidence: Evidencia,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Campo {
    pub max_bytes: usize,
    pub editable: bool,
    pub allowed: Option<Vec<String>>,
    /// Rótulo curado do campo no cartão (ex.: "Mensagem"). Não concede acesso nem edição.
    /// Default só para ler ledger antigo; vazio recusa a admissão nova.
    #[serde(default)]
    pub rotulo: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Evidencia {
    Inconclusiva,
    /// Contrato exclusivamente sintético. Não classifica fornecedores reais.
    #[cfg(test)]
    Fixture,
}

impl SnapshotAcesso {
    pub(crate) fn same_grant(&self, other: &Self) -> bool {
        self.instance == other.instance
            && self.generation == other.generation
            && self.profile == other.profile
            && self.config_revision == other.config_revision
            && self.curation_revision == other.curation_revision
            && self.access_revision == other.access_revision
            && self.bot == other.bot
            && digest(&json!(self.rules)) == digest(&json!(other.rules))
            && self.conta == other.conta
    }
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(!self.instance.is_empty() && !self.profile.is_empty() && !self.bot.is_empty(), "Fonte incompleta");
        ensure!(
            self.generation.len() == 36 && uuid::Uuid::parse_str(&self.generation).is_ok(),
            "Geração da fonte inválida"
        );
        ensure!(self.curation_revision > 0 && self.access_revision > 0, "Revisões da fonte indisponíveis");
        ensure!(!self.rules.is_empty(), "Acesso indisponível");
        ensure!(self.conta.is_some(), "conta_saida_nao_identificada: fonte sem conta de saída canônica");
        for rule in self.rules.values() {
            ensure!(digest(&rule.schema) == rule.schema_digest, "Schema curado divergente");
            if rule.descricao.trim().is_empty() || rule.descricao.len() > DESCRICAO_MAX {
                tracing::warn!(instance = %self.instance, "descrição aprovada ausente ou acima do teto");
                bail!("Descrição aprovada ausente ou acima do teto");
            }
            let curto = |v: &str, max: usize| !v.trim().is_empty() && v.len() <= max;
            ensure!(
                curto(&rule.presentation, 120) && rule.fields.values().all(|f| curto(&f.rotulo, 64)),
                "Rótulos curados da ação ausentes"
            );
            // A gramática corresponde integralmente ao schema, sem ignorar restrições remotas.
            let properties: BTreeMap<_, _> = rule
                .fields
                .iter()
                .map(|(name, field)| {
                    let mut schema = json!({"type":"string", "maxLength":field.max_bytes});
                    if let Some(values) = &field.allowed {
                        schema["enum"] = json!(values);
                    }
                    (name, schema)
                })
                .collect();
            let required: Vec<_> = rule.fields.keys().collect();
            let expected =
                json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
            ensure!(digest(&expected) == rule.schema_digest, "Schema fora da gramática curada");
        }
        Ok(())
    }
    pub(super) fn request(&self, conta: &ContaSaida, frame: &Value) -> Result<Privado> {
        ensure!(self.conta.as_ref() == Some(&conta.vinculo), "Conta de saída divergente da concessão");
        ensure!(frame.to_string().len() <= 64 * 1024, "Pedido acima de 64 KiB");
        ensure!(
            frame
                .as_object()
                .is_some_and(|o| o.keys().all(|k| matches!(k.as_str(), "jsonrpc" | "id" | "method" | "params"))),
            "Campos MCP desconhecidos"
        );
        ensure!(frame["method"] == "tools/call", "Chamada obrigatória");
        let params = frame["params"].as_object().ok_or_else(|| anyhow::anyhow!("Parâmetros inválidos"))?;
        ensure!(params.len() == 2, "Somente nome e argumentos são admitidos");
        let tool = frame["params"]["name"].as_str().ok_or_else(|| anyhow::anyhow!("Ferramenta inválida"))?;
        let rule = self.rules.get(tool).ok_or_else(|| anyhow::anyhow!("Ferramenta fora do acesso"))?;
        let args = &frame["params"]["arguments"];
        let object = args.as_object().ok_or_else(|| anyhow::anyhow!("Argumentos devem ser objeto"))?;
        ensure!(object.len() == rule.fields.len(), "Campos fora da curadoria");
        for (name, field) in &rule.fields {
            let value = object.get(name).and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("Campo inválido"))?;
            ensure!(!value.is_empty() && value.len() <= field.max_bytes, "Campo excede limite curado");
            ensure!(
                field.allowed.as_ref().is_none_or(|values| values.iter().any(|v| v == value)),
                "Valor fora da restrição curada"
            );
        }
        // Rótulos e conta vêm só da curadoria/fonte do Hub; o nome técnico segue exato no pedido.
        let rotulos: BTreeMap<_, _> = rule.fields.iter().map(|(name, field)| (name, &field.rotulo)).collect();
        let presentation = json!({"efeito":rule.presentation,"ferramenta":tool,"argumentos":args,
            "rotulos":rotulos,"conta":conta.presentation()});
        ensure!(presentation.to_string().len() <= 16 * 1024, "Apresentação integral excede limite");
        let request_digest =
            digest(&json!({"instance":self.instance,"tool":tool,"arguments":args,"conta":conta.vinculo}));
        Ok(Privado { tool: tool.into(), arguments: args.clone(), presentation, request_digest })
    }
    pub(super) fn classify(&self, tool: &str, frame: &Value) -> Estado {
        match self.rules.get(tool).map(|r| &r.evidence) {
            #[cfg(test)]
            Some(Evidencia::Fixture)
                if frame["result"]["structuredContent"]["fixtureEffect"] == "created"
                    && frame["result"]["isError"] != true =>
            {
                Estado::Concluida
            }
            #[cfg(test)]
            Some(Evidencia::Fixture)
                if frame["result"]["structuredContent"]["fixtureEffect"] == "rejected_without_effect" =>
            {
                Estado::FalhaComprovada
            }
            _ => {
                let _ = frame;
                Estado::Incerta
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Privado {
    pub tool: String,
    pub arguments: Value,
    pub presentation: Value,
    pub request_digest: String,
}
impl Privado {
    pub(super) fn frame(&self, id: &Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":self.tool,"arguments":self.arguments}})
    }
    pub(super) fn edit(&self, source: &SnapshotAcesso, conta: &ContaSaida, patch: &Value) -> Result<Self> {
        let patch = patch.as_object().ok_or_else(|| anyhow::anyhow!("Edição inválida"))?;
        ensure!(!patch.is_empty(), "Edição vazia");
        let rule = source.rules.get(&self.tool).ok_or_else(|| anyhow::anyhow!("Acesso revogado"))?;
        let mut frame = self.frame(&json!("edit"));
        for (name, value) in patch {
            if !rule.fields.get(name).is_some_and(|f| f.editable) {
                bail!("Campo não editável");
            }
            frame["params"]["arguments"][name] = value.clone();
        }
        source.request(conta, &frame)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Tentativa {
    pub id: String,
    pub ingress: String,
    pub original_digest: String,
    pub request_digest: String,
    pub snapshot_id: String,
    pub card_id: String,
    pub rev: i64,
    pub state: Estado,
    pub source: SnapshotAcesso,
    pub context: super::contexto::ContextoExecucao,
    pub t0: i64,
    pub card_deadline: i64,
    pub global_deadline: i64,
    pub admitted_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub decision: Option<String>,
    pub decision_digest: Option<String>,
    pub caller: Option<String>,
    pub delivery: String,
}
impl Tentativa {
    pub(crate) fn slot(&self) -> String {
        format!("donna.conectores.tentativa.{}.privado", self.id)
    }
    pub(crate) fn public(&self) -> Value {
        json!({"author":self.context.bot,"status":if self.state.terminal() { self.state.name() } else { "pending" },
            "text":"Pedido de conector. Confira o recibo no app compatível.","voiceApproval":"app_only",
            "conectorOperation":{"tentativaId":self.id,"cardId":self.card_id,"state":self.state,"decisionProtocol":DECISION_PROTOCOL}})
    }
    pub(crate) fn receipt(&self) -> Value {
        json!({"tentativaId":self.id,"snapshotId":self.snapshot_id,"cardId":self.card_id,"cardRev":self.rev,
            "requestDigest":self.request_digest,"state":self.state,"delivery":self.delivery,
            "t0":self.t0,"cardDeadline":self.card_deadline,"globalDeadline":self.global_deadline,
            "admittedAt":self.admitted_at,"completedAt":self.completed_at,"decisionProtocol":DECISION_PROTOCOL,
            "evidence": match self.state { Estado::Concluida => "sucesso_reportado", Estado::FalhaComprovada => "falha_sem_efeito_comprovada",
                Estado::Incerta => "efeito_desconhecido_confira_antes_de_repetir", _ => "sem_conclusao_externa" }})
    }
}
