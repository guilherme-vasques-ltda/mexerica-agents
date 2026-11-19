# Prompt pra colar no Claude Code / Codex

Coloque a pasta `donnabot-avatars` dentro do repo da Mexerica e cole o texto abaixo.

---

Quero integrar os avatares animados que estão em `donnabot-avatars/` (leia o README.md dessa pasta primeiro).

Objetivo:
1. **Modelo de dados.** Adicionar `avatar_config` (JSON, formato em `donnabot-avatars/schema/avatar-config.schema.json`) em cada bot. Fazer uma migration e, nos bots que já existem, preencher com um preset de `presets/roles.json` que combine com a função, ou com `DEFAULT_CONFIG`.
2. **Sidebar / lista de bots.** Trocar o avatar atual por `<BotFace config={bot.avatarConfig} size={44} mood={mood} />`.
3. **Tela Perfil → Avatar.** Trocar o seletor atual pelo `<AvatarEditor value onChange />`, salvando a cada mudança (com debounce de ~400 ms).
4. **Estado ao vivo.** Criar um hook ou store por bot com `useBotMood` e ligar aos eventos do runtime:
   - usuário digitando → `setBase('listen')`
   - modelo gerando → `setBase('think')`
   - chamada de ferramenta/MCP em andamento → `setBase('work')`
   - resposta concluída → `setBase('idle')` + `play('happy')`
   - erro → `play('surprise')`
   - nova mensagem recebida → `play('blink')`
   - bot desativado → `setBase('sleep')`; reativado → `play('wake')`
5. **Criar bot por função.** No fluxo de "novo bot", mostrar a aba Função do editor (os 30 presets) e usar `configFromRole(preset)` + `preset.name` como nome sugerido.

Regras:
- Não alterar os arquivos dentro de `donnabot-avatars/src` (trate como biblioteca). Se precisar de algo, me pergunte.
- Manter acessibilidade: passar `label` com nome e função do bot.
- Em listas grandes, `autoLife={false}` para avatares fora da tela.
- Rodar typecheck e mostrar o resultado no fim.

Antes de codar, me mostre o plano: onde fica o modelo do bot, qual componente renderiza a sidebar e de onde vêm os eventos do runtime.
