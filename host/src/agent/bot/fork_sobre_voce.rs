//! D6 (MEX-1347): a pessoa da conversa decide a nota "Sobre você" do turno. Num turno humano ela vem só da origem
//! que o Hub carimbou nas mensagens (`chat::memory::profile::person_of`). Pedido de outro bot carrega a pessoa
//! comprovada da conversa do remetente, carimbada pelo Hub na admissão; sem ela, o pedido não herda a pessoa da
//! sessão do destinatário. Rotina e retomada não herdam pessoa nenhuma (a resposta a um cartão de escolha segue a da
//! sessão que o originou). Canal pode ter várias pessoas e não tem nenhuma. Uma sessão ligada a outra pessoa, a uma
//! sem pessoa ou sem a marca nova (anterior à prova de pessoa, possivelmente já com a nota) nunca é reaproveitada:
//! a conversa abre uma sessão nova, sem o que a anterior ouviu.
use super::Actor;
use crate::LockExt;
use crate::chat::{handoff, memory};
use anyhow::{Result, ensure};

impl Actor {
    /// Fixa a pessoa do turno em `self.cfg.person` antes do preparo da sessão. `human` diz se o turno é das palavras
    /// de quem escreve (não oculto, não pedido, rotina nem Canal).
    pub(super) fn bind_profile_person(&mut self, human: bool, entry_ids: &[String]) -> Result<()> {
        let slot = self.slot();
        let binding = self
            .session_for(slot.as_deref())
            .map(|sid| memory::profile_binding(&self.hub.store, self.cfg.session_owner(), &sid));
        let heard = binding.as_ref().and_then(|b| b.person().map(str::to_owned));
        let person = if self.active_group.is_some() {
            None
        } else if human {
            let entries: Vec<_> = entry_ids.iter().filter_map(|id| self.hub.store.entry(id)).collect();
            // A resposta a um cartão de escolha desta conversa é gravada pelo próprio Hub, sem ingresso: não é
            // remetente novo e segue a pessoa da sessão, como um turno sem mensagem nova.
            let written: Vec<_> =
                entries.iter().filter(|entry| !crate::fork::genui::is_choice_response(&entry.data)).collect();
            if entries.len() < entry_ids.len() {
                None
            } else if written.is_empty() {
                heard.clone()
            } else {
                memory::profile::person_of(written)
            }
        } else if self.compacting.is_some() && self.active_routine.is_none() && self.active_request.is_none() {
            // A compactação trabalha na própria ACP já vinculada: não é um novo remetente.
            heard.clone()
        } else {
            // Sem prova da pessoa do pedido, nenhuma: a pessoa da sessão do destinatário nunca é herdada.
            self.active_request.as_ref().and_then(|request| request.person.clone())
        };
        self.adopt_profile_person(slot.as_deref(), binding.as_ref(), person, entry_ids)
    }

    fn adopt_profile_person(
        &mut self,
        slot: Option<&str>,
        binding: Option<&memory::ProfileBinding>,
        person: Option<String>,
        entry_ids: &[String],
    ) -> Result<()> {
        let wanted =
            person.as_deref().map_or(memory::ProfileBinding::Nobody, |p| memory::ProfileBinding::Person(p.to_owned()));
        // `binding` `None` ou `Unknown` é sessão sem a marca nova: procedência desconhecida, largada como qualquer
        // divergência.
        if binding != Some(&wanted) {
            // Um cartão gerado pelo Hub pode atravessar a troca só com prova durável da MESMA pessoa da origem.
            // Cartões legados ou sem prova continuam recusados; um vínculo ACP divergente nunca é reaproveitado.
            let card = self
                .session_card
                .as_ref()
                .filter(|card| card.lane == self.lane)
                .map(|card| (card.card.id.as_str(), card.card.text.as_str()))
                .or_else(|| {
                    (self.handoff_chat.as_ref() == Some(&self.lane))
                        .then(|| self.handoff_card.as_deref().zip(self.handoff.as_deref()))
                        .flatten()
                });
            let keep_card = if binding.is_none()
                && self.active_group.is_none()
                && self.active_request.is_none()
                && self.active_routine.is_none()
            {
                match (person.as_deref(), card) {
                    (Some(person), Some((id, text))) => {
                        handoff::proven_person(&self.hub.store, id, text)?.as_deref() == Some(person)
                    }
                    _ => false,
                }
            } else {
                false
            };
            self.set_session(slot, None);
            ensure!(self.session_for(slot).is_none(), "profile_session_not_released");
            self.cut_history_transport(keep_card, person.as_deref(), entry_ids)?;
        }
        self.cfg.person = person;
        Ok(())
    }

