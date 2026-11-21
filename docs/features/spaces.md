# Spaces

Spaces separam contextos pessoais e de trabalho no Hub. No desktop, o seletor no topo da barra lateral troca o app inteiro: bots, Berga, conversa, compositor e painel do Space escolhido. O + de Agentes cria bots no próprio Space. A criação abre a conversa fixa do novo bot, sem trocar para Pessoal.

A conta padrão do dono no mesmo Hub é exibida no assistente e exige confirmação, inclusive ao usar Criar já. Escolher outra conta depende da autoridade e do estado que o Hub informar. Se ela ficar indisponível, o app bloqueia a criação; não usa outra em silêncio.

Cada intenção fica presa ao Hub, Space e conversa de origem. Nada é redirecionado a Pessoal/outro Hub após queda, reconexão ou mudança de seleção. Histórico e memória continuam com as fronteiras do Host; o assistente não sugere funções de bots pessoais no Space. Não há nova concessão de autoridade, compartilhamento de memória ou cópia de credenciais.

Conversas canônicas têm texto, anexos, cartões de aprovação e canais pelo mesmo caminho do Pessoal. Equipes, Rotinas, Sessões, Conectores/cofre e Projetos aparecem apagados com “Ainda não disponível neste Space”; Skills indica “Só no Pessoal”. A faixa mantém as ferramentas. ⌘1…9 escolhe o Space, ⇧⌘K abre o seletor e ⌃Tab/⌃⇧Tab percorre a lista; Command+Space abre o Berga ativo.

Lojas de todos os Spaces conhecidos permanecem conectadas para acompanhar trabalho e não lidas. Cache e rascunhos são separados por Hub/Space. Texto e anexos voltam ao retornar à conversa; anexos de rascunho não persistem após fechar o app. Queda não troca o destino. Conversas adicionais antigas do Berga do Pessoal ficam em **Histórico legado**, somente leitura; não se criam novas por esse caminho. Nenhum dado é migrado ou apagado por trocar de Space.

Uso: [guia de Spaces](../guides/spaces.md). Contrato: [Spaces e conversas](../reference/spaces.md).

Em Spaces, o vínculo de conta precisa ser explícito. O padrão só pode ser pré-selecionado quando o Hub informa uma única conta legada conectada para o motor. Sem conta comprovada, sem autoridade para consultá-la ou com padrão ambíguo, é preciso escolher outra conta autorizada; não há fallback silencioso. Os motores com contas vinculáveis neste contrato são Codex e Claude.

Todo Space de trabalho pode ganhar seu principal **Berga**: nome e persona fixos, aparência própria do Space (uma de seis figuras do estilo Simples mais cor), conta de IA escolhida no Hub. Espaços antigos sem principal mostram "Ativar o Berga" na vista do Space e na folha de edição; nada migra sozinho. O Pessoal não é tocado.

A exclusão permanente de um Space existe só no aplicativo do computador, exige a mesma autoridade do nascimento e confirmação digitando o nome do Space. Se algum bot do Space estiver trabalhando, o Hub recusa e mostra só os nomes. A remoção apaga o Space inteiro — bots, conversas, memória, uploads, recibos, times, projetos e dados de roteamento daquele Space — sem tocar em outros Spaces nem no Pessoal.


Depois de ativado, a folha oferece **Aparência do Berga neste Space** para trocar
figura/cor. Principais antigos com outro nome oferecem **Renomear para Berga**;
nenhum nome muda sozinho. Hub sem o contrato de ciclo de vida bloqueia ativação e
exclusão com orientação de atualizar. Ao excluir, o app volta ao Pessoal e confirma
a remoção. Cancelar ou Escape fecha a confirmação sem excluir.

A confirmação lista todos os bots do Space e o alcance da remoção. Pessoal, outros
Spaces, contas de IA e logins do cofre são preservados; backups antigos e cópias dos
fornecedores podem continuar existindo. Se houver trabalho, inclusive do Berga do
Pessoal numa conversa antiga do Space, pare os bots indicados antes de tentar de novo.
