//! D368 (rodada 5), tabela única: cada método do `dispatch` (braços `"…" =>` de `api/*.rs` e
//! `fork/api.rs`) tem uma classe. O teste `todo_metodo_do_dispatch_esta_classificado` falha se um
//! método novo não estiver aqui. Quem pode chamar cada método fica em `fork::acesso_rpc`, verificado pelo
//! mesmo coletor; `require_local` e `require_owner` continuam somando nos handlers.
#![cfg_attr(not(test), allow(dead_code))]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Classe {
    /// Só lê.
    Leitura,
    /// Canal D430 privado, intenção fechada. Nunca entra no dispatcher comum.
    VinculoPessoal,
    /// Só o próprio computador do Hub (`LOCAL_ONLY`, `require_local`).
    LocalOnly,
    /// Grava configuração ou recurso do dono que o Berga usa. Do app pareado, só com a autoridade
    /// original reconferida na transação (`pedido::gravar`, `Autoridade::with_db`).
    Dono,
    /// Ação do dono que não muda o Berga (conversas, memória, outros bots, ordem). Mudança no Berga
    /// por esses caminhos é recusada pelos gatilhos.
    Comum,
    /// Ferramentas de bot (MCP): não editam a Donna.
    Bot,
    /// Compartilhamento de tela.
    Tela,
    /// Protocolo dos executores de computador (só aparelho com escopo Executor).
    Executor,
    /// Preferência do próprio chamador (D395: o idioma do app). Cada app grava só a sua, inclusive
    /// um aparelho sem Control; não dá poder sobre a Donna. Ponte, rotinas e pareamento recusados.
    Proprio,
    /// Login legado do motor. Local para Claude/Codex; para os outros, o aparelho com Control do
    /// dono, como exceção D391 à SAAS-04 (DON-914).
    LoginLegado,
}

use Classe::{Bot, Comum, Dono, Executor, Leitura, LocalOnly, LoginLegado, Proprio, Tela};

