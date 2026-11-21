# Menus de sessões

Na aba de uma sessão, use botão direito para **Copiar ID**, **Copiar nome da
sessão** ou **Arquivar**. Na lista lateral de Sessões, os itens são **Copiar ID**,
**Copiar nome**, **Arquivar** e **Excluir**. Também funciona na principal para
copiar; arquivamento só está disponível nas extras abertas quando o Host permite.

Com o foco na aba ou linha, Shift+F10 ou a tecla de menu abre as mesmas opções.
Setas, Home e End navegam; Enter ativa; Escape fecha e devolve o foco.
Os menus e a confirmação acompanham os seis idiomas, inclusive árabe (RTL).

Arquivar mantém a confirmação existente e usa `sessionClose`, com a revisão
lida do catálogo. A mudança aparece quando o Host confirma. A sessão continua
visível em Arquivadas. Trocar Space, bot, conexão ou época do Host invalida menus
e confirmações antigos; nenhuma ação usa o Space que passou a estar selecionado.

**Entrega parcial:** o Host atual não oferece exclusão individual de sessão.
Excluir fica indisponível e informa isso no menu. Não se
usa `deleteBot`, que excluiria o bot inteiro.

O `+` de nova sessão fica junto ao chip do bot, mesmo com as abas
recolhidas, quando o Host anuncia criar e enviar na sessão.