    /// A troca de pessoa vale para TODO o histórico transportado, não só para a ACP largada: a reconstrução por
    /// mensagens, o cartão de passagem capturado e o resumo em buffer que ouviram outra pessoa ou procedência
    /// desconhecida nunca entram na sessão nova (fail-closed; a perda de continuidade é o custo assumido). As
    /// mensagens originais ficam gravadas — só o transporte para a sessão nova é cortado, e o corte é durável:
    /// turnos seguintes desta conversa também não recuperam o que é anterior a ele.
    fn cut_history_transport(&mut self, keep_card: bool, person: Option<&str>, entry_ids: &[String]) -> Result<()> {
        if !keep_card {
            // A captura anterior ao preparo também alimenta retry e recuperação. Ela não pode ressuscitar o texto
            // que acabou de ser recusado pela troca de pessoa.
            let lane = self.lane.clone();
            if let Some(input) = self.input.as_mut().filter(|input| input.lane() == lane) {
                input.handoff = None;
            }
            self.discard_waiting_handoff(&lane)?;
        } else if self.has_session_card(&self.lane.clone())
            && self.handoff_chat.as_ref().is_none_or(|lane| lane == &self.lane)
        {
            // A prova cobre o cartão PR5, não outro buffer legado que pudesse estar na mesma lane.
            self.handoff = None;
            self.handoff_chat = None;
            self.handoff_card = None;
        }
        // Um resumo de outra conversa não participa deste turno e deve ficar nela.
        if !keep_card && self.handoff_chat.as_ref().is_none_or(|lane| lane == &self.lane) {
            self.handoff = None;
            self.handoff_chat = None;
        }
        if !keep_card && self.has_session_card(&self.lane.clone()) {
            self.session_card = None;
        }
        // O rascunho do cartão vive no banco por lane: sem a chave, a próxima captura não o enxerga.
        if !keep_card {
            self.hub.store.kv_delete(&handoff::key_for(&self.lane))?;
        }
        let mut db = self.hub.store.db.locked();
        let tx = db.transaction()?;
        let recent =
            crate::store::history_in_session_on(&tx, &self.lane.chat, Some(self.lane.session_id()), i64::MAX, 201)?;
        let latest = recent.last().map_or(0, |entry| entry.seq);
        let first = person.and_then(|person| first_proven_input(&recent, entry_ids, person));
        // Nunca reabra um corte anterior se uma mensagem antiga estava esperando na fila.
        let prior = memory::history_cut_lane_on(&tx, &self.lane)?.saturating_sub(1);
        let cut = first.and_then(|seq| seq.checked_sub(1)).unwrap_or(latest).max(prior);
        memory::save_history_cut_on(&tx, &self.lane, cut)?;
        tx.commit()?;
        Ok(())
    }
}

