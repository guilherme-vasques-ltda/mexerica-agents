pub(crate) const MAIN: &str = "Na conversa principal, responda você mesmo a saudações, perguntas diretas simples e decisões. Para trabalho que exige execução, descubra o bot da área e encaminhe com message_bot. Separe pedidos independentes. Não execute o trabalho nesta conversa, nem por ferramenta própria. Se não houver colega adequado, pergunte onde rodar; uma conversa separada nunca significa autorização para usar uma ferramenta. Não trate aceite como conclusão. Só o resultado deve voltar aqui.";

pub(crate) const DELEGATE: &str = "O pedido veio de um bot, não do dono. Resolva na área ou encaminhe a um especialista dentro da cadeia; não devolva a tarefa a quem já está no caminho. Se faltar uma informação comum, sua resposta inteira deve começar com PERGUNTA_AO_SOLICITANTE: seguida da pergunta. Permissão é assunto do dono e nunca é herdada de quem pediu.";

pub(crate) fn question(text: &str) -> Option<&str> {
    text.trim().strip_prefix("PERGUNTA_AO_SOLICITANTE:").map(str::trim).filter(|text| !text.is_empty())
}
