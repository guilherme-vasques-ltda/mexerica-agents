//! Regras próprias da Mexerica, com ganchos curtos no código herdado.
pub(crate) mod anthropic_automation;
pub(crate) mod antigravity;
pub mod autoridade;
pub mod bootstrap;
pub(crate) mod bridge_http;
pub(crate) mod cadeias;
#[cfg(unix)]
pub(crate) mod desktop_socket;
pub(crate) mod open_access;
pub(crate) mod open_canal;
pub(crate) mod open_chat;
pub(crate) mod open_chat_anexos;
pub(crate) mod open_chat_fila;
#[cfg(unix)]
pub(crate) mod open_codex;
pub(crate) mod open_command_settings;
pub(crate) mod open_manuais;
mod open_mcp;
pub(crate) mod open_modelo;
pub(crate) mod open_rotinas;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod open_terminal;
pub(crate) mod open_tool_actions;
pub(crate) mod open_tool_delegation;
mod open_tools;
pub(crate) mod open_turn;
pub(crate) mod voz_gpt_live;
pub(crate) mod web_local;
pub(crate) mod webhook;
pub(crate) mod webhook_chain;

use crate::LockExt;

pub(crate) mod acesso_rpc;
pub(crate) mod activity_code;
pub(crate) mod agent_skills;
pub mod api;
pub(crate) mod armazenamento;
#[allow(dead_code, reason = "Fonte 19.1a publicada antes dos consumidores; sem runtime novo.")]
pub mod atividades;
pub(crate) mod background_adapter;
pub(crate) mod background_anthropic;
pub(crate) mod background_completion;
pub(crate) mod background_executor;
pub(crate) mod background_history;
pub(crate) mod background_isolation;
pub(crate) mod background_memory_auto;
pub(crate) mod background_notice;
pub(crate) mod background_policy;
pub(crate) mod background_selection;
pub(crate) mod background_space_selection;
#[cfg(target_os = "linux")]
pub(crate) mod backup_atualizacao;
pub(crate) mod bot_admission;
pub mod bot_editor;
pub(crate) mod bot_skills;
pub(crate) mod browser;
#[cfg(test)]
mod chatgpt_responses;
pub(crate) mod claude_ponte;
pub(crate) mod claude_tools;
pub(crate) mod codex_usage;
pub(crate) mod compaction_prompt;
pub(crate) mod composio_retirada;
pub(crate) mod conectores;
pub(crate) mod conexao_motores;
pub(crate) mod contexto;
pub(crate) mod control_tower;
#[cfg(target_os = "linux")]
pub(crate) mod coordenador_atualizacao;
pub(crate) mod custo;
pub(crate) mod data_home;
#[allow(dead_code, reason = "MX-387 fatia A: núcleo publicado antes do consumidor (modo Auto); sem runtime novo.")]
pub(crate) mod decisor;
pub(crate) mod delegacao;
pub(crate) mod doctor;
pub(crate) mod donna_fixa;
pub mod engine_accounts;
pub mod engine_setup;
pub(crate) mod enrollment;
#[allow(dead_code, reason = "Referências 19.1a; resolvedor de autoridade pertence à próxima fonte.")]
pub mod escopo;
pub(crate) mod extensoes_em_todos;
pub(crate) mod fallback_contas;
pub(crate) mod funcao;
pub(crate) mod genui;
pub(crate) mod governanca;
pub(crate) mod grupo_entrega;
#[cfg(target_os = "linux")]
pub(crate) mod guarda_atualizacao;
pub(crate) mod harness_guidance;
pub(crate) mod heartbeat;
pub(crate) mod historico;
pub(crate) mod idioma_app;
#[cfg(target_os = "linux")]
pub(crate) mod instalacao_gerida;
pub(crate) mod instalacao_script;
pub(crate) mod instrucao_geral;
pub(crate) mod lider_canal;
#[allow(dead_code, reason = "Base de permissões publicada antes do painel e do transporte; sem runtime novo.")]
pub(crate) mod mail_manager;
pub(crate) mod mailbox;
pub(crate) mod memory;
pub(crate) mod memory_auto;
pub(crate) mod memory_learning;
pub(crate) mod mentions;
#[cfg(target_os = "linux")]
#[path = "../motor_copia/mod.rs"]
pub(crate) mod motor_copia;
pub(crate) mod native_catalog;
pub mod native_scheduling;
pub(crate) mod nunca_fazer;
pub(crate) mod onboarding;
pub(crate) mod onde_roda;
pub(crate) mod owner_question;
pub(crate) mod pairing_addresses;
pub(crate) mod pasta;
pub(crate) mod pedidos_time;
pub(crate) mod permission_card;
pub(crate) mod personal_bootstrap;
pub(crate) mod personal_link;
pub(crate) mod planos_codigo;
#[allow(dead_code, reason = "F1b/D247: seam privado antes do broker; sem produtor de autoridade.")]
pub(crate) mod plugins;
#[cfg(unix)]
pub(crate) mod private_fs;
pub(crate) mod projetos;
pub(crate) mod propostas;
pub(crate) mod prova_health;
#[cfg(target_os = "linux")]
pub(crate) mod publicacao_atualizacao;
pub(crate) mod push_textos;
pub(crate) mod resumo_diario;
pub(crate) mod retornos;
#[cfg(unix)]
#[allow(dead_code, reason = "MX-175: contrato de sandbox preparado; sem consumidor de execução no Hub.")]
pub(crate) mod sandbox;
pub(crate) mod secret_refs;
pub(crate) mod sessao_clerk;
pub(crate) mod sessoes;
pub(crate) mod skill_catalog;
pub(crate) mod skill_selection;
pub(crate) mod skills_doutor;
#[cfg(test)]
mod skills_doutor_tests;
pub mod spaces;
#[cfg(target_os = "linux")]
#[path = "../atualizacao/status.rs"]
pub(crate) mod status_atualizacao;
pub(crate) mod tarefas;
pub(crate) mod tarefas_runtime;
pub(crate) mod tela_por_computador;
pub(crate) mod terminal_login;
#[cfg(all(test, unix))]
pub(crate) mod test_support;
pub(crate) mod teto;
pub(crate) mod times;
pub(crate) mod torre_visoes;
pub(crate) mod usage_limits;
pub(crate) mod uso_historico;
pub(crate) mod whatsapp;
pub(crate) mod workspace;

