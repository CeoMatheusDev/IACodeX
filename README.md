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
cargo check
cargo test
cargo run
```

Na interface, abra **Settings > Providers**, selecione **DeepSeek** (ou crie um provedor OpenAI-compatível), informe a **API key**, confirme o modelo `deepseek-chat` e clique em **Apply**. Use **Fetch available models** para consultar o endpoint OpenAI-compatível `/models` com a URL e a chave informadas. Clique em um modelo retornado para selecioná-lo; a lista é temporária e o modelo escolhido é salvo ao aplicar. O cartão do provedor exibe um indicador: verde quando a última consulta funcionou, vermelho quando falhou e amarelo quando ainda não foi verificado. Use **Edit** para alterar a configuração; deixe a chave vazia para manter a anterior. Depois selecione um workspace, abra **New Session**, escolha o modelo no chat e envie a mensagem. Não é necessário configurar variáveis de ambiente.

A chave configurada na interface é salva em `~/.config/iacodex/providers.json` e tem prioridade sobre a variável opcional `DEEPSEEK_API_KEY`. No Unix, novos arquivos JSON são gravados com permissões `0600`. **A chave permanece em texto simples:** não compartilhe o arquivo. **TODO:** integrar Secret Service/keyring.

## Estado e limitações

A primeira integração com o LLM oferece streaming de texto, botão Stop e teste de loop com cliente simulado. **Ainda não há ferramentas, execução de shell, políticas de permissões efetivas, nem persistência do conteúdo completo do chat.** As sessões em JSON armazenam títulos. A instância do agente é recriada a cada envio, portanto o histórico em memória do AgentLoop ainda não é compartilhado entre mensagens da UI.

A compilação e os testes precisam ser validados localmente no Fedora com `cargo check && cargo test`.
