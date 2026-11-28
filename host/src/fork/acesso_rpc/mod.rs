//! Quem pode chamar cada RPC do Hub. Cada método roteado declara aqui uma classe de acesso; método sem
//! classe é recusado para todo chamador, inclusive este computador (fail-closed). O teste
//! `toda_rpc_roteada_declara_acesso` lê o código dos roteadores e reprova método novo sem classe.
//!
//! A classe é só a primeira porta: as conferências próprias de cada família (`engine_setup::access`,
//! `control_tower`, `onde_roda::autoridade`, `ponte_credencial::conferir`, `require_local` dos handlers)
//! continuam somando depois dela. Os chamadores `Pairing` e `Bootstrap` não passam por esta tabela: o canal de
//! pareamento só aceita `pair` e o de bootstrap tem laço próprio; aqui eles são sempre recusados, e `pair` (classe
//! `Pairing`) é recusado a todos fora daquele canal.
use crate::api::devices::Caller;
use crate::store::Scope;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Acesso {
    /// Só este computador (loopback com o token).
    LocalOnly,
    /// Este computador e a Ponte das ferramentas do bot; nenhum aparelho.
    Ponte,
    /// Este computador e o aparelho com o escopo `owner.devices`.
    OwnerDevices,
    /// Este computador e o aparelho com o escopo `control`.
    Control,
    /// Este computador e o aparelho com o escopo `screen`.
    Screen,
    /// Preferência do próprio app: este computador e qualquer aparelho autorizado, mesmo sem escopo.
    Proprio,
    /// Protocolo dos executores: só o aparelho cujo único escopo é `executor`; nem este computador.
    Executor,
    /// Só o canal de pareamento, que atende `pair` antes de qualquer `dispatch`; aqui ninguém passa.
    Pairing,
}

pub(crate) const TABELA: &[(Acesso, &[&str])] = &[
    (Acesso::LocalOnly, SO_LOCAL),
    (Acesso::Ponte, PONTE),
    (Acesso::OwnerDevices, DONO_DOS_APARELHOS),
    (Acesso::Control, CONTROLE),
    (Acesso::Screen, TELA),
    (Acesso::Proprio, PROPRIO),
    (Acesso::Executor, EXECUTOR),
    (Acesso::Pairing, &["pair"]),
];

/// Métodos do canal E2E que não passam pelo `dispatch` mas consultam `permit` (o teste de cobertura os aceita
/// fora do inventário dos roteadores).
#[cfg(test)]
pub(crate) const DO_CANAL: &[&str] = &["subscribe", "pair"];

/// Nomes que uma porta de produção atende sem consultar esta tabela, com o arquivo da guarda própria. No `dispatch`
/// eles são desconhecidos ou `Pairing` (recusados a todos), salvo quando o `dispatch` também roteia o mesmo nome com a
/// sua classe (`hello`). O teste `toda_porta_de_producao_e_declarada` reprova nome atendido fora daqui.
#[cfg(test)]
pub(crate) const FORA_DA_TABELA: &[(&str, &str)] = &[
    ("pair", "remote/channel.rs"),
    ("hello", "remote/channel.rs"),
    ("bootstrap.challenge", "remote/channel.rs"),
    ("bootstrap.prove", "remote/channel.rs"),
    ("hello", "fork/personal_bootstrap/probe.rs"),
    ("donnaPersonalSessionProof", "fork/personal_bootstrap/probe.rs"),
    ("session.open", "fork/autoridade/personal_channel.rs"),
    ("link.status", "fork/autoridade/personal_channel.rs"),
    ("session.close", "fork/autoridade/personal_channel.rs"),
];

/// Nomes que uma porta fora do `dispatch` lê do pedido e passa por esta tabela (`permit` antes da guarda própria, ou
/// repasse ao `dispatch`).
#[cfg(test)]
pub(crate) const PELA_TABELA: &[(&str, &str)] = &[
    ("browserStreamSubscribe", "remote/channel.rs"),
    ("browserStreamInput", "remote/channel.rs"),
    ("browserStreamAck", "remote/channel.rs"),
    ("browserStreamControl", "remote/channel.rs"),
    ("browserStreamTarget", "remote/channel.rs"),
    ("send", "remote/relay.rs"),
];