pub(crate) const METODOS: &[(&str, Classe)] = &[
    ("donnaScreenAccess", Dono),
    ("donnaBrowserStatus", Leitura),
    ("donnaBrowserLocalConsent", LocalOnly),
    ("donnaBrowserLocalFrame", LocalOnly),
    ("donnaBrowserLocalState", LocalOnly),
    ("nuncaFazer", LocalOnly),
    ("nuncaFazerAdicionar", LocalOnly),
    ("nuncaFazerEditar", LocalOnly),
    ("nuncaFazerRemover", LocalOnly),
    ("nuncaFazerReordenar", LocalOnly),
    ("instrucaoGeral", LocalOnly),
    ("instrucaoGeralSalvar", LocalOnly),
    // api/bots.rs
    ("answerMemory", Comum),
    ("archiveMemory", Comum),
    ("botConversation", Leitura),
    ("chatCall", Bot),
    ("clearMemory", Comum),
    ("createBot", Comum),
    ("deleteBot", Comum),
    ("deleteMemoryFile", Comum),
    ("deleteRoutine", Dono),
    ("duplicateRoutine", Dono),
    ("editHandoff", Comum),
    ("forgetMemory", Comum),
    ("history", Leitura),
    ("keepConversation", Comum),
    ("logCall", Comum),
    ("manageCall", Bot),
    ("markRead", Comum),
    ("memory", Leitura),
    ("memoryCall", Bot),
    ("memoryFile", Leitura),
    ("memoryProfile", Leitura),
    ("newSession", Comum),
    ("pinMemory", Comum),
    ("queuedEdit", Comum),
    ("queuedMove", Comum),
    ("queuedRemove", Comum),
    ("queuedSendNow", Comum),
    ("react", Comum),
    ("releaseConversation", LocalOnly),
    ("releaseMemoryRefusal", Comum),
    ("readUpload", Leitura),
    ("reorderBots", Comum),
    ("respondPermission", Comum),
    ("restoreMemoryFile", Comum),
    ("retrySubscription", Comum),
    ("routineCall", Bot),
    ("routineSchedule", Leitura),
    ("routineTeams", Leitura),
    ("routineWebhook", Dono),
    ("routines", Leitura),
    ("routinesOverview", Leitura),
    ("routinesPause", Leitura),
    ("runRoutine", Comum),
    ("saveMemory", Comum),
    ("saveMemoryFile", Comum),
    ("saveMemoryProfile", Comum),
    ("saveRoutine", Dono),
    ("send", Comum),
    ("setRoutineEnabled", Dono),
    ("setRoutinesPaused", Comum),
    ("setTeamSettings", LocalOnly),
    ("stop", Comum),
    ("taskCall", Bot),
    ("teamCall", Bot),
    ("teamSettings", Leitura),
    ("undoDream", Comum),
    ("undoMemory", Comum),
    ("undoTeamChange", Comum),
    ("updateBot", Dono),
    ("upload", Comum),
    // api/host.rs
    ("agentAuth", Leitura),
    ("agentAuthenticate", LoginLegado),
    ("agentModels", Leitura),
    ("agentSetup", LoginLegado),
    ("checkHostUpdate", LocalOnly),
    ("computerTools", LocalOnly),
    ("hello", Leitura),
    ("workspaceContext", Leitura),
    ("hostUpdateStatus", LocalOnly),
    ("installHostUpdate", LocalOnly),
    ("listDirs", Leitura),
    ("refreshBackends", Leitura),
    ("reportError", LocalOnly),
    ("requestScreenPermission", LocalOnly),
    ("screenClose", Tela),
    ("screenOffer", Tela),
    ("screenPrepare", Tela),
    ("screenStatus", Leitura),
    ("screenTakeover", Tela),
    // Quadros do navegador (fatia 2): só aparelho dono com Control+Screen, conferido em `AutoridadeCanal`
    // na inscrição e a cada quadro/entrada/controle; nunca bot ou ponte. Não passam por `permit`.
    ("browserStreamAck", Tela),
    ("browserStreamControl", Tela),
    ("browserStreamInput", Tela),
    ("browserStreamSubscribe", Tela),
    ("browserStreamTarget", Tela),
    ("setAgentEnv", LocalOnly),
    // Slot Go exclusivo; exige o vínculo original engine.accounts.
    ("engineGoKeySave", Dono),
    // Planos de código (OpenCode como motor): `LOCAL_ONLY` e `require_local`; nunca devolvem a chave.
    ("planosCodigo", LocalOnly),
    ("planoCodigoTestar", LocalOnly),
    ("planoCodigoSalvar", LocalOnly),
    ("planoCodigoRemover", LocalOnly),
    ("setAnalytics", LocalOnly),
    ("setHostAutomaticUpdates", LocalOnly),
    ("setScreenEnabled", LocalOnly),
    ("setVoiceKey", LocalOnly),
    // Chaves de API do Motor Mexerica (API direta): `LOCAL_ONLY` e `require_local`; nunca devolvem a chave.
    ("motorChaves", LocalOnly),
    ("motorChaveTestar", LocalOnly),
    ("motorChaveSalvar", LocalOnly),
    ("motorChaveRemover", LocalOnly),
    ("motorEnderecoSalvar", LocalOnly),
    // Entrar com OpenRouter (OAuth PKCE): `LOCAL_ONLY` e `require_local`; a chave nunca volta ao app.
    ("openrouterEntrar", LocalOnly),
    ("openrouterEntrarConcluir", LocalOnly),
    // Entrar com ChatGPT (OAuth PKCE): `LOCAL_ONLY` e `require_local`; os tokens nunca voltam ao app.
    ("chatgptEntrar", LocalOnly),
    ("chatgptEntrarConcluir", LocalOnly),
    ("chatgptEstado", LocalOnly),
    ("chatgptSair", LocalOnly),
    // gerador Mexeriquinhas 2b: todo o grupo exige Caller::Local (host/src/fork_avatars/gerador).
    ("mexeriquinhaGeradorEstado", LocalOnly),
    ("mexeriquinhaGeradorChave", LocalOnly),
    ("mexeriquinhaGerar", LocalOnly),
    ("mexeriquinhaGeradorJob", LocalOnly),
    ("mexeriquinhaGeradorCancelar", LocalOnly),
    ("mexeriquinhaConjuntoSalvar", LocalOnly),
    ("mexeriquinhaConjuntos", LocalOnly),
    ("mexeriquinhaQuadro", LocalOnly),
    ("mexeriquinhaConjuntoApagar", LocalOnly),
    ("termClose", LoginLegado),
    ("termInput", LoginLegado),
    ("termResize", LoginLegado),
    ("track", LocalOnly),
    ("usage", Leitura),
    ("voiceModels", Leitura),
    ("voiceSession", Comum),
    ("voiceSpeak", Comum),
    ("voiceStatus", Leitura),
    ("voiceTranscribe", Comum),
    // api/marketplace.rs
    ("connectorCall", Bot),
    ("connectorInfo", Leitura),
    ("connectorRequestFinish", Dono),
    ("connectorRequestGet", Leitura),
    ("connectorSignInFinish", Dono),
    ("connectorTarget", LocalOnly),
    ("connectorVerify", Leitura),
    ("connectors", Leitura),
    ("credentialLabelLogin", Dono),
    ("credentialLogins", Leitura),
    ("credentialRemoveLogin", Dono),
    ("credentialSaveLogin", Dono),
    ("credentialSetOnePassword", Dono),
    ("credentialStatus", Leitura),
    ("credentialUpdateConnector", Dono),
    ("importConnectors", Dono),
    ("installConnector", Dono),
    ("marketBotContas", Leitura),
    ("marketBotContasDefinir", Dono),
    ("marketCatalogo", Leitura),
    ("marketGoogleApp", Dono),
    ("marketGoogleAppRemover", Dono),
    ("marketGoogleAppSalvar", Dono),
    ("marketAppProprioRemover", Dono),
    ("marketAppProprioSalvar", Dono),
    ("marketConectar", Dono),
    ("marketConnectors", Leitura),
    ("marketContaDesconectar", Dono),
    ("marketContaRenomear", Dono),
    ("marketFerramentaDesligar", Dono),
    ("marketFerramentaLigar", Dono),
    ("marketFerramentas", Leitura),
    ("marketContas", Leitura),
    ("marketPersonalizado", Dono),
    ("marketServico", Leitura),
    ("removeConnector", Dono),
    // api/remote.rs
    ("accessRequests", LocalOnly),
    ("bootstrapInspect", Leitura),
    ("bootstrapInstallGrant", LocalOnly),
    ("claimSign", LocalOnly),
    ("cloudStatus", LocalOnly),
    ("controlHealth", Leitura),
    ("decideAccessRequest", LocalOnly),
    ("devices", LocalOnly),
    ("pairing", LocalOnly),
    ("registerActivity", Comum),
    ("registerDevice", Comum),
    ("revokeDevice", LocalOnly),
    ("setApproval", LocalOnly),
    ("setCloud", LocalOnly),
    ("unclaim", LocalOnly),
    ("unregisterActivity", Comum),
    ("unregisterDevice", Comum),
    // fork/api.rs
    ("backgroundHistory", Leitura),
    ("backgroundSpaceSelections", Leitura),
    ("checkFunctionModel", Leitura),
    ("donnaBotEditor", Leitura),
    ("donnaBotEditorSave", Dono),
    ("donnaRoutineCreationPolicy", Leitura),
    ("donnaRoutineCreationPolicySave", Dono),
    ("donnaRoutineCreationReview", Dono),
    ("donnaEngineAccounts", Leitura),
    ("donnaSkillsDoutor", Leitura),
    ("donnaGuardObserve", LocalOnly),
    ("donnaGuardRelease", LocalOnly),
    ("donnaGuardRuntime", LocalOnly),
    ("donnaProjects", Leitura),
    ("donnaTasks", Leitura),
    ("donnaTaskMetaSave", Dono),
    ("donnaTaskComments", Leitura),
    ("donnaTaskComment", Dono),
    ("donnaProjectEnsureDefault", Dono),
    ("donnaProject", Leitura),
    ("donnaProjectCreate", Dono),
    ("donnaProjectSave", Dono),
    ("donnaTaskStart", Dono),
    ("donnaTaskSchedule", Dono),
    ("donnaTaskAgenda", Leitura),
    ("donnaTaskSessions", Leitura),
    ("donnaTaskProposals", Leitura),
    ("donnaTaskProposalDecide", Dono),
    ("donnaTeamRequests", Leitura),
    ("donnaTimes", Leitura),
    ("donnaTimesPreview", Leitura),
    ("donnaTimesSave", Comum),
    ("functionModels", Leitura),
    // D389 (R5/SEG-491-01): modo de aprender e sugestões são do dono (Local ou app pareado com Control), com a
    // autoridade original conferida na Tx que grava; ponte e bot recusados. O modelo da memória segue Local.
    ("approveMemorySuggestion", Comum),
    ("discardMemorySuggestion", Comum),
    ("memoryLearning", Comum),
    ("saveMemoryLearning", Comum),
    ("restoreFunctionModel", Comum),
    ("restoreFunctionModels", Comum),
    ("saveBackgroundSpaceSelection", Comum),
    ("saveFunctionModel", Comum),
    ("sessionAutoCloseConfig", Leitura),
    ("sessionAutoCloseSave", Dono),
    // G1b sessões: resposta do dono ao cartão de escolha de uma extra (autoridade original na Tx, sem engine.accounts).
    ("sessionChoiceAnswer", Comum),
    ("sessionClose", Comum),
    ("sessionDelete", Comum),
    ("sessionContinue", Comum),
    ("sessionCreate", Comum),
    // D422: no Berga, a troca de modelo da conversa é do dono pelo app (guarda SAAS-04 original na mesma Tx).
    ("sessionEngineSet", Dono),
    ("sessionRename", Comum),
    ("sessions", Leitura),
    // Módulos encaminhados (handles/METHODS/`method == …`): Spaces, conversas, skills, WhatsApp, Agenda,
    // contas de IA, roteamento, delegação, regras, conectores, Workplace, computadores, limites de uso.
    ("botGroups", Leitura),
    ("computersList", Leitura),
    ("donnaContext", Leitura),
    ("donnaControlTower", Leitura),
    ("donnaAttention", Leitura),
    ("donnaTowerView", Leitura),
    ("donnaGovernance", Leitura),
    ("donnaGovernanceSave", Dono),
    ("donnaHubDoctor", Leitura),
    ("donnaConversationHistory", Leitura),
    ("donnaConversationRead", Leitura),
    ("donnaConversationReadFile", Leitura),
    ("donnaConversationReadUpload", Leitura),
    ("donnaConversationStartupLookup", Leitura),
    ("donnaConversations", Leitura),
    ("donnaDelegationRules", Leitura),
    ("donnaEngineAccess", Leitura),
    ("donnaEngineAccountUsage", Leitura),
    ("donnaNativeCatalog", Leitura),
    ("donnaOwnerPresentation", Leitura),
    ("donnaRouting", Leitura),
    ("donnaRoutingCheck", Leitura),
    ("donnaSpaces", Leitura),
    ("donnaAppLanguageSave", Proprio),
    ("donnaCostReport", Leitura),
    // MX-260: só lê, mas pelo ponto de gravação do dono (autoridade reconferida na Tx).
    ("donnaUsageHistory", Dono),
    ("donnaCeiling", Leitura),
    ("donnaCeilingRelease", Dono),
    ("donnaCeilingSave", Dono),
    ("donnaStorageReport", Leitura),
    ("donnaUsageLimits", Leitura),
    ("donnaWhatsapp", Leitura),
    ("donnaWhatsappChats", Leitura),
    ("donnaWhatsappMessages", Leitura),
    ("donnaWorkplaceGuide", Leitura),
    ("donnaWorkplaceInspectStatus", Leitura),
    ("donnaWorkplaceList", Leitura),
    ("marketSkills", Leitura),
    ("readFile", Leitura),
    ("skillDetails", Leitura),
    ("skills", Leitura),
    ("extensaoEmTodosPrevia", Leitura),
    ("computerBots", LocalOnly),
    ("computerCall", LocalOnly),
    ("computerEnrollCode", LocalOnly),
    ("computerJob", LocalOnly),
    ("computerRemove", LocalOnly),
    ("computerRename", LocalOnly),
    ("computersCall", LocalOnly),
    ("donnaApprovalSet", LocalOnly),
    ("donnaConectorCapability", LocalOnly),
    ("donnaConectorDecidir", LocalOnly),
    ("donnaConectorEditar", LocalOnly),
    ("donnaConectorFrame", LocalOnly),
    ("donnaConectorRelease", LocalOnly),
    ("donnaConectorTentativa", LocalOnly),
    ("donnaConectorRascunho", LocalOnly),
    // PR3 PL1: status do vínculo pessoal (`personal_bootstrap::status`, LOCAL_ONLY em devices.rs).
    ("donnaPersonalBootstrapStatus", LocalOnly),
    ("donnaOnboardingDetect", LocalOnly),
    ("donnaOnboardingSave", LocalOnly),
    ("donnaOnboardingStatus", LocalOnly),
    ("donnaOpenModelSet", LocalOnly),
    ("donnaRuleDelete", LocalOnly),
    ("donnaRuleSave", LocalOnly),
    ("donnaRuleSetEnabled", LocalOnly),
    ("donnaRules", LocalOnly),
    ("donnaMailManager", LocalOnly),
    ("donnaMailManagerSave", LocalOnly),
    ("donnaMailManagerConnect", LocalOnly),
    ("donnaMailManagerDisconnect", LocalOnly),
    ("donnaMailManagerGrant", LocalOnly),
    ("executorAudit", Executor),
    ("executorBye", Executor),
    ("executorHello", Executor),
    ("executorPoll", Executor),
    ("executorResult", Executor),
    ("executorUpdate", Executor),
    ("connectorSignIn", Dono),
    ("donnaDelegationRulesSave", Dono),
    ("donnaEngineAccountCreate", Dono),
    ("donnaEngineAccountDisconnect", Dono),
    ("donnaEngineAccountRemove", Dono),
    ("donnaEngineAccountRename", Dono),
    // MEX-1391/F6-B: a ordem de reserva e as ações do fallback são do dono; a leitura é a de contas de IA.
    ("donnaEngineFallback", Leitura),
    ("donnaEngineFallbackSave", Dono),
    ("donnaEngineFallbackAction", Dono),
    ("donnaEngineAccountModelVisibility", Dono),
    ("donnaEngineAccountSetEnabled", Dono),
    ("donnaEngineSetupAction", Dono),
    ("donnaEngineSetupStart", Dono),
    ("donnaResumoDiarioSave", Dono),
    ("donnaRoutingSave", Dono),
    ("donnaUsageLimitsSave", Dono),
    ("donnaUsageWaitCancel", Dono),
    ("donnaUsageWaitResume", Dono),
    ("donnaWhatsappBotSave", Dono),
    ("donnaWhatsappDisconnect", Dono),
    ("donnaWhatsappPair", Dono),
    ("donnaWhatsappSave", Dono),
    ("installSkill", Dono),
    ("removeSkill", Dono),
    // MX-264: lote atômico de skill/conector em todos os bots, autoridade original na Tx.
    ("extensaoEmTodosAplicar", Dono),
    // MX-264/C: padrão "novos bots já vêm com…" — só skills; conector é recusado no método.
    ("extensaoNovosBotsDefinir", Dono),
    ("botGroupsEdit", Comum),
    ("donnaContextCompact", Comum),
    ("donnaContextSettings", Leitura),
    ("donnaContextSettingsSave", Dono),
    ("donnaConversationChoiceAnswer", Comum),
    ("donnaConversationCreate", Comum),
    ("donnaConversationMemory", Comum),
    ("donnaConversationNewSession", Comum),
    ("donnaConversationRetry", Comum),
    ("donnaConversationSend", Comum),
    ("donnaConversationStartupOpen", Comum),
    ("donnaConversationStop", Comum),
    ("donnaConversationUpdate", Comum),
    ("donnaConversationUpload", Comum),
    ("donnaNativeCatalogAdd", Comum),
    ("donnaRoutingReceipt", Comum),
    ("donnaSpaceCreate", Comum),
    ("donnaSpaceBirth", Comum),
    ("donnaSpacePrincipal", Dono),
    ("donnaSpaceDelete", Dono),
    ("donnaSpaceReorder", Comum),
    ("donnaSpaceUpdate", Comum),
    ("donnaWhatsappChatMark", Dono),
    ("donnaWhatsappGroupAuto", LocalOnly),
    ("donnaWhatsappQueue", Leitura),
    ("donnaWhatsappQueueDecide", Dono),
    ("donnaWhatsappSendDraft", LocalOnly),
    ("donnaWhatsappSend", LocalOnly),
    ("donnaWhatsappMarkRead", Comum),
    ("donnaWhatsappPairCancel", Comum),
    ("donnaWhatsappWacliRetry", Comum),
    ("donnaWhatsappChatErase", Comum),
    ("donnaWorkplaceAccess", Comum),
    ("donnaWorkplaceCreate", Comum),
    ("donnaWorkplaceInspectCancel", Comum),
    ("donnaWorkplaceInspectStart", Comum),
    ("donnaWorkplacePinRunner", Comum),
    ("donnaWorkplacePrepareConnection", Comum),
    ("donnaWorkplaceRefix", Comum),
    ("donnaWorkplaceRemove", Comum),
    ("donnaWorkplaceRename", Comum),
];

