//! MB-1b (D326): `ask_owner` no ator. Só o turno do próprio bot, com execução viva, numa conversa onde o
//! dono escreve (a própria ou um canal); pedidos de outro bot/Times e rotinas não têm dono na conversa.
use super::Actor;
use crate::fork::push_textos::{Corpo, Titulo};
use crate::fork::sessoes::{ExecutionRef, bindings};
use crate::remote::push::{self, AlertKind};

impl Actor {
    pub(super) fn ask_owner(&mut self, text: String, execution: Option<&ExecutionRef>) -> anyhow::Result<()> {
        let execution = execution.ok_or_else(|| anyhow::anyhow!("chamada vencida: execução ausente"))?;
        bindings::recheck(&self.hub, execution)?;
        let Some(turn) = self.turn.filter(|_| !self.waiting_for_reset()) else { anyhow::bail!("no turn is running") };
        if self.active_request.is_some() {
            anyhow::bail!(
                "ask_owner não está disponível em pedido de outro bot ou de uma equipe: devolva a dúvida na sua resposta final"
            );
        }
        if self.active_routine.is_some() {
            anyhow::bail!(
                "ask_owner não está disponível em rotina, que não tem dono na conversa: registre a dúvida na resposta"
            );
        }
        self.close_seg();
        crate::fork::owner_question::ask(&self.hub, &self.cfg.id, &self.lane, turn, &text, execution)?;
        // Ajuste E: a mesma notificação da permissão pendente; tocar abre onde está o cartão (o canal, se for o caso).
        let target = self.hub.store.bot(&self.lane.chat).ok().flatten().map_or_else(|| self.cfg.clone(), |r| r.config);
        push::notify(
            &self.hub,
            &target,
            Some(&self.cfg.id),
            &Titulo::Precisa(&self.cfg.name),
            &Corpo::Texto(&text),
            AlertKind::NeedsInput,
        );
        // Na conversa própria, a pergunta já é o que o dono lê: a resposta final fica no rastro, sem repetir.
        if self.active_group.is_none() {
            self.sent.push(text);
        }
        Ok(())
    }
}