/// Arquivos de produção que leem o nome do pedido (`["m"]`) e quantas vezes. Toda leitura é de uma forma que o teste
/// reconhece (comparação com nome, `match`, lista, repasse ao `dispatch` ou quadro opaco do relay); arquivo ou
/// leitura nova reprova até ser declarada.
#[cfg(test)]
pub(crate) const LEITURAS_DO_NOME: &[(&str, usize)] = &[
    ("remote/channel.rs", 5),
    ("remote/relay.rs", 3),
    ("fork/personal_bootstrap/probe.rs", 1),
    ("fork/personal_bootstrap/relay.rs", 1),
];

/// O que guarda uma rota HTTP, conferido no código do handler pelo teste `toda_rota_http_e_declarada`.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Guarda {
    /// Sem credencial: responde a quem alcança a porta; o handler não confere token.
    Publica,
    /// Pública só no mínimo: sem `Authorization` responde a quem alcança a porta; com ele, o que passa do
    /// mínimo exige a chamada nomeada, e credencial recusada não cai na resposta pública.
    PublicaMinima(&'static str),
    /// Token deste computador (ou sessão Web), conferido no handler.
    Bearer,
    /// Autentica e passa pelo `dispatch`, e por ele por esta tabela.
    Dispatch,
    /// Guarda própria: a chamada que o handler faz para conferir.
    Propria(&'static str),
}

/// Rotas HTTP de produção e o que as guarda. Só `/api/*` chega ao `dispatch`.
#[cfg(test)]
pub(crate) const ROTAS_HTTP: &[(&str, &str, Guarda)] = &[
    ("api/http.rs", "/health", Guarda::PublicaMinima("prova_health::aceitar(&hub, nonce, proof, peer)")),
    ("api/http.rs", "/events", Guarda::Bearer),
    ("api/http.rs", "/api/donnaGuardRuntime", Guarda::Dispatch),
    ("api/http.rs", "/api/{method}", Guarda::Dispatch),
    ("api/http.rs", "/term/{id}", Guarda::Bearer),
    ("api/http.rs", "/hooks/routines/{id}", Guarda::Propria("routines.receive(")),
    ("api/http.rs", "/oauth/callback", Guarda::Propria("oauth::finish(")),
    ("api/http.rs", "/computers/enroll-hello", Guarda::Propria("hello(&hub")),
    ("api/http.rs", "/ingest/statusline", Guarda::Bearer),
    ("api/http.rs", "/channel", Guarda::Propria("serve_direct(")),
    ("api/http.rs", "/personal-possession", Guarda::Propria("run(&hub, &mut socket)")),
    ("api/http.rs", "/personal-prepare", Guarda::Propria("run(&hub, &mut socket)")),
    ("fork/web_local.rs", "/web/auth/session", Guarda::Propria("web.base_guard(")),
    ("fork/web_local.rs", "/web/auth/login", Guarda::Propria("web.base_guard(")),
    ("fork/web_local.rs", "/web/auth/logout", Guarda::Propria("web.base_guard(")),
    ("fork/web_local.rs", "fallback", Guarda::Publica),
    ("fork/web_local.rs", "method_not_allowed_fallback", Guarda::Publica),
    ("fork/whatsapp/inbox.rs", "/whatsapp/{bot}", Guarda::Propria("shared.accept(")),
    ("fork/browser/turn.rs", "/mcp", Guarda::Propria("state.token.as_bytes()")),
    ("fork/claude_ponte/permissao.rs", "/mcp", Guarda::Propria("estado.segredo")),
    ("fork/planos_codigo/repasse.rs", "fallback", Guarda::Propria("repasse.encaminhar(")),
];

pub(crate) fn acesso(method: &str) -> Option<Acesso> {
    TABELA.iter().find(|(_, metodos)| metodos.contains(&method)).map(|(acesso, _)| *acesso)
}

/// Nome sem classe: o `dispatch` e a rota HTTP o respondem como desconhecido antes de ler os argumentos, com
/// qualquer prefixo.
pub(crate) fn desconhecido(method: &str) -> bool {
    acesso(method).is_none()
}

pub(crate) fn permite(caller: &Caller, method: &str) -> bool {
    let Some(acesso) = acesso(method) else { return false };
    match caller {
        _ if acesso == Acesso::Pairing => false,
        Caller::Local => acesso != Acesso::Executor,
        Caller::Ponte { .. } => acesso == Acesso::Ponte,
        Caller::Pairing { .. } | Caller::Bootstrap { .. } => false,
        Caller::Device { scopes, .. } if scopes.contains(&Scope::WebChat) => {
            scopes == &[Scope::WebChat]
                && matches!(acesso, Acesso::Control | Acesso::Proprio)
                && WEB_CHAT.contains(&method)
        }
        Caller::Device { scopes, .. } if acesso == Acesso::Executor => scopes == &[Scope::Executor],
        Caller::Device { scopes, .. } if scopes.contains(&Scope::Executor) => false,
        Caller::Device { scopes, .. } => match acesso {
            Acesso::LocalOnly | Acesso::Ponte | Acesso::Executor | Acesso::Pairing => false,
            Acesso::OwnerDevices => scopes.contains(&Scope::OwnerDevices),
            Acesso::Control => scopes.contains(&Scope::Control),
            Acesso::Screen => scopes.contains(&Scope::Screen),
            Acesso::Proprio => true,
        },
    }
}

pub(crate) fn permite_assinatura(caller: &Caller, tipo: &str) -> bool {
    match caller {
        Caller::Device { scopes, .. } if scopes.contains(&Scope::WebChat) => {
            scopes == &[Scope::WebChat] && tipo == "events"
        }
        _ => permite(caller, "subscribe"),
    }
}

pub(crate) const WEB_CHAT: &[&str] = &[
    "botGroups",
    "donnaConversationHistory",
    "donnaConversations",
    "donnaConversationSend",
    "donnaSpaces",
    "hello",
    "history",
    "send",
    "sessions",
    "workspaceContext",
];

pub(crate) const SO_LOCAL: &[&str] = &[
    "bootstrapInstallGrant",
    "chatgptEntrar",
    "chatgptEntrarConcluir",
    "chatgptEstado",
    "chatgptSair",
    "checkHostUpdate",
    "claimSign",
    "cloudStatus",
    "computerBots",
    "computerEnrollCode",
    "computerJob",
    "computerRemove",
    "computerRename",
    "computersList",
    "connectorTarget",
    "donnaApprovalSet",
    "donnaBrowserLocalConsent",
    "donnaBrowserLocalFrame",
    "donnaBrowserLocalState",
    "donnaConectorCapability",
    "donnaConectorDecidir",
    "donnaConectorEditar",
    "donnaConectorRascunho",
    "donnaConectorTentativa",
    "donnaMailManager",
    "donnaMailManagerConnect",
    "donnaMailManagerDisconnect",
    "donnaMailManagerGrant",
    "donnaMailManagerSave",
    "donnaOnboardingDetect",
    "donnaOnboardingSave",
    "donnaOnboardingStatus",
    "donnaOpenModelSet",
    "donnaPersonalBootstrapStatus",
    "donnaRuleDelete",
    "donnaRules",
    "donnaRuleSave",
    "donnaRuleSetEnabled",
    "donnaWhatsappGroupAuto",
    "donnaWhatsappSend",
    "donnaWhatsappSendDraft",
    "donnaWorkplaceAccess",
    "donnaWorkplaceCreate",
    "donnaWorkplaceGuide",
    "donnaWorkplaceInspectCancel",
    "donnaWorkplaceInspectStart",
    "donnaWorkplaceInspectStatus",
    "donnaWorkplaceList",
    "donnaWorkplacePinRunner",
    "donnaWorkplacePrepareConnection",
    "donnaWorkplaceRefix",
    "donnaWorkplaceRemove",
    "donnaWorkplaceRename",
    "hostUpdateStatus",
    "installHostUpdate",
    "instrucaoGeral",
    "instrucaoGeralSalvar",
    "motorChaveRemover",
    "motorChaves",
    "motorChaveSalvar",
    "motorChaveTestar",
    "motorEnderecoSalvar",
    "nuncaFazer",
    "nuncaFazerAdicionar",
    "nuncaFazerEditar",
    "nuncaFazerRemover",
    "nuncaFazerReordenar",
    "openrouterEntrar",
    "openrouterEntrarConcluir",
    "pairing",
    "planoCodigoRemover",
    "planoCodigoSalvar",
    "planoCodigoTestar",
    "planosCodigo",
    "releaseConversation",
    "reportError",
    "requestScreenPermission",
    "setAnalytics",
    "setApproval",
    "setCloud",
    "setHostAutomaticUpdates",
    "setScreenEnabled",
    "setTeamSettings",
    "setVoiceKey",
    "track",
    "unclaim",
];

pub(crate) const PONTE: &[&str] = &[
    "chatCall",
    "computerCall",
    "computersCall",
    "computerTools",
    "connectorCall",
    "donnaConectorFrame",
    "donnaConectorRelease",
    "donnaGuardObserve",
    "donnaGuardRelease",
    "donnaGuardRuntime",
    "manageCall",
    "memoryCall",
    "routineCall",
    "taskCall",
    "teamCall",
];

pub(crate) const DONO_DOS_APARELHOS: &[&str] = &["accessRequests", "decideAccessRequest", "devices", "revokeDevice"];

pub(crate) const TELA: &[&str] = &["screenClose", "screenOffer", "screenPrepare", "screenTakeover"];

pub(crate) const PROPRIO: &[&str] = &["donnaAppLanguageSave"];

pub(crate) const EXECUTOR: &[&str] =
    &["executorAudit", "executorBye", "executorHello", "executorPoll", "executorResult", "executorUpdate"];

pub(crate) const CONTROLE: &[&str] = &[
    "agentAuth",
    "agentAuthenticate",
    "agentModels",
    "agentSetup",
    "answerMemory",
    "approveMemorySuggestion",
    "archiveMemory",
    "backgroundHistory",
    "backgroundSpaceSelections",
    "bootstrapInspect",
    "botConversation",
    "botGroups",
    "botGroupsEdit",
    "browserStreamAck",
    "browserStreamControl",
    "browserStreamInput",
    "browserStreamSubscribe",
    "browserStreamTarget",
    "checkFunctionModel",
    "clearMemory",
    "connectorInfo",
    "connectorRequestFinish",
    "connectorRequestGet",
    "connectors",
    "connectorSignIn",
    "connectorSignInFinish",
    "connectorVerify",
    "controlHealth",
    "createBot",
    "credentialLabelLogin",
    "credentialLogins",
    "credentialRemoveLogin",
    "credentialSaveLogin",
    "credentialSetOnePassword",
    "credentialStatus",
    "credentialUpdateConnector",
    "deleteBot",
    "deleteMemoryFile",
    "deleteRoutine",
    "discardMemorySuggestion",
    "donnaBotEditor",
    "donnaBotEditorSave",
    "donnaBrowserStatus",
    "donnaCeiling",
    "donnaCeilingRelease",
    "donnaCeilingSave",
    "donnaContext",
    "donnaAttention",
    "donnaContextCompact",
    "donnaContextSettings",
    "donnaContextSettingsSave",
    "donnaControlTower",
    "donnaHubDoctor",
    "donnaConversationChoiceAnswer",
    "donnaConversationCreate",
    "donnaConversationHistory",
    "donnaConversationMemory",
    "donnaConversationNewSession",
    "donnaConversationRead",
    "donnaConversationReadFile",
    "donnaConversationReadUpload",
    "donnaConversationRetry",
    "donnaConversations",
    "donnaConversationSend",
    "donnaConversationStartupLookup",
    "donnaConversationStartupOpen",
    "donnaConversationStop",
    "donnaConversationUpdate",
    "donnaConversationUpload",
    "donnaCostReport",
    "donnaDelegationRules",
    "donnaDelegationRulesSave",
    "donnaEngineAccess",
    "donnaEngineAccountCreate",
    "donnaEngineAccountDisconnect",
    "donnaEngineAccountModelVisibility",
    "donnaEngineAccountRemove",
    "donnaEngineAccountRename",
    "donnaEngineAccounts",
    "donnaEngineAccountSetEnabled",
    "donnaEngineAccountUsage",
    "donnaEngineFallback",
    "donnaEngineFallbackAction",
    "donnaEngineFallbackSave",
    "donnaEngineSetupAction",
    "donnaEngineSetupStart",
    "donnaGovernance",
    "donnaGovernanceSave",
    "donnaNativeCatalog",
    "donnaNativeCatalogAdd",
    "donnaOwnerPresentation",
    "donnaProject",
    "donnaProjectCreate",
    "donnaProjectEnsureDefault",
    "donnaProjects",
    "donnaProjectSave",
    "donnaResumoDiarioSave",
    "donnaRoutineCreationPolicy",
    "donnaRoutineCreationPolicySave",
    "donnaRoutineCreationReview",
    "donnaRouting",
    "donnaRoutingCheck",
    "donnaRoutingReceipt",
    "donnaRoutingSave",
    "donnaScreenAccess",
    "donnaSkillsDoutor",
    "donnaSpaceBirth",
    "donnaSpaceCreate",
    "donnaSpaceDelete",
    "donnaSpacePrincipal",
    "donnaSpaceReorder",
    "donnaSpaces",
    "donnaSpaceUpdate",
    "donnaStorageReport",
    "donnaTaskAgenda",
    "donnaTaskComment",
    "donnaTaskComments",
    "donnaTaskMetaSave",
    "donnaTaskProposalDecide",
    "donnaTaskProposals",
    "donnaTasks",
    "donnaTaskSchedule",
    "donnaTaskSessions",
    "donnaTaskStart",
    "donnaTeamRequests",
    "donnaTimes",
    "donnaTimesPreview",
    "donnaTimesSave",
    "donnaTowerView",
    "donnaUsageHistory",
    "donnaUsageLimits",
    "donnaUsageLimitsSave",
    "donnaUsageWaitCancel",
    "donnaUsageWaitResume",
    "donnaWhatsapp",
    "donnaWhatsappBotSave",
    "donnaWhatsappChatErase",
    "donnaWhatsappChatMark",
    "donnaWhatsappChats",
    "donnaWhatsappDisconnect",
    "donnaWhatsappMarkRead",
    "donnaWhatsappMessages",
    "donnaWhatsappPair",
    "donnaWhatsappPairCancel",
    "donnaWhatsappQueue",
    "donnaWhatsappQueueDecide",
    "donnaWhatsappSave",
    "donnaWhatsappWacliRetry",
    "duplicateRoutine",
    "editHandoff",
    "engineGoKeySave",
    "extensaoEmTodosAplicar",
    "extensaoEmTodosPrevia",
    "extensaoNovosBotsDefinir",
    "forgetMemory",
    "functionModels",
    "hello",
    "history",
    "importConnectors",
    "installConnector",
    "installSkill",
    "keepConversation",
    "listDirs",
    "logCall",
    "marketBotContas",
    "marketBotContasDefinir",
    "marketCatalogo",
    "marketConectar",
    "marketConnectors",
    "marketContaDesconectar",
    "marketContaRenomear",
    "marketContas",
    "marketFerramentaDesligar",
    "marketFerramentaLigar",
    "marketFerramentas",
    "marketGoogleApp",
    "marketGoogleAppRemover",
    "marketGoogleAppSalvar",
    "marketAppProprioRemover",
    "marketAppProprioSalvar",
    "marketPersonalizado",
    "marketServico",
    "marketSkills",
    "markRead",
    "memory",
    "memoryFile",
    "memoryLearning",
    "memoryProfile",
    "mexeriquinhaConjuntoApagar",
    "mexeriquinhaConjuntos",
    "mexeriquinhaConjuntoSalvar",
    "mexeriquinhaGeradorCancelar",
    "mexeriquinhaGeradorChave",
    "mexeriquinhaGeradorEstado",
    "mexeriquinhaGeradorJob",
    "mexeriquinhaGerar",
    "mexeriquinhaQuadro",
    "newSession",
    "pinMemory",
    "queuedEdit",
    "queuedMove",
    "queuedRemove",
    "queuedSendNow",
    "react",
    "readFile",
    "readUpload",
    "refreshBackends",
    "registerActivity",
    "registerDevice",
    "releaseMemoryRefusal",
    "removeConnector",
    "removeSkill",
    "reorderBots",
    "respondPermission",
    "restoreFunctionModel",
    "restoreFunctionModels",
    "restoreMemoryFile",
    "retrySubscription",
    "routines",
    "routineSchedule",
    "routinesOverview",
    "routinesPause",
    "routineTeams",
    "routineWebhook",
    "runRoutine",
    "saveBackgroundSpaceSelection",
    "saveFunctionModel",
    "saveMemory",
    "saveMemoryFile",
    "saveMemoryLearning",
    "saveMemoryProfile",
    "saveRoutine",
    "screenStatus",
    "send",
    "sessionAutoCloseConfig",
    "sessionAutoCloseSave",
    "sessionChoiceAnswer",
    "sessionClose",
    "sessionContinue",
    "sessionCreate",
    "sessionDelete",
    "sessionEngineSet",
    "sessionRename",
    "sessions",
    "setAgentEnv",
    "setRoutineEnabled",
    "setRoutinesPaused",
    "skillDetails",
    "skills",
    "stop",
    "subscribe",
    "teamSettings",
    "termClose",
    "termInput",
    "termResize",
    "undoDream",
    "undoMemory",
    "undoTeamChange",
    "unregisterActivity",
    "unregisterDevice",
    "updateBot",
    "upload",
    "usage",
    "voiceModels",
    "voiceSession",
    "voiceSpeak",
    "voiceStatus",
    "voiceTranscribe",
    "workspaceContext",
];
#[cfg(test)]
mod tests;
