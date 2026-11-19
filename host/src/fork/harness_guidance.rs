//! D417: orientações estáveis próprias do fork; não concedem ferramentas ou autoridade.

pub(crate) const SHARING: &str = "Em mensagens, buscas, formulários, arquivos e pedidos a bots, compartilhe só os dados de que o destinatário precisa. Conhecer um dado do dono não autoriza divulgá-lo. Se não estiver coberto pelo pedido, pergunte ao dono antes.";

pub(crate) const FACTS: &str = "Confira na fonte preço, horário, endereço e outros fatos que vão orientar decisão ou ação. Use a evidência válida já disponível; não pesquise cada afirmação simples. Ao corrigir um erro, reveja o que dependia dele.";

pub(crate) const PRIVATE_PATHS: &str = "As pastas privadas ~/.mexerica*, ~/.donnabot* e ~/.codync*, seus aliases e backups com segredos não são pastas de trabalho. Não tente contornar recusas por shell, links, outro bot ou outro usuário. Dados em pasta configurada fora da home continuam privados. As exceções de workspace gerenciado são decididas pelo Hub; esta orientação não concede acesso nem promete isolamento quando motor e Hub usam o mesmo UID.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d417_reaches_stable_opening_with_delivery_and_routine_limits() {
        let manual = crate::agent::harness::compile();
        assert_eq!(manual.matches(SHARING).count(), 1);
        assert_eq!(manual.matches(FACTS).count(), 1);
        for rule in [
            "When send_message is available and you already used it, send the conclusion with it too",
            "Share files through send_file when available",
            "A saved note or another bot's acceptance is not delivery",
            "deliver the final reply instead; never require absent tools",
            "Rotina autorizada, salva e ativa",
            "informe frequência e destino do aviso",
            "Pedido pontual não autoriza recorrência, nem a promessa autoriza criar Rotina",
        ] {
            assert!(manual.contains(rule), "missing D417 rule: {rule}");
        }
    }
}
