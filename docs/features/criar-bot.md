# Novo bot

Um Hub novo já cria a [Berga principal](donna-central.md#berga-pronta-no-hub-novo)
no primeiro início. A tela abaixo continua sendo o caminho para criar bots
adicionais; não é necessário usá-la para ter o Berga. Se faltar conta de IA, o
aviso na conversa do Berga orienta conectar e escolher a conta nas configurações.

A criação de bot no app do Mac é um assistente em quatro passos: a
**prévia ao vivo** à esquerda e **Configure seu bot** à direita. Tudo continua editável
depois, nas configurações do bot.

**Prévia, à esquerda**
- O rosto do bot, com **Aleatório** (sorteia outra aparência) e **Personalizar** (abre as abas
  Função e Aparência por cima dos passos. Em Aparência, escolha Simples ou Pelúcia3D e um nome do catálogo; Simples também permite Olhos, Cor, Roupa, Acessório e Objeto, além de “Crie o seu” sem chave. O gerador3D com IA não está nesta tela).
- **Nome do bot**, com o cursor já no campo, e o **dado**, que sorteia outro nome no estilo
  dos bots da biblioteca para o papel escolhido (Crivo, Lupa, Prumo…), nunca nome de pessoa.
  Vazio, o Hub dá um nome depois das primeiras conversas.
- As etiquetas de papel, motor e permissão, o resumo (equipe, onde, pastas, permissões,
  skills e conectores) e a linha real da barra lateral.

**Passos, à direita**
1. **Papel e projeto**: sete papéis comuns, cada um com uma frase, e **Ver todas as funções**,
   que abre a aba Função (exemplos, Suas funções e Nova função). **Equipe**: Sem equipe (padrão) ou
   uma das equipes que o quadro de Equipes do lugar escolhido já tem, com o caminho (Marketing ›
   Conteúdo) e **Reporta a**: o chefe da área primeiro, ou outro gerente dela. O Berga nunca aparece
   como chefe nem é movida (o Hub também recusa com `primary_protected`). Só o dono
   vê a lista; sem leitura, a nota diz que ele começa em Sem equipe. **Onde ele trabalha**: Pessoal (padrão) ou um dos
   Spaces que o app já conhece. Num Space, o Hub exige motor explícito e o bot nasce sem memória
   compartilhada, sem skills e sem gestão da equipe; o Hub confere se o Space existe. Depois de
   criar num Space, o app abre esse Space. O bot nasce na **área própria** criada pelo Hub; a pasta de um projeto se troca depois em
   Configurações do bot → Comportamento. Liberar mais pastas ainda não existe, e a pasta é onde
   ele começa, não um limite.
2. **Motor**: os motores do Hub, com o estado (pronto, falta entrar, não instalado), e o modelo pelo
   **seletor único** da Conversa, o mesmo do compositor, no modo "padrão do bot": Favoritos no
   topo, busca, a estrela que marca favorito e os atalhos ⌘1 a ⌘3 dos três primeiros. Os favoritos são
   os mesmos do compositor, guardados neste aparelho. O tamanho de contexto aparece na dica da linha só quando o
   catálogo real do motor o informa; sem número, não aparece nada. Ao lado ficam o esforço, se o modelo o
   anunciar, e a velocidade (Padrão ou Rápido), se o motor tiver o modo rápido, gravada como `fastMode`.
   Escolher um modelo de outro motor numa aba do seletor troca o motor, como clicar nele, e a conta volta
   ao padrão. A linha **Padrão do motor**, dentro do seletor, deixa o modelo em branco: ele segue o padrão
   do motor (ou da conta). O "Default (recommended)" que o motor anuncia não aparece, porque é a
   mesma escolha. Motor que pede entrada não lista modelos até a pessoa entrar. O seletor só devolve
   a escolha; quem grava é o `createBot`. A nota do passo lembra que esse é o **padrão do bot**: vale
   para conversas novas, e cada conversa pode trocar o seu modelo depois. Na OpenCode, aparece o
   aviso de que a trava de segurança não cobre tudo
   ([Segurança dos motores](seguranca-dos-motores.md)); ele não
   bloqueia a escolha.
   **Conta de IA** (Codex e Claude Code): **Padrão do Hub** ou uma conta gerenciada conectada do
   mesmo motor, gravada em `forkEditor.engineAccountId`, o mesmo campo do editor do bot. A lista
   só aparece quando o Hub responde a capacidade `engine.accounts` para este aparelho (SAAS-04:
   posse ACS4 ativa, ativação e lease); sem ela, a nota diz que ele começa no padrão do Hub e que
   a conta se escolhe depois, em Configurações do bot → Motor. Com conta escolhida, a aba desse
   motor no seletor lista só os modelos que a conta comprovou (nome e contexto do catálogo real
   quando o motor conhece o mesmo modelo), porque o Hub recusa modelo ou opção fora dessa lista. Nada de senha, código ou chave na criação. Se o Hub recusar (autorização
   vencida, conta desconectada, modelo não comprovado, Space inexistente), a tela mostra o
   motivo pelo código do erro, sem repetir a mensagem do Hub.
3. **Permissões**: **Pedir** (padrão e recomendado): cada pedido do motor vira um cartão e as
   mudanças na equipe esperam você. **Liberar**: os pedidos do motor continuam virando cartão,
   mas as mudanças na equipe saem na hora, com aviso e Desfazer. Escolher Liberar pede
   confirmação, com o foco em **Manter Pedir**; sem mexer, vale a autonomia geral.
   **Acesso total** aparece desligado: ainda não existe.
4. **Ferramentas**: skills, conectores, logins salvos e uso do computador começam desligados;
   ligam-se depois, em Configurações do bot → Ferramentas. Nada de senha, token
   ou chave na criação.

O botão final é **Acordar {nome}**. **Criar já, com o resto no padrão** cria o bot a partir de
qualquer passo antes do último, com o que já foi escolhido. A folha envia ao Hub só nome,
função, aparência, motor, modelo, opções do modelo e autonomia; quem valida é o Hub.

Em janela estreita ou baixa, a prévia vira uma faixa no topo. Se não der para ler os padrões do Hub ao abrir
(autonomia geral, motores), a folha diz o motivo pelo código (Hub fora de alcance ou leitura recusada), sem repetir
a mensagem do Hub.

A folha segue o idioma do app (Ajustes → Geral → Idioma, [Idiomas](../guides/idiomas.md)): português, inglês,
espanhol, francês, chinês simplificado e árabe (da direita para a esquerda). Os nomes das funções da biblioteca
aparecem traduzidos; o que vai ao Hub (a função gravada, os IDs) não muda com o idioma.

### Equipe em dois passos

Com equipe escolhida, a criação grava em dois passos, como o [modelo Equipe Marketing](times.md):
primeiro o `createBot`; depois a mesma escrita do quadro de Equipes (`donnaTimes` → `donnaTimesPreview` →
`donnaTimesSave`, com revisão e chave da gravação), numa sessão só da folha, para não levar junto um
rascunho aberto na tela Equipes. A mudança é só esta: o bot novo entra no topo da equipe e passa a reportar
ao chefe escolhido.

Se o segundo passo falhar, **o bot continua criado** (nada é desfeito) e a folha mostra "{nome} foi criado,
mas não entrou na equipe {equipe}" e o motivo pelo código (quadro mudou em outro aparelho, Hub sem resposta,
equipe ou chefe que mudaram, só o dono muda as Equipes…), sem repetir a mensagem do Hub. Enquanto o aviso está aberto, equipe, chefe e Space ficam travados: **Tentar de novo**
refaz só o segundo passo, sempre com o pedido original (mesma equipe, mesmo chefe, mesmo lugar); se a gravação ficou sem resposta, reenvia o mesmo pedido, com a mesma chave.
**Abrir Equipes** leva ao quadro, e **Fechar** termina com o bot em Sem equipe.

