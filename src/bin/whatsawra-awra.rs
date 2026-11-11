//! WhatsAwra drawn with Awra.

fn main() {
    let app = awra::Application::new("rocks.whatsawra.WhatsAwra");
    let window = awra::Window::offscreen("WhatsAwra", 800, 600, 1.0);
    let label = awra::Label::new(Some("Hello from Awra"));
    window.set_child(Some(&label));
    window.present();
    for _ in 0..20 {
        awra::pump();
    }
    window.snapshot_png("awra-hello.png");
    drop(app);
}
