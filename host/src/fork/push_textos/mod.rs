//! MX-138: os textos da notificação push no idioma escolhido no app de CADA aparelho.
//!
//! O app do iPhone já mostra `title`/`subtitle`/`body` selados que o Hub manda; aqui ficam os
//! templates nos seis idiomas do app (`idioma_app::IDIOMAS`). "Mexerica" e "Berga" são nomes
//! próprios, iguais em todos os idiomas, em alfabeto latino. `Corpo::Texto` passa como veio:
//! já é texto pronto (resposta do agente). O aviso de rotina do Hub sai traduzido por
//! `Corpo::Rotina`, no idioma do aparelho como o resto.
use crate::remote::push::AlertKind;
use crate::store::Store;

#[cfg(test)]
mod tests;

/// O título do alerta, montado por quem chama conforme o que aconteceu.
pub(crate) enum Titulo<'a> {
    /// Só o nome do bot (fim de tarefa, rotina, resposta do agente).
    Nome(&'a str),
    /// "{nome} precisa de você" — permissão pendente ou pergunta do dono.
    Precisa(&'a str),
    /// "{nome} em {grupo}" — última resposta de um membro num grupo.
    NoGrupo { nome: &'a str, grupo: &'a str },
}

/// A situação do aviso de rotina, para o corpo do push sair no idioma do aparelho.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Situacao {
    Terminou,
    Precisa,
    Falhou,
    Interrompida,
}

/// O motivo de uma falha observada pelo Hub, traduzido nos 6 idiomas (nunca texto do Hub).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motivo {
    /// Passou do tempo limite.
    Tempo,
    /// Limite da assinatura atingido.
    Limite,
    /// É preciso entrar de novo na conta do bot.
    Entrar,
    /// Não foi possível começar/enfileirar.
    NaoComecou,
    /// O agente parou antes de terminar (demais motivos observados).
    Parou,
}

/// De onde vem o detalhe do aviso de rotina.
pub(crate) enum Detalhe<'a> {
    /// Resumo escrito pelo bot — passa como veio, sem retraduzir.
    Resumo(&'a str),
    /// Motivo observado pelo Hub — sai no idioma do aparelho.
    Motivo(Motivo),
    /// Nada do bot para mostrar: o texto padrão do idioma ("confira o histórico").
    Padrao,
}

