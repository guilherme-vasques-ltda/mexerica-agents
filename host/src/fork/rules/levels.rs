//! Textos únicos do Hub. Nenhum cliente precisa inventar rótulos ou promessas de cobertura.
use serde_json::{Value, json};

pub fn contract() -> Value {
    json!({
        "runtimeActive":true,
        "application":"permission_requests",
        "message":"Os pedidos de permissão que chegam à Mexerica passam pelas suas regras: uma regra de Recusar nega na hora, sem cartão. Os demais pedidos aparecem em um cartão para você decidir. Algumas ações do bot podem acontecer sem esse pedido. As novas opções de aprovação continuam em preparação.",
        "niveis":[
          {"id":"ask","rotulo":"Pedir liberação",
           "descricao":"Você decide, em um cartão, cada pedido de permissão que a Mexerica receber. Nem toda ação do bot gera um pedido.",
           "confirmacao":null,"superficies":["general","create","editor","management_card"],"disponibilidadePorMotor":availability()},
          {"id":"auto_approve","rotulo":"Auto aprovar",
           "descricao":"Em preparação: suas regras poderão liberar ações comuns. Ações bloqueadas continuarão bloqueadas; as que precisarem da sua decisão aparecerão em um cartão.",
           "confirmacao":"Quando estiver disponível, Auto aprovar permitirá que suas regras liberem ações comuns. Ações delicadas, como pagar ou apagar sem poder desfazer, continuarão pedindo confirmação.",
           "superficies":["general","create","editor","management_card"],"disponibilidadePorMotor":availability()},
          {"id":"full_access","rotulo":"Acesso total",
           "descricao":"Em preparação: poderá liberar pedidos comuns e reduzir as proteções do programa. Ações delicadas, como pagar ou apagar sem poder desfazer, continuarão pedindo confirmação.",
           "confirmacao":"Acesso total dará mais liberdade a este bot e reduzirá suas proteções. Isso não separa seus arquivos dos outros bots que usam o mesmo computador.",
           "superficies":["editor"],"disponibilidadePorMotor":availability()}
        ],
        "disponibilidadePorMotor":availability(),
        "floor":[
          {"id":"payment","rotulo":"Pagar ou transferir dinheiro"},
          {"id":"destructive","rotulo":"Apagar ou substituir algo sem poder desfazer"},
          {"id":"admin","rotulo":"Obter permissões de administrador"},
          {"id":"secrets","rotulo":"Senhas e chaves de acesso"},
          {"id":"hub_folder","rotulo":"Arquivos da Mexerica e dos programas de IA"},
          {"id":"hub_api","rotulo":"Usar os controles da Mexerica e dos programas de IA"},
          {"id":"hub_configuration","rotulo":"Mudar ajustes da Mexerica e dos programas de IA"},
          {"id":"execucao_futura","rotulo":"Mudar arquivos que fazem o computador agir depois"},
          {"id":"confirmacao_local","rotulo":"Confirmações no seu computador (em preparação)"},
          {"id":"desconhecida","rotulo":"Ações cujo resultado ainda não está claro"}
        ],
        "dangerousSelectorsAvailable":false,
        "schedulingFloor":{"commands":["crontab","systemd-run","at","launchctl"],"runtimeEnforcement":false},
        "sending":{"typed":true,"savedRulesAvailable":false,"firstTime":"ask","otherBotAndDescendants":"owner_card"},
        "avisosPorPapel":{
          "owner":"Os bots que usam o mesmo computador podem alcançar os mesmos dados. Regras e Acesso total não criam uma separação entre eles.",
          "codex":"Algumas ações do Codex podem acontecer sem passar pela Mexerica. Salvar uma regra não garante que ela será aplicada a essas ações.",
          "localAuthority":"As confirmações previstas para seus computadores ainda estão em preparação. Esta etapa não entrega a confirmação por digital nem novos cartões para tarefas agendadas.",
          "bot":"Uma regra de Recusar sempre vence uma regra de Permitir. As outras confirmações de segurança continuam valendo."
        },
        "max":100
    })
}

fn availability() -> Value {
    json!({"claude":{"available":false,"reason":"A liberação automática por regras ainda depende de provas de efeito por programa. Nesta etapa, os pedidos de permissão continuam aparecendo em cartão."},
      "codex":{"available":false,"reason":"A liberação automática por regras ainda depende de provas de efeito por programa. Nesta etapa, os pedidos de permissão continuam aparecendo em cartão."}})
}
