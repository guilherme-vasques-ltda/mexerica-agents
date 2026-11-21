# Mexerica — regras e aprovações

As regras de aprovação passam a valer nos pedidos de permissão que chegam à
Mexerica: uma regra de Recusar nega o pedido na hora, sem cartão. Os demais
pedidos continuam aparecendo em cartões para você decidir. Salvar uma regra de
Permitir ainda não muda o que seus bots podem fazer.

Nem toda ação do programa de IA gera um pedido. Por isso, esta etapa ainda não
garante confirmação antes de toda ação delicada.

Veja o que está sendo preparado e quais são os limites.
Os detalhes abaixo são uma referência para quem desenvolve ou mantém o produto;
você não precisa seguir comandos ou abrir um terminal para continuar usando a Mexerica.

## Pedidos de conectores (cartão comum)

Quando um bot quer fazer algo numa conta ligada por conector (por exemplo, responder
no WhatsApp ou guardar um rascunho no Gmail), a conversa mostra um cartão com o que o
Hub mandou: quem pede, a ação em português (por exemplo, "Enviar mensagem no
WhatsApp"), a **conta de saída** (o número ou e-mail de onde sai) e cada campo com
o nome dado pelo Hub e o conteúdo exato. **Segure para
aprovar** vale só para aquele pedido, uma vez. **Editar** pede ao Hub um cartão novo;
o antigo deixa de valer. **Recusar** é um clique. Depois, o cartão mostra o que
aconteceu: aprovado, enviando, concluído, falhou, recusado, venceu ou **não
confirmado** ("não sabemos se saiu; nada foi repetido"). Sem conteúdo do Hub, sem
conta de saída identificada, fora do prazo ou num app que não reconhece o pedido,
o cartão diz isso e não mostra botões. A conta não se edita nem se escolhe no cartão. Quem decide se o pedido vale é sempre o Hub; a voz não aprova esses
cartões. Hoje o Hub ainda não cria esses pedidos fora dos testes: faltam a fonte das
contas curadas, ligar a apresentação no Hub e, no
WhatsApp, o número confirmado da conta conectada.

## Referência para desenvolvimento

O contrato, vocabulário novo, preservação conservadora de regras
legadas e limites estão na preparação das regras de aprovação.
Não há mais promessa de interpretar comandos/URLs legados por aproximação.

Recusar sempre vence Permitir. Chão precede qualquer liberação, inclusive Acesso
total; cobertura desconhecida não autoriza automaticamente. Pedido autenticado
do dono só diz respeito à ação e alvo correspondentes; conteúdo de fora não cria
pedido humano. Erro ou autoridade inválida fecha, sem efeito.

O porteiro (`fork/rules/gate.rs`) já é consultado em `on_permission`, depois da
conferência de sessão e vínculo e antes do cartão. Ele extrai a identidade da
ferramenta só com prova do adaptador (`_meta.claudeCode.toolName` no Claude,
envelope `server`/`serverName`/`tool` no Codex, entrada gravada do `tool_call`
no pedido correlacionado), compara `acao`/`alvo`/`ferramenta` por igualdade
literal e avalia o pedido com o nível mais restritivo entre a projeção preparada
e o `Permission` legado. Recusar casada responde `reject_once` (ou `cancelled`)
e grava `gate_decision` em `donna_rule_events`; falha de auditoria não rebaixa
o Deny. Permitir automático exige efeito comum comprovado — que nenhum pedido
ACP comprova nesta etapa — e responderia só `allow_once`, com auditoria
obrigatória. Todo o resto segue para o cartão, como antes.

O contrato futuro tem Pedir liberação, Auto aprovar/Liberar e Acesso total.
Todos continuam indisponíveis como política efetiva nova. Na ativação, Codex
permanecerá livre em Liberar/Acesso total e a garantia se limitará ao que passa pelo Hub;
a configuração atual é preservada. No
Chrome nativo do Mac, enviar/pagar/apagar perguntam em Liberar; a exceção se estende
aos computadores do dono. Login não exige biometria por padrão; Touch ID escolhido
no item continua exigido. Helper, Touch ID e cartão de rotina não foram entregues.

A API local conserva `donnaRules`, `donnaApprovalSet`, `donnaRuleSave`,
`donnaRuleSetEnabled` e `donnaRuleDelete`, sem método MCP correspondente.
Criar exige revisão geral; editar/ativar/remover exige revisão da regra; nível
exige revisão daquele sujeito. A edição conserva enabled. Dados, IDs, escopos,
datas e eventos existentes permanecem; um comparador sem tradução comprovada
continua visível e não autoriza. Recusar desconhecida relacionada fecha.
Respostas salvas de envio seguem indisponíveis.

A projeção do legado permanece transacional, preservando Pedir explícito e
restrições do mesmo sujeito. Nenhuma migração cria Acesso total. Bootstrap,
política efetiva, revalidação de cartões e integração dos executores não estão
disponíveis. Configuração salva não comprova essas proteções nem isola
processos que compartilham o usuário do Hub.
