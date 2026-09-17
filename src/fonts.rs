use std::borrow::Cow;

use gpui::App;

// Bundled so the PWA renders offline and needs no cross-origin font fetch
// (which COEP: require-corp would otherwise force us to CORS-enable).
pub fn load_fonts(cx: &App) -> bool {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(
            include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf").as_slice(),
        ),
        Cow::Borrowed(
            include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBold.ttf").as_slice(),
        ),
    ];
    if let Err(error) = cx.text_system().add_fonts(fonts.into()) {
        web_sys::console::error_1(&format!("failed to load fonts: {error:#}").into());
        return false;
    }
    true
}
