# Mexerica Open — teste local no macOS

No diretório `apps/desktop`, use Node 24, `npm ci` e
`npm run dist:mac:open`. O pacote usa o Xcode, o SDK do Mac e a versão do Rust
fixada em `rust-toolchain.toml` para compilar os binários da arquitetura local.
A saída é `dist/open/mac-arm64/Mexerica Open.app` no Apple Silicon e
`dist/open/mac/Mexerica Open.app` no Intel. É um pacote local com assinatura
ad hoc, sem instalador, download ou atualizações.

Abra o `.app` pelo Finder ou com `open`, sem mover para Aplicativos.
Confirme que a janela apareceu: em um teste num Mac limpo, a primeira
tentativa não iniciou o app e não mostrou erro; duas aberturas posteriores
funcionaram. Não execute o binário dentro do pacote para tentar abrir de novo:
isso deixou um segundo processo sem janela no teste.

A janela abre sem conta da Mexerica e inicia o Hub incluído no pacote. O Hub
encerra com o app e não instala um serviço do macOS nem se conecta a um relay.
A Open conecta uma conta do Codex e conversa só por texto na conversa
principal de cada bot. A Open não usa contas, executores ou credenciais
de outros perfis. WhatsApp e a seleção de pastas externas
continuam indisponíveis. O Hub só aceita as chamadas aprovadas para a
Open, como ler o estado e editar bots e Spaces locais; as demais são
recusadas. O ditado e a voz no próprio Mac usam o helper de fala.
O macOS pede permissão ao usar o microfone ou o reconhecimento de fala.

O macOS identifica este pacote por `app.mexerica.open`; ele declara apenas o
esquema `mexerica-open:`. O Hub reserva a porta 19524 na interface local,
sem expor a API por TCP. A janela usa um socket Unix privado do processo
que ela iniciou. O perfil da janela fica em
`~/Library/Application Support/Mexerica Open`; o Hub roda com uma pasta
pessoal própria em `~/.mexerica-open/home`, sem importar configurações ou
credenciais de outros apps. O cofre da Open reserva o serviço de Chaveiro
`app.mexerica.open.credentials`, mas está desativado nesta versão. A janela
não usa Safe Storage.

### Conectar o Codex

Em **Ajustes → Contas de IA**, adicione uma conta Codex. Se ainda não
houver uma configuração, a Open pede autorização para preparar o Codex
CLI 0.164.0. Ela procura uma cópia oficial em caminhos conhecidos ou
baixa o pacote oficial para a arquitetura do app. A Open confere as
impressões SHA-256 do binário e, no download, também do pacote. Ela grava
a cópia em uma pasta privada `~/.local/share/mexerica-open-codex-*` e a
seleção em `~/.mexerica-open/codex-local.json`, sem alterar o Codex
existente. Uma configuração inválida existente não é sobrescrita.

Abra a página oficial que o app mostra e confirme o código do dispositivo.
Sem concluir a autorização no navegador, a conta não fica conectada.
O Codex guarda a nova sessão no perfil da Open; não copie o `auth.json`
de outra instalação nem compartilhe o arquivo criado.

Depois de conectar, o app oferece os modelos confirmados para essa conta,
com o padrão do Codex já marcado. Confirmar aplica a escolha ao Berga e aos
bots Codex sem motor configurado, sem mudar escolhas existentes.

### Consultar conectores

Em **Extensões → Conectores**, o dono no computador local pode pesquisar
serviços e abrir suas fichas. O catálogo é de consulta: a Open não conecta
serviços aos bots nem mostra contas ou credenciais.

### Conversar

Com a conta conectada, escreva na conversa principal de um bot, como o Berga
no Pessoal. O seletor de modelo na conversa permite trocar a conta e o modelo
do bot. Sem uma conta escolhida, a Open prefere uma conta conectada que
não esteja em reconexão e que aceite o modelo do bot. Se não houver modelo
escolhido, usa o padrão da conta.
Sem conta conectada, a mensagem fica como não enviada, com o motivo ao lado;
conecte a conta em Ajustes → Contas de IA e use Reenviar.
Cada resposta roda o Codex no mesmo isolamento do login. Ele pode pesquisar no
índice do provedor, quando disponível para o modelo e a conta, sem abrir páginas
no computador. Para pesquisar, o Codex pode enviar as mensagens mais recentes e
trechos das respostas ao serviço de busca do mesmo provedor. Os resultados podem
estar desatualizados e vir de fontes não confiáveis. O Codex não executa comandos
nem abre outros programas e, fora do perfil da Open, só lê a política do
administrador em `/etc/codex` e o arquivo público de certificados do macOS. Se
tentar usar uma ferramenta não permitida, a
resposta é encerrada com um aviso. Geração de imagens, anexos e conversas extras
não estão disponíveis. Se existir `/etc/codex`, a pasta e seus arquivos
precisam ter o `root` como dono e não podem aceitar escrita do grupo nem de
outros usuários; caso contrário, a Open não abre o Codex. Para a conexão
segura, o Codex isolado usa os certificados públicos do macOS em
`/etc/ssl/cert.pem`, sem consultar o Chaveiro, e não lê o arquivo `.env` do
perfil, que poderia trocar esse caminho. A Open só abre o Codex se esse arquivo
e as pastas acima dele forem do `root` sem escrita do grupo nem de outros; a
conferência olha as permissões comuns (não as listas de acesso) e não impede
uma troca feita depois dela pelo próprio administrador. Certificados instalados
à parte, como os de uma rede corporativa, não valem na Open.

Em ambiente de testes Unix sem o app aberto, volte à raiz do repositório e use
`(cd host && CODYNC_ENV=open CARGO_PROFILE_DEV_DEBUG=0 cargo test --test open_hub)`
para testar o Hub isolado.

Encerre pela janela ou com Command+Q. Em caso de saída forçada, o cliente
pode recusar a reabertura se encontrar arquivos de trava antigos; não use
outro perfil para contornar a recusa. Para limpar o teste, feche o app e
apague só as duas pastas da Open e o próprio pacote. O macOS pode manter
metadados próprios de `app.mexerica.open`, como preferências, caches, estado
da janela ou o registro do esquema; eles não pertencem aos outros apps.
A separação não isola usuários do macOS nem torna seguro executar código
não revisado.

A edição Open não muda o ícone, a presença no Dock ou a abertura ao iniciar
a sessão. Esses controles não aparecem em Ajustes. Em **Extensões →
Conectores**, o catálogo local é só para consulta; não acessa o cofre nem
permite conectar ou gerenciar serviços.
