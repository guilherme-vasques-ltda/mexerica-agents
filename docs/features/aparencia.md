# Aparência

Ajustes → Aparência muda como o app do Mac se apresenta. Tudo fica só neste Mac: nada vai para o Hub nem muda os bots.

## O que dá para mudar

| Preferência | Valores | Padrão |
| --- | --- | --- |
| Toques Mexerica | Ligado ou desligado | Ligado |
| Tema | Sistema, Claro, Escuro | Sistema (segue o Mac) |
| Destaque | Verde, Violeta, Azul, Jade, Âmbar, Rosa, Grafite | Verde, da marca Grafite, só para quem nunca escolheu; uma escolha salva, inclusive Violeta, continua |
| Fundo | Nenhum, os dez da Mexerica (Aurora suave, Janela da cidade, Jardim cítrico, Ondas pastel, Montanhas suaves, Linho fino, Folhas botânicas, Nuvens suaves, Canto aconchegante, Vidro cítrico), Serra, Duna, Maré, Noite, Cidade, Risca de giz, Mogno, Cafeteria, Café, Campo, Lago, Horizonte urbano, Escritório, Ondas, Círculos, Blocos, Jardim ou uma imagem sua | Nenhum |
| Desfoque | 0 a 40 px | 0 |
| Transparência | Geral (um valor, 0 a 100%) ou Por área | Geral, 50% |
| Suavizar fundo | 0 a 60% | 18% no claro, 12% no escuro |
| Tamanho do texto, ícones de uso, barra lateral compacta | como antes em Geral | sem mudança |

- Com **Nenhum**, o app fica opaco como sempre foi. Com um fundo, a barra lateral vira vidro sobre ele e a conversa fica direto sobre o fundo: cada texto mora num balão, numa pílula ou num cartão de vidro.
- **Fundo contínuo**: a imagem é uma só, passando por baixo da faixa, da barra lateral e da conversa. Com fundo, a barra lateral fixa não tem contorno, sombra nem linha entre ela e a conversa. A borda continua arrastável e mostra o foco do teclado. Quanto da imagem aparece através da barra é a transparência da **Barra lateral**, que o limite de legibilidade pode segurar; **Suavizar fundo** libera mais. A barra flutuante mantém a sombra.
- **Transparência por área**: em **Por área**, além do Geral aparecem quatro controles: **Barra lateral**, **Painel de detalhes** (o cartão ao lado da conversa), **Caixa de mensagem** (só a caixa onde você escreve; os balões enviados seguem o Geral) e **Fundo da conversa** (uma placa só atrás das mensagens: 100% deixa o fundo inteiro à vista, 0% cobre; ela não passa por baixo do painel de detalhes nem da caixa de mensagem, que mostram o fundo só pelo próprio vidro). Cada área diz "Segue a geral" até você mexer; **Usar a geral** volta só aquela área. Trocar entre Geral e Por área não apaga nada.
- No **Geral**, o mesmo valor vale para as quatro áreas: o painel de detalhes fica de vidro e o fundo da conversa ganha uma placa de 100% menos o valor (com 100%, a conversa fica direto sobre o fundo, como antes). O valor que você já tinha vira o Geral, sem voltar ao padrão.
- **Desfoque** muda o fundo onde ele aparece direto: a conversa, entre os balões. A barra lateral e os balões continuam com o próprio vidro, que já desfoca o que está atrás.
- Os fundos da casa têm versão noturna, que entra sozinha no tema escuro. **Noite** só existe no escuro: no tema claro ele não aparece na lista e, se estiver escolhido, o app fica sem fundo até voltar ao escuro (Ajustes explica isso). No escuro, o véu padrão é mais leve (12%) para o fundo aparecer; as versões noturnas dos sete fundos antigos ainda são bem escuras.
- **Dez desenhos abstratos**: Cafeteria, Café, Campo, Lago, Horizonte urbano, Escritório, Ondas, Círculos, Blocos e Jardim. São ilustrações de formas simples e cores suaves, com várias opções bem claras. Cada desenho tem uma versão clara e outra escura, escolhida automaticamente pelo tema. Os fundos anteriores continuam disponíveis e sua escolha atual é preservada.
- **Imagem sua**: JPEG, PNG ou WebP. O app guarda uma cópia reduzida (até 2880 px no lado maior) no armazenamento dele; dá para remover a qualquer momento. As partes transparentes de um PNG ou WebP mostram a cor do tema por baixo. Se a imagem não abrir, o app diz isso em português e mantém a imagem anterior. Ao abrir o app, ele espera a sua imagem (no máximo um instante) para a janela já aparecer com ela.

## Legibilidade primeiro

- O app lê o fundo inteiro e não deixa o vidro ficar tão claro que o texto secundário caia abaixo do contraste mínimo (4.5:1), em qualquer tamanho de janela e parte da imagem. Quando isso segura a sua escolha, a Transparência (ou a área) mostra o valor em uso e explica: "Limitado a 9% neste fundo para o texto continuar legível." A garantia é conservadora: em fotos com partes muito escuras e muito claras o vidro fica quase opaco; **Suavizar fundo** aproxima as cores e libera mais transparência.
- **Reduzir transparência** no Mac deixa todo vidro opaco, também na prévia de Ajustes. **Aumentar contraste** sobe o vidro e as linhas.
- **Reduzir movimento** no Mac tira as animações (troca de fundo e escala ao pressionar).

## Ainda não

- Fotos HEIC (o padrão do iPhone e do Mac) ainda não abrem: exporte como JPEG antes de escolher.
- Fundo por space: o fundo escolhido vale para o app inteiro; uma escolha diferente por space ainda não existe.
- Os menus nativos do macOS e a cor da janela antes de carregar seguem o tema do Mac, mesmo com Claro ou Escuro escolhido no app.

## Toques Mexerica

O interruptor em **Ajustes > Aparência** liga ou desliga cinco detalhes:
calor discreto nas sombras e no foco do compositor, casca em listas vazias de
Rotinas e nos vazios selecionados de Extensões/Skills, fiapos ao redor do arco de
carregamento, gomo no progresso de tarefas (como o download de atualização) e
folha do símbolo junto ao ajuste, que acena no hover ou ao entrar pelo teclado.
Uso, armazenamento e medidores de cota conservam sua barra e altura.

A preferência vem ligada, fica neste computador e acompanha as outras janelas.
Desligar restaura os indicadores e ícones comuns, sem mudar ações ou estados.
O arco continua girando enquanto visível; janela oculta, região fora da tela ou
contêiner oculto pausam a atividade. Movimento reduzido elimina o giro e os gestos.
Contraste ampliado e cores forçadas usam os controles sem decoração. O gomo
segue o valor confirmado, inclusive em RTL, e não aparece no indeterminado.

O hover acompanha o destaque escolhido na faixa de ícones, na lista lateral,
nos botões de ícone neutros e nos itens de menu: fundo com 18% de accent,
borda interna de 1 px e texto/ícone num tom do accent ajustado a pelo menos
4,5:1. O item usa uma base da superfície do tema para manter essa garantia
sobre fundos transparentes. O restante da tela conserva sua transparência.
Desligar Toques restaura o hover neutro. O foco de teclado mantém seu anel
original, mais espesso e distinto da borda do hover.

Calor e desenhos seguem o destaque atual (`accent`). O laranja
padrão é tratado separadamente; esta mudança não cria uma cor nem troca uma preferência
salva. Erros e indisponibilidade não recebem casca. Não há sorteios, sons,
cheirinho ou cliques secretos nesta fase.