/// Protocolos que não são a API do Hub e também comparam `method` (MCP JSON-RPC das pontes, o helper
/// de tela e o ACP interno do Motor Mexerica). O coletor os ignora, com motivo; nenhum é método do `dispatch`.
pub(crate) const FORA_DA_API: &[(&str, &str)] = &[
    ("guard_hub/mcp/", "MCP JSON-RPC das pontes Guard"),
    ("fork/sessoes/gateway.rs", "MCP JSON-RPC das sessões extras"),
    ("fork/spaces/connector_context.rs", "MCP JSON-RPC do contexto de conector"),
    ("screen/helper.rs", "protocolo do helper nativo de tela"),
    ("market/verify.rs", "MCP JSON-RPC da verificação de conector"),
    ("fork/conectores/proxy.rs", "MCP JSON-RPC do proxy de conectores"),
    ("fork/motor_mexerica/link.rs", "ACP interno entre o bot e o Motor Mexerica, dentro do Hub; o app não o alcança"),
];

/// Inventário do transporte próprio B; classificação não autentica nem concede direitos.
/// Estes nomes NÃO entram em METODOS/dispatch nem nas ferramentas do bot.
pub(crate) const METODOS_VINCULO_PESSOAL: &[(&str, Classe)] = &[
    ("session.open", Classe::VinculoPessoal),
    ("link.status", Classe::VinculoPessoal),
    ("session.close", Classe::VinculoPessoal),
];

