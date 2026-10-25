slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;
    app.on_send_prompt(|message| {
        println!("Prompt (interface demonstrativa): {}", message);
    });
    app.run()
}