pub mod modelos_funcao;
pub(crate) mod rota_anthropic;
pub(crate) mod roteamento;
pub(crate) mod rotina_resultado;
pub(crate) mod routine_creation;
pub mod rules;
pub(crate) mod runtime_audiencia;

/// Migra os dados preparatórios do fork; nunca configura motores nem aprova ações.
pub(crate) fn migrate(store: &crate::store::Store) -> anyhow::Result<()> {
    nunca_fazer::migrate(&store.db.locked())?;
    workspace::migrate(&store.db.locked())?;
    mailbox::migrate(&store.db.locked())?;
    sessao_clerk::migrate(&store.db.locked())?;
    instrucao_geral::migrate(&store.db.locked())?;
    custo::migrate(&store.db.locked())?;
    autoridade::concessao::migrate(&store.db.locked())?;
    conectores::oauth_binding::migrate(&store.db.locked())?;
    atividades::escopo_migracao::migrate(store)?;
    rules::migrate::migrate(&store.db.locked())?;
    genui::storage::migrate(&store.db.locked())?;
    genui::chart_store::migrate(&store.db.locked())?;
    onde_roda::store::migrate(&store.db.locked())?;
    times::migrate(&store.db.locked())?;
    projetos::migrate(&store.db.locked())?;
    governanca::migrate(&store.db.locked())?;
    tarefas::migrate(&store.db.locked())?;
    tarefas_runtime::migrate(&store.db.locked())?;
    propostas::migrate(&store.db.locked())?;
    delegacao::migrate(&store.db.locked())?;
    donna_fixa::migrate(&store.db.locked())?;
    native_catalog::migrate(&store.db.locked())?;
    plugins::migrate(&store.db.locked())?;
    whatsapp::migrate(&store.db.locked())?;
    mail_manager::migrate(&store.db.locked())?;
    cadeias::store::migrate(&store.db.locked())?;
    retornos::migrate(&store.db.locked())?;
    delegacao::pausa::migrate(&store.db.locked())?;
    resumo_diario::migrate(&store.db.locked())?;
    roteamento::receipt::migrate(&store.db.locked())?;
    teto::migrate(&store.db.locked())?;
    rota_anthropic::migrate(&store.db.locked())?;
    uso_historico::migrate(&store.db.locked())?;
    modelos_funcao::config::initialize(store)
}

/// Invocado dentro da transação de exclusão, inclusive nos caminhos de gestão.
pub fn on_bot_deleted(c: &rusqlite::Connection, id: &str) -> anyhow::Result<()> {
    open_command_settings::on_bot_deleted(c, id)?;
    #[cfg(target_os = "macos")]
    crate::screen::fork_mac_consent::set_on(c, id, false)?;
    routine_creation::external_grant::delete_bot(c, id)?;
    nunca_fazer::on_bot_deleted(c, id)?;
    usage_limits::on_bot_deleted(c, id)?;
    teto::on_bot_deleted(c, id)?;
    times::on_bot_deleted(c, id)?;
    resumo_diario::on_bot_deleted(c, id)?;
    lider_canal::on_bot_deleted(c, id)?;
    native_catalog::on_bot_deleted(c, id)?;
    genui::storage::on_bot_deleted(c, id)?;
    genui::chart_store::on_bot_deleted(c, id)?;
    plugins::on_bot_deleted(c, id)?;
    whatsapp::on_bot_deleted(c, id)?;
    mail_manager::on_bot_deleted(c, id)?;
    roteamento::receipt::on_bot_deleted(c, id)?;
    crate::chat::memory::omitted::forget_bot(c, id)?;
    rules::store::forget_bot(c, id)
}

#[cfg(all(test, unix))]
mod usage_probe;

pub(crate) mod assinatura_pessoal;
pub(crate) mod ponte_credencial;

pub(crate) mod compressao;
pub(crate) mod motor_background;
pub(crate) mod motor_codex_audit;
pub(crate) mod motor_exec;

pub(crate) mod linux_installer;
pub(crate) mod motor_install;
#[allow(
    dead_code,
    reason = "MEX-1293 fatia 1: motor experimental atrás de flag; o adaptador do loop do bot é a próxima fatia."
)]
pub(crate) mod motor_mexerica;
pub(crate) mod motor_migration;
#[cfg(unix)]
pub(crate) mod motor_publish;
pub(crate) mod motor_roots;
#[cfg(unix)]
pub(crate) mod motor_uploads;
pub(crate) mod motor_workspace_memory;
pub(crate) mod motores_retirados;

#[cfg(all(test, unix))]
mod motor_roots_tests;
