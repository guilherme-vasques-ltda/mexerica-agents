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
recusadas. O pacote leva o helper de fala, mas a janela não o executa
nem pede microfone.

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

A Open não instala nem procura o Codex no `PATH`. Use o executável nativo
do Codex CLI 0.164.0 já instalado. Sem esse executável, ou se o macOS não
conseguir iniciar o processo isolado, a Open informa que o Codex está
indisponível e não recorre a outro perfil.

Na raiz própria `~/.mexerica-open`, crie `codex-local.json` com permissão
0600. Informe `executable` como o caminho absoluto do binário real, sem
links; `version` como `0.164.0`; e `sha256` como o hash de 64 caracteres
minúsculos do executável confiável. A Open admite instalações em
`/opt/homebrew`, `/usr/local/bin`, `/Applications/Codex.app` e nas
pastas `.local/bin` e `.local/share` do usuário.

Em **Ajustes → Contas de IA**, adicione uma conta Codex e abra a página
oficial quando a janela mostrar o código de dispositivo. Sem concluir
a autorização no navegador, a conta não fica conectada. O Codex guarda
sua sessão em arquivo no perfil da Open; não copie o `auth.json` de
outra instalação nem compartilhe o arquivo criado.

### Conversar

Com a conta conectada, escreva na conversa principal de um bot, como o Berga
no Pessoal. Se houver mais de uma conta conectada, a Open usa a mais recente.
Sem conta conectada, a mensagem fica como não enviada, com o motivo ao lado;
conecte a conta em Ajustes → Contas de IA e use Reenviar.
Cada resposta roda o Codex no mesmo isolamento do login, com as ferramentas
desligadas: ele não executa comandos, não pesquisa na internet, não abre outros
programas e, fora do perfil da Open, só lê a política do administrador em
`/etc/codex` e o arquivo público de certificados do macOS. Se o Codex tentar
usar uma ferramenta, a resposta é encerrada com um aviso. Anexos e conversas
extras não estão disponíveis. Se existir `/etc/codex`, a pasta e seus arquivos
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
a sessão. Esses controles não aparecem em Ajustes.