/// O corpo do alerta visível no aparelho.
pub(crate) enum Corpo<'a> {
    /// Texto pronto, já escrito (resposta do agente, resultado final da rotina).
    Texto(&'a str),
    /// "A rotina «nome» <situação>: <detalhe>" — o detalhe é resumo do bot, motivo
    /// observado traduzido ou o texto padrão do idioma; texto do Hub nunca passa.
    Rotina {
        nome: &'a str,
        situacao: Situacao,
        detalhe: Detalhe<'a>,
    },
    /// "Reagiu {emoji} à sua mensagem" — o turno inteiro foi uma reação.
    Reagiu(&'a str),
    Terminou,
    NaoTerminou,
}

/// Os templates de um idioma. `{n}` = nome, `{g}` = grupo, `{e}` = emoji, `{d}` = detalhe.
struct Textos {
    precisa: &'static str,
    no_grupo: &'static str,
    rotina_terminou: &'static str,
    rotina_precisa: &'static str,
    rotina_falhou: &'static str,
    rotina_interrompida: &'static str,
    rotina_padrao: &'static str,
    motivo_tempo: &'static str,
    motivo_limite: &'static str,
    motivo_entrar: &'static str,
    motivo_nao_comecou: &'static str,
    motivo_parou: &'static str,
    reagiu: &'static str,
    terminou: &'static str,
    nao_terminou: &'static str,
    sub_done: &'static str,
    sub_needs: &'static str,
    sub_failed: &'static str,
    gen_done: &'static str,
    gen_needs: &'static str,
    gen_failed: &'static str,
}

const PT_BR: Textos = Textos {
    precisa: "{n} precisa de você",
    no_grupo: "{n} em {g}",
    rotina_terminou: "A rotina «{n}» terminou: {d}",
    rotina_precisa: "A rotina «{n}» precisa de você: {d}",
    rotina_falhou: "A rotina «{n}» não conseguiu terminar: {d}",
    rotina_interrompida: "A rotina «{n}» foi interrompida: {d}",
    rotina_padrao: "confira o histórico da execução.",
    motivo_tempo: "passou do tempo limite e foi parada.",
    motivo_limite: "o limite da assinatura foi atingido.",
    motivo_entrar: "é preciso entrar de novo na conta usada por este bot.",
    motivo_nao_comecou: "não foi possível começar.",
    motivo_parou: "o bot parou antes de terminar.",
    reagiu: "Reagiu {e} à sua mensagem",
    terminou: "Terminou.",
    nao_terminou: "O agente não terminou.",
    sub_done: "Tarefa concluída",
    sub_needs: "Precisa da sua resposta",
    sub_failed: "Tarefa não concluída",
    gen_done: "Sua tarefa terminou. Abra a Mexerica para ler o resultado.",
    gen_needs: "Um bot precisa da sua resposta. Abra a Mexerica para ver.",
    gen_failed: "Uma tarefa não terminou. Abra a Mexerica para ver o problema.",
};

const IDIOMAS: [(&str, &Textos); 6] = [
    ("pt-BR", &PT_BR),
    (
        "en",
        &Textos {
            precisa: "{n} needs you",
            no_grupo: "{n} in {g}",
            rotina_terminou: "Routine “{n}” finished: {d}",
            rotina_precisa: "Routine “{n}” needs you: {d}",
            rotina_falhou: "Routine “{n}” couldn't finish: {d}",
            rotina_interrompida: "Routine “{n}” was interrupted: {d}",
            rotina_padrao: "check the run history.",
            motivo_tempo: "it ran out of time and was stopped.",
            motivo_limite: "the subscription limit was reached.",
            motivo_entrar: "you need to sign in again to the account this bot uses.",
            motivo_nao_comecou: "it couldn't start.",
            motivo_parou: "the bot stopped before finishing.",
            reagiu: "Reacted {e} to your message",
            terminou: "Finished.",
            nao_terminou: "The agent didn't finish.",
            sub_done: "Task complete",
            sub_needs: "Response needed",
            sub_failed: "Task failed",
            gen_done: "Your task is complete. Open Mexerica to read the result.",
            gen_needs: "A bot needs your response. Open Mexerica to review.",
            gen_failed: "A task could not finish. Open Mexerica to review the issue.",
        },
    ),
    (
        "es",
        &Textos {
            precisa: "{n} te necesita",
            no_grupo: "{n} en {g}",
            rotina_terminou: "La rutina «{n}» terminó: {d}",
            rotina_precisa: "La rutina «{n}» te necesita: {d}",
            rotina_falhou: "La rutina «{n}» no pudo terminar: {d}",
            rotina_interrompida: "La rutina «{n}» fue interrumpida: {d}",
            rotina_padrao: "revisa el historial de la ejecución.",
            motivo_tempo: "se agotó el tiempo y se detuvo.",
            motivo_limite: "se alcanzó el límite de la suscripción.",
            motivo_entrar: "hay que iniciar sesión de nuevo en la cuenta que usa este bot.",
            motivo_nao_comecou: "no se pudo empezar.",
            motivo_parou: "el bot se detuvo antes de terminar.",
            reagiu: "Reaccionó {e} a tu mensaje",
            terminou: "Terminó.",
            nao_terminou: "El agente no terminó.",
            sub_done: "Tarea completada",
            sub_needs: "Necesita tu respuesta",
            sub_failed: "Tarea no completada",
            gen_done: "Tu tarea terminó. Abre Mexerica para leer el resultado.",
            gen_needs: "Un bot necesita tu respuesta. Abre Mexerica para ver.",
            gen_failed: "Una tarea no terminó. Abre Mexerica para ver el problema.",
        },
    ),
    (
        "fr",
        &Textos {
            precisa: "{n} a besoin de vous",
            no_grupo: "{n} dans {g}",
            rotina_terminou: "La routine «\u{202F}{n}\u{202F}» a terminé\u{202F}: {d}",
            rotina_precisa: "La routine «\u{202F}{n}\u{202F}» a besoin de vous\u{202F}: {d}",
            rotina_falhou: "La routine «\u{202F}{n}\u{202F}» n'a pas pu se terminer\u{202F}: {d}",
            rotina_interrompida: "La routine «\u{202F}{n}\u{202F}» a été interrompue\u{202F}: {d}",
            rotina_padrao: "consultez l'historique de l'exécution.",
            motivo_tempo: "a dépassé le temps imparti et a été arrêtée.",
            motivo_limite: "la limite de l'abonnement a été atteinte.",
            motivo_entrar: "il faut se reconnecter au compte utilisé par ce bot.",
            motivo_nao_comecou: "n'a pas pu commencer.",
            motivo_parou: "le bot s'est arrêté avant de terminer.",
            reagiu: "A réagi {e} à votre message",
            terminou: "Terminé.",
            nao_terminou: "L'agent n'a pas terminé.",
            sub_done: "Tâche terminée",
            sub_needs: "Réponse nécessaire",
            sub_failed: "Tâche non terminée",
            gen_done: "Votre tâche est terminée. Ouvrez Mexerica pour lire le résultat.",
            gen_needs: "Un bot a besoin de votre réponse. Ouvrez Mexerica pour voir.",
            gen_failed: "Une tâche n'a pas pu se terminer. Ouvrez Mexerica pour voir le problème.",
        },
    ),
    (
        "zh-CN",
        &Textos {
            precisa: "{n} 需要你",
            no_grupo: "{g} 中的 {n}",
            rotina_terminou: "例行任务“{n}”已完成：{d}",
            rotina_precisa: "例行任务“{n}”需要你：{d}",
            rotina_falhou: "例行任务“{n}”未能完成：{d}",
            rotina_interrompida: "例行任务“{n}”被中断：{d}",
            rotina_padrao: "请查看执行历史。",
            motivo_tempo: "超时被停止。",
            motivo_limite: "达到订阅额度上限。",
            motivo_entrar: "需要重新登录此机器人使用的账户。",
            motivo_nao_comecou: "无法开始。",
            motivo_parou: "机器人在完成前停止了。",
            reagiu: "对你的消息做出了 {e} 反应",
            terminou: "已完成。",
            nao_terminou: "智能体没有完成。",
            sub_done: "任务已完成",
            sub_needs: "需要你的回复",
            sub_failed: "任务未完成",
            gen_done: "你的任务已完成。打开 Mexerica 查看结果。",
            gen_needs: "有机器人需要你的回复。打开 Mexerica 查看。",
            gen_failed: "一个任务未能完成。打开 Mexerica 查看问题。",
        },
    ),
    (
        "ar-AE",
        &Textos {
            precisa: "{n} بحاجة إليك",
            no_grupo: "{n} في {g}",
            rotina_terminou: "روتين «{n}» انتهى: {d}",
            rotina_precisa: "روتين «{n}» بحاجة إليك: {d}",
            rotina_falhou: "روتين «{n}» لم يستطع الانتهاء: {d}",
            rotina_interrompida: "روتين «{n}» تمت مقاطعته: {d}",
            rotina_padrao: "راجع سجل التنفيذ.",
            motivo_tempo: "انتهى الوقت وتوقفت.",
            motivo_limite: "تم بلوغ حد الاشتراك.",
            motivo_entrar: "يلزم تسجيل الدخول مجددًا إلى الحساب الذي يستخدمه هذا الروبوت.",
            motivo_nao_comecou: "تعذر البدء.",
            motivo_parou: "توقف الروبوت قبل الانتهاء.",
            reagiu: "تفاعل بـ{e} على رسالتك",
            terminou: "انتهى.",
            nao_terminou: "لم ينتهِ الوكيل.",
            sub_done: "اكتملت المهمة",
            sub_needs: "بحاجة إلى ردك",
            sub_failed: "لم تكتمل المهمة",
            gen_done: "انتهت مهمتك. افتح Mexerica لقراءة النتيجة.",
            gen_needs: "أحد الروبوتات بحاجة إلى ردك. افتح Mexerica للاطلاع.",
            gen_failed: "لم تنتهِ إحدى المهام. افتح Mexerica للاطلاع على المشكلة.",
        },
    ),
];

fn motivo_texto(tx: &Textos, motivo: Motivo) -> &'static str {
    match motivo {
        Motivo::Tempo => tx.motivo_tempo,
        Motivo::Limite => tx.motivo_limite,
        Motivo::Entrar => tx.motivo_entrar,
        Motivo::NaoComecou => tx.motivo_nao_comecou,
        Motivo::Parou => tx.motivo_parou,
    }
}

