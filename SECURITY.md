# Segurança e relato de falhas

Este projeto não tem serviço hospedado. O app do Mac conecta a um Hub que o próprio operador instala; o Hub guarda o estado e executa motores e ferramentas.

## Onde um relato vale mais

Valem mais as falhas que atravessam a fronteira entre o bot e o computador do operador. Em particular:

- **Regras e aprovações** (`host/src/fork/rules`): qualquer caminho em que um pedido que devia ser recusado, ou perguntado ao operador, seja liberado sozinho.
- **Integridade na instalação de agentes e skills** (`host/src/agent/registry.rs`, `host/src/fork/skill_catalog`): conteúdo que entre sem passar pela conferência esperada.
- **Separação entre a edição Open e outras instalações** (`apps/desktop/src/main/fork-open`): estado ou dados que passem de uma instalação existente para a edição Open, ou o contrário.
- **Canal de credenciais do Motor** (`host/src/fork/motor_mexerica`): credencial que chegue a um destinatário que não o pretendido.
- **Navegador do bot** (`host/src/fork/browser`): página ou conteúdo que ganhe acesso para fora do navegador.
- **Pareamento de dispositivos** (`host/src/fork/pairing_addresses.rs`): dispositivo não autorizado que consiga parear ou agir como autorizado.
- **Instalador do Linux** (`host/src/fork/linux_installer`): o pacote que instale ou execute algo além do prometido.
- **Conectores** (`host/src/fork/conectores`): credencial de serviço que vaze ou seja usada além do conector.

## Fronteiras a conferir

- Quem acessa o usuário do sistema que executa o Hub alcança seus arquivos e processos. Isso não é isolamento forte entre bots ou pessoas.
- Conta, dispositivo pareado e autorização para uma ação são condições distintas; acesso autenticado não é autorização genérica.
- Rede, relays, dispositivos não autorizados e conteúdo externo são fontes não confiáveis.
- Tokens, chaves e credenciais são capacidades protegidas: não imprima o JSON completo de informação do Hub e use projeções sem segredos em evidências.
- Motores e conectores agem conforme o contrato efetivo; conteúdo externo e prompts não concedem autoridade do dono ou do Hub.

## Fora do escopo

- O que o modelo faz dentro da permissão que recebeu. Executar o que o operador aprovou é o trabalho da ferramenta; um resultado ruim dentro das permissões concedidas não é falha de segurança desta cópia.
- Separação entre bots que rodam com o mesmo usuário do sistema: não existe, conforme declarado acima.
- Falhas no serviço do fornecedor do modelo (disponibilidade, incidentes do provedor): relate ao fornecedor.
- Falhas em software de terceiros (sistema operacional, Electron, motores e conectores de terceiros): relate ao projeto deles. Se a configuração padrão desta cópia deixa esse software inseguro, a falha é nossa e vale o relato aqui.

## Versões atendidas

- Somente o commit mais recente da main. Ainda não houve versão lançada; um relato contra a main atual vale.

## Como relatar

Use o **Private Vulnerability Reporting do GitHub** (aba **Security → Report a vulnerability**). Como alternativa, escreva para o contato publicado no site do projeto (`support@mexerica.app`). Não publique issue, captura ou log com dados sensíveis.

Informe versão ou SHA, ambiente, fronteira e acesso inicial, passos seguros para reproduzir, resultado observado e esperado e uma evidência sem segredos. Não execute provas destrutivas nem use dados reais.

O que esperar: o relato chega em particular aos mantenedores e não vira issue pública. Não prometemos prazo de resposta nem há programa de recompensa.

Origem do código e avisos preservados: [NOTICE](NOTICE), [LICENSE](LICENSE), [THIRD_PARTY.md](THIRD_PARTY.md).