## Função

- O Berga é fixo e é o líder: o papel e o nome dele não aparecem como opção nem no
  dado, e a função personalizada do principal não entra em **Suas funções**. A trava de verdade é do Hub. As outras 29 funções da biblioteca são **exemplos**; os rótulos aparecem em português, em forma
  de área ou atividade (Chefe de gabinete, Gestão de projetos, Pesquisa, Desenvolvimento, Redação,
  Design, Contabilidade…), sem mudar IDs, dados nem instruções dos bots. A tradução é só da tela: o Hub
  recebe no `title` o texto original da biblioteca (por exemplo, "Researcher"), e a etiqueta da
  barra lateral mostra esse texto até a frente Idiomas traduzi-la na exibição. A grafia antiga
  de bots existentes continua reconhecida como exemplo.
- Escolher um exemplo aplica a aparência,
  preenche o nome da função e sugere o nome do bot, sem apagar um nome já digitado.
- **Nome da função** é editável, com até 40 caracteres.
- **Nova função** começa uma função do dono: ele dá o nome e ajusta o visual nas outras
  abas, a partir da aparência atual.
- Mudar o corpo, os olhos, a cor, os acessórios ou o selo não troca a função escolhida.

A função fica no próprio bot, no campo `title` da configuração do Hub. Ela aparece como **etiqueta ao lado do nome na barra lateral**, na cor do bot.
Se faltar espaço, a etiqueta encolhe antes do nome. Canais não têm função.

