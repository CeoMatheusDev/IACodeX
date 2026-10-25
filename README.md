# IACodeX

Aplicativo desktop experimental de assistência à programação em **Rust + Slint**.

## Componentes

- Interface Slint: workspaces, sessões, configurações e modelos.
- Persistência JSON em `~/.config/iacodex/`: `settings.json`, `providers.json`, `workspaces.json`, `sessions.json`.
- Harness inicial em `src/agent/`: ciclo de resposta do LLM, eventos e cancelamento.
- Cliente HTTP em `src/llm/`: API OpenAI-compatível, streaming SSE e tratamento de erros.
- Runtime Tokio separado da thread Slint; respostas encaminhadas com `slint::invoke_from_event_loop`.

## Executar no Fedora

```bash
sudo dnf install gcc pkgconf-pkg-config fontconfig-devel libxkbcommon-devel wayland-devel
rustup toolchain install 1.99.1
export DEEPSEEK_API_KEY="SUA_CHAVE"
cargo check
cargo test
cargo run
```

Cadastre um provedor DeepSeek em `providers.json`, incluindo um modelo com ID `deepseek-chat`. Na interface, selecione o workspace, clique em **New Session**, selecione o modelo e envie a mensagem.

A variável `DEEPSEEK_API_KEY` tem prioridade sobre `api_key` no `providers.json`. Armazenar chaves em JSON é menos seguro. **TODO:** integrar Secret Service/keyring.

## Estado e limitações

A primeira integração com o LLM oferece streaming de texto, botão Stop e teste de loop com cliente simulado. **Ainda não há ferramentas, execução de shell, políticas de permissões efetivas, nem persistência do conteúdo completo do chat.** As sessões em JSON armazenam títulos. A instância do agente é recriada a cada envio, portanto o histórico em memória do AgentLoop ainda não é compartilhado entre mensagens da UI.

A compilação e os testes precisam ser validados localmente no Fedora com `cargo check && cargo test`.