fn textos(codigo: &str) -> &'static Textos {
    IDIOMAS.iter().find(|(c, _)| *c == codigo).map_or(&PT_BR, |(_, t)| *t)
}

/// Interpola `{chave}` de `pares` numa passada só: um marcador dentro de um valor inserido
/// (um bot chamado "Ana {g}") nunca é reprocessado como template.
fn preencher(template: &str, pares: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut resto = template;
    while let Some(i) = resto.find('{') {
        out.push_str(&resto[..i]);
        let depois = &resto[i..];
        let Some(fim) = depois.find('}') else {
            out.push_str(depois);
            resto = "";
            break;
        };
        match pares.iter().find(|(k, _)| *k == &depois[1..fim]) {
            Some((_, valor)) => out.push_str(valor),
            None => out.push_str(&depois[..=fim]),
        }
        resto = &depois[fim + 1..];
    }
    out.push_str(resto);
    out
}

/// O título do alerta no idioma do aparelho.
pub(crate) fn titulo(idioma: &str, t: &Titulo<'_>) -> String {
    let tx = textos(idioma);
    match t {
        Titulo::Nome(nome) => (*nome).to_owned(),
        Titulo::Precisa(nome) => preencher(tx.precisa, &[("n", nome)]),
        Titulo::NoGrupo { nome, grupo } => preencher(tx.no_grupo, &[("n", nome), ("g", grupo)]),
    }
}

