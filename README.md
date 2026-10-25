# IACodeX

Aplicação desktop experimental em **Rust + Slint**, inspirada no estilo visual do DeepSeek Harness.

## Estrutura

```text
IACodeX/
├── assets/
│   └── images/           # Recursos PNG para a interface
├── src/
│   └── main.rs           # Inicialização e callbacks Rust
├── ui/
│   └── app.slint         # Layout, componentes e interações visuais
├── Cargo.toml            # Dependências e configuração do pacote
├── Cargo.lock            # Versões resolvidas das dependências
├── build.rs              # Compilação da interface Slint
├── rust-toolchain.toml   # Toolchain Rust fixada
├── .gitignore
├── LICENSE
└── README.md
```

As imagens em `assets/images/` estão preservadas para uso futuro. Ao usá-las em `ui/app.slint`, referencie-as com `@image-url("../assets/images/nome-do-arquivo.png")`.

## Executar no Fedora

```bash
sudo dnf install gcc pkgconf-pkg-config fontconfig-devel libxkbcommon-devel wayland-devel
rustup toolchain install 1.99.1
cargo run
```

## Estado atual

A interface é um protótipo. A janela Settings e os controles de modelos/plugins ainda não implementam persistência, credenciais de API ou conexão com provedores. O envio de prompts apenas imprime o texto no terminal.

A compilação deve ser validada no Fedora com `cargo check` ou `cargo run`.
