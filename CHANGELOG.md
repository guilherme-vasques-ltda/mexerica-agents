# Registro de mudanças

O que de fato entrou na main deste repositório. Ainda não houve versão lançada: enquanto não houver, as entradas ficam na seção «Não lançado», sem número de versão nem data. O registro é atualizado na hora de fechar cada versão, a partir dos PRs que entraram; PR individual não edita este arquivo.

## Não lançado

Ponto de partida: a importação inicial desta cópia (hub em Rust, motor e app de desktop, com a documentação própria). Depois dela, entraram, do mais antigo para o mais novo dentro de cada grupo:

### Mudanças que quebram

- Nenhuma até aqui.

### Segurança

- A verificação automática do repositório passou a varrer cada commit do PR em busca de segredos e a exigir a lista de segredos também nas dependências (#6).
- A edição Open do app de desktop ganhou identidade própria, separada das instalações existentes (#1).
- Na edição Open, as rotas fora do Hub isolado ficam fechadas e a API local aceita só os métodos suportados (#10).

### Novidades

- Edição Open do app de desktop: sem nuvem e sem conta conectada, com os textos traduzidos e reconhecimento de fala exigido no aparelho (#1).
- O Hub do macOS inicia em um perfil isolado na edição Open (#10).
- Regras do repositório para construir, testar e contribuir (`AGENTS.md`) (#7).

### Consertos

- Linux: o pacote `.deb` declara a dependência do ALSA e pode ser gerado com mantenedor e página da Mexerica (#9).
- Créditos e avisos conferidos: descrições copiadas trocadas por texto próprio, logos dos serviços restauradas com condição de uso e aviso de modificação acrescentado aos arquivos alterados em relação à origem (#2).
- Na edição Open, ajustes nativos indisponíveis ficam escondidos (#19).
- A logo da Firecrawl ficou visível no fundo claro (#17).
- Windows: o catálogo de skills deixou de falhar ao iniciar o Hub; a troca de pastas do catálogo roda em passos, com reconferência e limpeza que preserva a cópia anterior; caminhos longos saem do limite de 260 caracteres; o desinstalador chama o executável certo (#11).
- Edição de bots e leitura de eventos corrigidas na edição Open; o canal do motor compila no macOS (#10).
