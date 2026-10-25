# IACodeX

Interface desktop nativa inspirada no DeepSeek Harness, feita em **Rust + Slint**.

## Fedora

```bash
sudo dnf install gcc pkgconf-pkg-config fontconfig-devel libxkbcommon-devel wayland-devel
rustup toolchain install 1.99.1
cargo run --release
```

O arquivo `rust-toolchain.toml` fixa Rust 1.99.1. Dependências Rust são obtidas pelo Cargo na primeira compilação.

## Organização

- `src/main.rs` — inicialização e callbacks Rust
- `ui/app.slint` — layout, estilos e interações visuais
- `build.rs` — compilação do Slint

## Escopo da fase 1

Protótipo visual com navegação demonstrativa. Não há armazenamento de sessões, chamadas de IA, acesso real a arquivos, agentes ou terminal. O botão de envio imprime o prompt no terminal.

## Observação

Este pacote foi gerado sem executar `cargo check` no ambiente de destino. Podem ser necessários ajustes de API conforme a versão do Slint resolvida pelo Cargo.
