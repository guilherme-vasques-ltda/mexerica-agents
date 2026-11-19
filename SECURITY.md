# Segurança e relato de falhas

Este projeto não tem serviço hospedado. O app do Mac conecta a um Hub que o próprio operador instala; o Hub guarda o estado e executa motores e ferramentas.

## Fronteiras a conferir

- Quem acessa o usuário do sistema que executa o Hub alcança seus arquivos e processos. Isso não é isolamento forte entre bots ou pessoas.
- Conta, dispositivo pareado e autorização para uma ação são condições distintas; acesso autenticado não é autorização genérica.
- Rede, relays, dispositivos não autorizados e conteúdo externo são fontes não confiáveis.
- Tokens, chaves e credenciais são capacidades protegidas: não imprima o JSON completo de informação do Hub e use projeções sem segredos em evidências.
- Motores e conectores agem conforme o contrato efetivo; conteúdo externo e prompts não concedem autoridade do dono ou do Hub.

## Como relatar

Use o **Private Vulnerability Reporting do GitHub** (aba **Security → Report a vulnerability**), quando estiver habilitado neste repositório. Não publique issue, captura ou log com dados sensíveis.

Informe versão ou SHA, ambiente, fronteira e acesso inicial, passos seguros para reproduzir, resultado observado e esperado e uma evidência sem segredos. Não execute provas destrutivas nem use dados reais.

Origem do código e avisos preservados: [NOTICE](NOTICE), [LICENSE](LICENSE), [THIRD_PARTY.md](THIRD_PARTY.md).