O Hub limpa espaços repetidos e recusa mais de 40 caracteres ou caracteres invisíveis
(controle, direção de texto e largura zero), para a etiqueta não esconder texto. A mesma
regra vale para `createBot`, `updateBot` e o editor do bot.

**Suas funções** mostra as funções que o dono já criou. O Hub não guarda uma lista
separada: elas vêm das funções dos bots atuais que não são um dos 30 exemplos, com a
aparência do bot mais recente. Apagar o último bot de uma função tira essa função da lista.

A função é só texto de exibição: não concede gestão nem muda o papel na equipe.
O preset Donna (nome técnico do avatar) também é só aparência.

## Pelo modelo Equipe Marketing

O [modelo Equipe Marketing](times.md) de Equipes cria os
bots que faltam pelo mesmo `createBot` desta tela, com a função e a aparência do preset,
motor e modelo padrão, autonomia Pedir e conectores e contas desligados. A diferença:
as instruções desses bots já trazem as regras comuns da equipe.

## Depois de criar

Em **Configurações do bot → Identidade**, o campo **Função** muda a etiqueta, com a
mesma regra. Bots criados antes desta tela começam sem função e podem ganhar uma por ali.

## O que ainda não está aqui

Pasta de projeto, skills, conectores e logins entram numa atualização futura do assistente.
O tamanho de contexto só aparece quando o catálogo do motor o informar. O pedido em
texto livre e o rascunho por IA dependem de uma atualização futura do Hub.

## Roteamento inicial

Bot novo começa **Fixo**, sem reservas e sem rodízio. A conta/modelo/opções já
escolhidos são a fonte única do Fixo; nenhuma migração escolhe conta nova.
Depois de criar, **Configurações → Motor** mostra a conta, o modelo e os ajustes.
A escolha automática, os guias e as alternativas aparecem lá apagados, sem ação e
com o selo “ainda indisponível”. A criação de
bot não mostra essas funções. Preferências anteriores continuam guardadas e não mudam a
execução. Consulte [Roteamento de modelos](roteamento-de-modelos.md).


## Principal de um novo Space

Criar Space pede sua identidade e depois o nome, motor e conta de IA do seu
principal. A conta padrão conectada do mesmo Hub aparece identificada, mas
criar exige confirmação explícita. No novo fluxo `donnaSpaceBirth`, Space e principal são salvos juntos, sem
copiar o Berga do Pessoal. Mesmo nome em Spaces diferentes não significa
mesma identidade ou memória. Sem conta conectada ou Hub compatível, o app
explica o bloqueio. Repetir um pedido incerto conserva sua intenção original.

Spaces já existentes continuam sem regularização automática.

A garantia de principal próprio é do novo fluxo de criação do Space. O endpoint legado `donnaSpaceCreate` conserva a criação somente de metadados, e Spaces antigos podem continuar sem principal. Esta etapa não migra dados nem impõe essa garantia globalmente. Birth admite aparelho do dono com Control e vínculo ACS4/ativação/lease vigentes, sem liberar os mutantes legados locais de Space.

Na criação dentro de um Space, o consentimento identifica a conta efetivamente enviada, além do motor e da origem. Mudança dessa seleção invalida a confirmação, inclusive quando a conta padrão muda na atualização da lista. Nomes de conta, motor e Hub têm direção isolada para textos mistos. Se o Hub confirmar a criação depois de uma troca de conexão, o app mostra que o bot foi criado no Space original e orienta reabri-lo, sem oferecer outra criação para a mesma intenção conhecida. Não há promessa de deduplicação após perda do recibo.