/// Só preserva a entrada que provocou o corte se ela e tudo que já poderia ter sido transportado depois dela forem
/// comprovadamente da mesma pessoa. Respostas já existentes após a entrada tornam a ordem ambígua e fecham a porta.
fn first_proven_input(entries: &[crate::store::Entry], ids: &[String], person: &str) -> Option<i64> {
    if ids.is_empty() {
        return None;
    }
    let current: Vec<_> = entries.iter().filter(|entry| ids.contains(&entry.id)).collect();
    if current.len() != ids.len()
        || current.iter().any(|entry| memory::profile::author(entry).as_deref() != Some(person))
    {
        return None;
    }
    let first = current.iter().map(|entry| entry.seq).min()?;
    for entry in entries.iter().filter(|entry| entry.seq >= first) {
        if entry.kind == "agent" && entry.data["final"] == true {
            return None;
        }
        if entry.kind == "user"
            && (entry.data["status"] != "queued" || ids.contains(&entry.id))
            && memory::profile::author(entry).as_deref() != Some(person)
        {
            return None;
        }
    }
    Some(first)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::super::new_actor;
    use crate::LockExt;
    use crate::{
        api::{devices::Caller, dispatch},
        chat::{
            handoff,
            memory::{self, profile::OWNER},
        },
        hub::Hub,
        store::{BotConfig, EntryKind, Lane, Store},
    };
    use serde_json::{Value, json};
    use std::sync::Arc;

    const NOTE: &str = "SOBRE_VOCE_DONO_SENTINELA prefere café.";

    async fn fixture() -> (Arc<Hub>, BotConfig, String, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("mexerica-sobre-voce-ator-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let identity = crate::remote::identity::Identity::load_or_create(&dir).unwrap();
        let hub = Hub::new(store, "sobre-voce-ator".into(), identity, "fixture".into(), 0);
        let snap = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({"hubId":hub.host_id})).await.unwrap();
        let created = dispatch(
            &hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({
                "hubId":hub.host_id, "expectedRevision":snap["revision"], "name":"Equipe",
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"EQ"}}
            }),
        )
        .await
        .unwrap();
        let space = created["spaces"].as_array().unwrap().iter().find(|s| s["name"] == "Equipe").unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let cfg: BotConfig = serde_json::from_value(
            json!({"id":"bot","name":"Bot","backend":"fixture","cwd":dir,"notify":false,"spaceId":space}),
        )
        .unwrap();
        hub.store.save_bot(&cfg).unwrap();
        let body = json!({"spaceId":space,"content":NOTE,"base":Value::Null});
        dispatch(&hub, &Caller::Local, "saveMemoryProfile", body).await.unwrap();
        (hub, cfg, space, dir)
    }

    /// Mensagem humana com o ingresso que o Hub carimba no envio (`None`: sem origem comprovada).
    fn message(hub: &Hub, caller: Option<&str>) -> String {
        let mut data = json!({"text":"oi","status":"queued"});
        if let Some(caller) = caller {
            data["contentOrigin"] = json!({"schema":1,"status":"unknown","ingress":{"caller":caller}});
        }
        hub.store.insert_entry(&Lane::main("bot"), EntryKind::User, 1, &data).unwrap().id
    }

    #[test]
    fn corte_so_preserva_entrada_de_autoria_e_ordem_comprovadas() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let lane = Lane::main("bot");
        let owner = store
            .insert_entry(
                &lane,
                EntryKind::User,
                1,
                &json!({"text":"atual","status":"queued","contentOrigin":{"ingress":{"caller":"local"}}}),
            )
            .unwrap();
        let other =
            store.insert_entry(&lane, EntryKind::User, 2, &json!({"text":"outra pessoa","status":"queued"})).unwrap();
        let read = || store.history("bot", i64::MAX, 201).unwrap();
        assert_eq!(super::first_proven_input(&read(), std::slice::from_ref(&owner.id), OWNER), Some(owner.seq));
        let mut data = other.data;
        data["status"] = "sent".into();
        store
            .db
            .locked()
            .execute("UPDATE entries SET data=?2 WHERE id=?1", rusqlite::params![other.id, data.to_string()])
            .unwrap();
        assert_eq!(super::first_proven_input(&read(), std::slice::from_ref(&owner.id), OWNER), None);
        assert_eq!(super::first_proven_input(&read(), std::slice::from_ref(&owner.id), "membro-b"), None);
    }

    #[tokio::test]
    async fn passagem_exige_mesma_pessoa_texto_integro_e_sessao_nova() {
        for (card_person, alter, legacy, expected) in [
            (Some(OWNER), false, false, true),
            (Some("membro-b"), false, false, false),
            (None, false, false, false),
            (Some(OWNER), true, false, false),
            (Some(OWNER), false, true, false),
        ] {
            let (hub, cfg, _, dir) = fixture().await;
            let lane = Lane::main("bot");
            let card = {
                let db = hub.store.db.locked();
                handoff::draft_on(&db, &lane, "bot", handoff::Reason::Restart, "resumo", card_person).unwrap().1
            };
            if alter {
                let mut data = card.data.clone();
                data["handoff"]["text"] = "resumo adulterado".into();
                hub.set_entry(&card.id, &data).unwrap();
            }
            let mut actor = new_actor(hub.clone(), cfg);
            if legacy {
                actor.session_id = Some("sessao-sem-marca".into());
            }
            actor.capture_session_card(&lane).unwrap();
            let owner = message(&hub, Some("local"));
            actor.bind_profile_person(true, &[owner]).unwrap();
            assert_eq!(actor.has_session_card(&lane), expected, "{card_person:?}, {alter}, {legacy}");
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn entrada_antiga_da_fila_nao_reabre_corte_anterior() {
        let (hub, cfg, _, dir) = fixture().await;
        let entry = message(&hub, Some("local"));
        hub.store.kv_set(&memory::history_cut_key("bot"), "999").unwrap();
        let mut actor = new_actor(hub.clone(), cfg);
        actor.cut_history_transport(false, Some(OWNER), &[entry]).unwrap();
        assert_eq!(memory::history_cut(&hub.store, "bot").unwrap(), 1000);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn dono_recebe_outra_pessoa_nao_e_a_sessao_do_dono_nao_e_reaproveitada() {
        let (hub, cfg, space, dir) = fixture().await;
        let mut actor = new_actor(hub.clone(), cfg);
        actor.session_id = Some("s-dono".into());

        // O dono, pela origem carimbada: recebe a nota do Space e a sessão fica dele.
        let owner = message(&hub, Some("local"));
        actor.bind_profile_person(true, std::slice::from_ref(&owner)).unwrap();
        assert_eq!(actor.cfg.person.as_deref(), Some(OWNER));
        assert!(actor.compose("s-dono", "oi").await.unwrap().contains(NOTE));
        assert_eq!(memory::profile_person(&hub.store, "bot", "s-dono").as_deref(), Some(OWNER));

        // Compactar a própria sessão conserva sua pessoa; não é um turno de outra origem.
        actor.session_id = Some("s-dono".into());
        actor.compacting = Some(false);
        actor.bind_profile_person(false, &[]).unwrap();
        assert_eq!(actor.cfg.person.as_deref(), Some(OWNER));
        assert_eq!(actor.session_id.as_deref(), Some("s-dono"));
        actor.compacting = None;

        // Cortar outra conversa não destrói o resumo privado, que continua preso à própria lane.
        actor.handoff = Some("resumo privado".into());
        actor.handoff_chat = Some(Lane::main("outra-conversa"));
        actor.adopt_profile_person(None, Some(&memory::ProfileBinding::Unknown), None, &[]).unwrap();
        assert_eq!(actor.handoff.as_deref(), Some("resumo privado"));
        assert_eq!(actor.handoff_chat, Some(Lane::main("outra-conversa")));
        actor.handoff = None;
        actor.handoff_chat = None;

        // Pedido de outro bot SEM pessoa comprovada na origem não herda a da sessão: nenhuma nota, sessão nova.
        let (request, _answer) = crate::chat::team::BotRequest::for_test("pedido");
        actor.active_request = Some(request);
        actor.bind_profile_person(false, &[]).unwrap();
        assert_eq!(actor.cfg.person, None);
        assert_eq!(actor.session_id, None, "pedido sem pessoa não reusa a sessão do dono");
        actor.active_request = None;
        // Pedido com a pessoa comprovada da conversa do remetente: segue nela, na sessão que já é dela.
        actor.session_id = Some("s-dono".into());
        let (mut request, _answer) = crate::chat::team::BotRequest::for_test("pedido-2");
        request.person = Some(OWNER.into());
        actor.active_request = Some(request);
        actor.bind_profile_person(false, &[]).unwrap();
        assert_eq!(actor.cfg.person.as_deref(), Some(OWNER));
        assert_eq!(actor.session_id.as_deref(), Some("s-dono"));
        actor.active_request = None;

        // Sessão anterior à marca nova (legada, possivelmente com a nota): remetente desconhecido a larga.
        actor.session_id = Some("s-legada".into());
        let unknown = message(&hub, Some("ponte"));
        actor.bind_profile_person(true, std::slice::from_ref(&unknown)).unwrap();
        assert_eq!(actor.cfg.person, None);
        assert_eq!(actor.session_id, None, "sessão sem a marca nova não é reaproveitada");
        // A largada deixa o corte durável: o que a sessão anterior ouviu não é reconstruído, nem por histórico,
        // cartão de passagem ou resumo em buffer.
        assert!(memory::history_cut(&hub.store, &actor.lane.chat).unwrap() > 0, "o corte de histórico ficou gravado");
        hub.store.kv_set(&handoff::key_for(&actor.lane), "cartao-antigo").unwrap();
        actor.handoff = Some("resumo com a nota".into());
        actor.handoff_chat = Some(actor.lane.clone());
        actor.session_id = Some("s-dono".into());
        let unknown = message(&hub, None);
        actor.bind_profile_person(true, std::slice::from_ref(&unknown)).unwrap();
        assert!(actor.handoff.is_none() && actor.session_card.is_none(), "resumo capturado não atravessa a troca");
        assert!(
            hub.store.kv_get(&handoff::key_for(&actor.lane)).is_none(),
            "o cartão de passagem da lane foi desligado"
        );

        // Outro membro da Equipe (identidade futura) no mesmo bot e Space: a sessão do dono é largada e a dele não
        // recebe a nota do dono.
        actor
            .adopt_profile_person(
                None,
                Some(&memory::ProfileBinding::Person(OWNER.into())),
                Some("membro-b".into()),
                &[],
            )
            .unwrap();
        assert_eq!(actor.session_id, None, "a sessão que ouviu a nota do dono não é reaproveitada");
        let prompt = actor.compose("s-membro", "oi").await.unwrap();
        assert!(!prompt.contains(NOTE), "{prompt}");
        assert_eq!(memory::profile_person(&hub.store, "bot", "s-membro").as_deref(), Some("membro-b"));
        // Uma sessão de outra pessoa nunca é religada ao dono.
        let refused = memory::bind_profile_person(&hub.store, "bot", "s-membro", Some(OWNER)).unwrap_err();
        assert!(refused.to_string().contains("profile_person_mismatch"));

        // Remetente desconhecido (sem origem carimbada) e Ponte: nenhuma nota, e a sessão do dono não serve.
        for caller in [None, Some("ponte")] {
            actor.session_id = Some("s-dono".into());
            let unknown = message(&hub, caller);
            actor.bind_profile_person(true, std::slice::from_ref(&unknown)).unwrap();
            assert_eq!(actor.cfg.person, None);
            assert_eq!(actor.session_id, None, "{caller:?}");
            assert!(!actor.compose("s-desconhecido", "oi").await.unwrap().contains(NOTE));
        }
        // A resposta a um cartão de escolha não é remetente novo: segue a sessão do dono.
        actor.session_id = Some("s-dono".into());
        let choice = json!({"text":"Resposta","choice_response":{"version":1,"questionId":"q","revision":2}});
        let choice = hub.store.insert_entry(&Lane::main("bot"), EntryKind::User, 1, &choice).unwrap().id;
        actor.bind_profile_person(true, std::slice::from_ref(&choice)).unwrap();
        assert_eq!(actor.cfg.person.as_deref(), Some(OWNER));
        assert_eq!(actor.session_id.as_deref(), Some("s-dono"));
        actor.session_id = None;
        // Mensagens de pessoas diferentes no mesmo turno também não identificam ninguém.
        let unknown = message(&hub, None);
        actor.bind_profile_person(true, &[owner.clone(), unknown]).unwrap();
        assert_eq!(actor.cfg.person, None);

        // Canal pode ter várias pessoas: nenhuma nota, mesmo com a mensagem do dono.
        let channel = Lane::main("canal");
        let (reply, _answer) = tokio::sync::oneshot::channel();
        actor.active_group = Some(crate::chat::group::GroupTurn {
            lane: channel.clone(),
            prompt: "[Channel: Sala]\nMensagem".into(),
            shown: vec![],
            epoch: hub.groups.reserve_epoch_for_test(&channel),
            authority: None,
            reply,
        });
        actor.bind_profile_person(false, std::slice::from_ref(&owner)).unwrap();
        assert_eq!(actor.cfg.person, None);
        actor.active_group = None;

        // De volta ao dono: a sessão dele continua dele e a nota volta.
        actor.session_id = Some("s-dono".into());
        actor.bind_profile_person(true, std::slice::from_ref(&owner)).unwrap();
        assert_eq!(actor.session_id.as_deref(), Some("s-dono"));
        assert_eq!(actor.cfg.space_id, space);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