/// O corpo do alerta no idioma do aparelho.
pub(crate) fn corpo(idioma: &str, c: &Corpo<'_>) -> String {
    let tx = textos(idioma);
    match c {
        Corpo::Texto(texto) => (*texto).to_owned(),
        Corpo::Rotina { nome, situacao, detalhe } => {
            let template = match situacao {
                Situacao::Terminou => tx.rotina_terminou,
                Situacao::Precisa => tx.rotina_precisa,
                Situacao::Falhou => tx.rotina_falhou,
                Situacao::Interrompida => tx.rotina_interrompida,
            };
            let detalhe = match detalhe {
                Detalhe::Resumo(texto) => (*texto).to_owned(),
                Detalhe::Motivo(motivo) => motivo_texto(tx, *motivo).to_owned(),
                Detalhe::Padrao => tx.rotina_padrao.to_owned(),
            };
            preencher(template, &[("n", nome), ("d", detalhe.as_str())])
        }
        Corpo::Reagiu(emoji) => preencher(tx.reagiu, &[("e", emoji)]),
        Corpo::Terminou => tx.terminou.to_owned(),
        Corpo::NaoTerminou => tx.nao_terminou.to_owned(),
    }
}

/// A linha secundária do alerta ("Tarefa concluída", "Precisa da sua resposta", ...).
pub(crate) fn subtitulo(idioma: &str, kind: AlertKind) -> &'static str {
    let tx = textos(idioma);
    match kind {
        AlertKind::Done => tx.sub_done,
        AlertKind::NeedsInput => tx.sub_needs,
        AlertKind::Failed => tx.sub_failed,
    }
}

/// A linha genérica que o relay/APNs veem (o conteúdo real vai selado por aparelho).
pub(crate) fn generica(idioma: &str, kind: AlertKind) -> &'static str {
    let tx = textos(idioma);
    match kind {
        AlertKind::Done => tx.gen_done,
        AlertKind::NeedsInput => tx.gen_needs,
        AlertKind::Failed => tx.gen_failed,
    }
}

/// O idioma das notificações de um aparelho: o que ELE guardou no app, só com a geração atual da
/// concessão (aparelho revogado ou desconhecido não herda idioma antigo); sem isso, o do dono;
/// sem nada, pt-BR.
pub(crate) fn idioma_do_aparelho(store: &Store, device_key: &str) -> &'static str {
    crate::fork::idioma_app::do_aparelho(store, device_key).unwrap_or_else(|| crate::fork::idioma_app::do_dono(store))
}