/// Coletor dos métodos do `dispatch` no código-fonte, para os testes-porta. Formas reconhecidas: braços
/// de `match method { "x" => … }` (no nível do `match`), `matches!(method, "x" | …)`, `method == "x"`,
/// `method == CONST`, `method.eq(…)`, `if let "x" = method`, `CONST.contains(&method)`, constantes usadas
/// por `fn handles`/`fn methods` e as mesmas formas sobre apelidos (`let m = method;`). É inventário por
/// leitura do código, não trava de autorização; o registro único de métodos fica na DON-915.
#[cfg(test)]
#[allow(clippy::expect_used, reason = "Só testes: expressões regulares fixas.")]
pub(crate) mod coletor {
    use regex::Regex;
    use std::collections::{BTreeMap, BTreeSet};

    /// `let nome = method;` (e `.as_str()`, `&`, `.to_owned()`…): o apelido roteia igual a `method`.
    const CONV: &str = r"(?:\s*\.\s*(?:as_str|as_ref|to_owned|to_string|trim|borrow)\s*\(\s*\))*";

    fn nome(n: &str) -> bool {
        let mut chars = n.chars();
        chars.next().is_some_and(|c| c.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_alphanumeric())
    }

    fn literais(texto: &str) -> Vec<String> {
        let aspas = Regex::new(r#""([^"\\]*)""#).expect("regex");
        aspas.captures_iter(texto).map(|c| c[1].to_owned()).filter(|n| nome(n)).collect()
    }

    /// Só o código de produção: um bloco `#[cfg(test)] mod tests { … }` no fim do arquivo sai, e as
    /// linhas de comentário também (exemplos em documentação não são métodos).
    pub(crate) fn producao(codigo: &str) -> String {
        let codigo = codigo.find("#[cfg(test)]\nmod tests {").map_or(codigo, |i| &codigo[..i]);
        codigo.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
    }

    /// O texto no nível 1 de um bloco que começa logo depois de `{`.
    fn nivel_um(bloco: &str) -> String {
        let mut nivel = 1;
        let mut out = String::new();
        for c in bloco.chars() {
            match c {
                '{' => nivel += 1,
                '}' => {
                    nivel -= 1;
                    if nivel == 0 {
                        break;
                    }
                }
                _ if nivel == 1 => out.push(c),
                _ => {}
            }
        }
        out
    }

    /// Método → arquivos onde aparece.
    pub(crate) fn coletar(fontes: &[(String, String)]) -> BTreeMap<String, BTreeSet<String>> {
        let arr = Regex::new(
            r"(?s)const\s+([A-Z_][A-Z0-9_]*)\s*:\s*&?(?:\[\s*&(?:'static\s+)?str\s*(?:;\s*\d+\s*)?\]|&\[&str\])\s*=\s*&?\[(.*?)\]\s*;",
        )
        .expect("regex");
        let um =
            Regex::new(r#"const\s+([A-Z_][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"]*)"\s*;"#).expect("regex");
        let mut listas: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (_, codigo) in fontes {
            let codigo = producao(codigo);
            let codigo = codigo.as_str();
            for c in arr.captures_iter(codigo) {
                listas.entry(c[1].to_owned()).or_default().extend(literais(&c[2]));
            }
            for c in um.captures_iter(codigo) {
                if nome(&c[2]) {
                    listas.entry(c[1].to_owned()).or_default().insert(c[2].to_owned());
                }
            }
        }
        let constantes = |texto: &str| -> BTreeSet<String> {
            let ident = Regex::new(r"\b([A-Z][A-Z0-9_]+)\b").expect("regex");
            ident.captures_iter(texto).flat_map(|c| listas.get(&c[1]).cloned().unwrap_or_default()).collect()
        };
        let braco = Regex::new(r#"((?:"[A-Za-z0-9]+"\s*\|\s*)*"[A-Za-z0-9]+")\s*(?:if [^=]*)?=>"#).expect("regex");
        let braco_const = Regex::new(r"\b([A-Z][A-Z0-9_]+)\s*(?:\|[^=]*)?=>").expect("regex");
        let filtro =
            Regex::new(r"(?s)fn\s+(?:handles|methods)\s*\(\s*method\s*:\s*&str\s*\)\s*->\s*bool\s*\{(.*?)\n\}")
                .expect("regex");
        let contem_m = Regex::new(r"(?:[a-z_]+::)*([A-Z][A-Z0-9_]+)\s*\.\s*contains\s*\(\s*&\s*m\b").expect("regex");
        let mut metodos: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (arquivo, codigo) in fontes {
            let codigo = producao(codigo);
            let codigo = codigo.as_str();
            let mut achados: BTreeSet<String> =
                contem_m.captures_iter(codigo).flat_map(|c| listas.get(&c[1]).cloned().unwrap_or_default()).collect();
            let mut apelidos = BTreeSet::from(["method".to_owned()]);
            loop {
                let antes = apelidos.len();
                for a in apelidos.clone() {
                    let alias = Regex::new(&format!(
                        r"\blet\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::\s*&?\s*(?:'static\s+)?(?:str|String)\s*)?=\s*&?\s*{a}\b{CONV}\s*;"
                    ))
                    .expect("regex");
                    apelidos.extend(alias.captures_iter(codigo).map(|c| c[1].to_owned()));
                }
                if apelidos.len() == antes {
                    break;
                }
            }
            for a in &apelidos {
                let a = &format!(r"{a}\b{CONV}");
                let re = |padrao: String| Regex::new(&padrao).expect("regex");
                let casa = re(format!(r"(?s)matches!\(\s*&?\s*{a}\s*,(.*?)\)"));
                let igual = re(format!(
                    r#"\b{a}\s*==\s*"([^"]*)"|"([^"]*)"\s*==\s*&?\s*{a}|\b{a}\s*\.\s*eq\s*\(\s*&?\s*"([^"]*)"\s*\)|"([^"]*)"\s*\.\s*eq\s*\(\s*&?\s*{a}\s*\)"#
                ));
                let igual_const = re(format!(
                    r"\b{a}\s*(?:==|\.\s*eq\s*\(\s*&?)\s*(?:[a-z_]+::)*([A-Z][A-Z0-9_]+)\b|\b([A-Z][A-Z0-9_]+)\s*(?:==|\.\s*eq\s*\(\s*&?)\s*&?\s*{a}"
                ));
                let contem = re(format!(r"(?:[a-z_]+::)*([A-Z][A-Z0-9_]+)\s*\.\s*contains\s*\(\s*&?\s*{a}"));
                let partida = re(format!(r"\bmatch\s+&?\s*{a}\s*\{{"));
                let se_let = re(format!(r"(?s)\b(?:if|while)\s+let\s+([^=]*?)\s*=\s*&?\s*{a}\s*(?:\{{|&&|else)"));
                for c in casa.captures_iter(codigo) {
                    achados.extend(literais(&c[1]));
                    achados.extend(constantes(&c[1]));
                }
                for c in igual.captures_iter(codigo) {
                    achados.extend((1..=4).find_map(|i| c.get(i)).map(|m| m.as_str().to_owned()));
                }
                for c in igual_const.captures_iter(codigo) {
                    let nome = c.get(1).or_else(|| c.get(2)).map_or("", |m| m.as_str());
                    achados.extend(listas.get(nome).cloned().unwrap_or_default());
                }
                for c in contem.captures_iter(codigo) {
                    achados.extend(listas.get(&c[1]).cloned().unwrap_or_default());
                }
                for c in se_let.captures_iter(codigo) {
                    achados.extend(literais(&c[1]));
                    achados.extend(constantes(&c[1]));
                }
                for m in partida.find_iter(codigo) {
                    let topo = nivel_um(&codigo[m.end()..]);
                    for c in braco.captures_iter(&topo) {
                        achados.extend(literais(&c[1]));
                    }
                    for c in braco_const.captures_iter(&topo) {
                        achados.extend(listas.get(&c[1]).cloned().unwrap_or_default());
                    }
                }
            }
            for c in filtro.captures_iter(codigo) {
                achados.extend(constantes(&c[1]));
            }
            for n in achados.into_iter().filter(|n| nome(n)) {
                metodos.entry(n).or_default().insert(arquivo.clone());
            }
        }
        metodos
    }

    /// Todo o código de produção do Host (sem arquivos de teste), para o coletor de métodos.
    pub(crate) fn fontes_do_host() -> Vec<(String, String)> {
        fn andar(dir: &std::path::Path, raiz: &std::path::Path, out: &mut Vec<(String, String)>) {
            for entrada in std::fs::read_dir(dir).expect("fonte do host") {
                let caminho = entrada.expect("fonte do host").path();
                if caminho.is_dir() {
                    andar(&caminho, raiz, out);
                } else if caminho.extension().is_some_and(|e| e == "rs") {
                    let relativo =
                        caminho.strip_prefix(raiz).expect("fonte do host").to_string_lossy().replace('\\', "/");
                    let nome = caminho.file_name().expect("fonte do host").to_string_lossy();
                    if nome.contains("test") || nome.contains("provas") {
                        continue;
                    }
                    if super::FORA_DA_API.iter().any(|(prefixo, _)| relativo.starts_with(prefixo)) {
                        continue;
                    }
                    out.push((relativo, std::fs::read_to_string(&caminho).expect("fonte do host")));
                }
            }
        }
        let raiz = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        andar(&raiz, &raiz, &mut out);
        out
    }

    /// Erro com os métodos sem classe e as classes sem método (sobras).
    pub(crate) fn verificar(
        tabela: &[(&str, super::Classe)],
        metodos: &BTreeMap<String, BTreeSet<String>>,
    ) -> Result<(), String> {
        let classificados: BTreeSet<&str> = tabela.iter().map(|(n, _)| *n).collect();
        let faltam: Vec<String> = metodos
            .iter()
            .filter(|(n, _)| !classificados.contains(n.as_str()))
            .map(|(n, arquivos)| format!("{n} ({})", arquivos.iter().cloned().collect::<Vec<_>>().join(", ")))
            .collect();
        let sobram: Vec<&str> = classificados.iter().copied().filter(|n| !metodos.contains_key(*n)).collect();
        if classificados.len() != tabela.len() {
            return Err("método repetido na tabela".to_owned());
        }
        if faltam.is_empty() && sobram.is_empty() {
            Ok(())
        } else {
            Err(format!("sem classe em donna_fixa::metodos: {faltam:?}; sem método no código: {sobram:?}"))
        }
    }
}
