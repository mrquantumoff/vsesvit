use adw::prelude::*;

use crate::APP_ID;

pub(crate) fn present(parent: &impl IsA<gtk::Widget>) {
    let has_app_icon = gtk::IconTheme::for_display(&parent.display()).has_icon(APP_ID);
    let dialog = adw::AboutDialog::builder()
        .application_name("Vsesvit")
        .application_icon(if has_app_icon {
            APP_ID
        } else {
            "web-browser-symbolic"
        })
        .version(env!("CARGO_PKG_VERSION"))
        .comments("A web browser for Linux and Windows")
        .debug_info(debug_info())
        .build();
    dialog.present(Some(parent));
}

fn debug_info() -> String {
    format!(
        "Vsesvit {}\nGTK {}.{}.{}\nlibadwaita {}.{}.{}\nWebKitGTK {}.{}.{}\n",
        env!("CARGO_PKG_VERSION"),
        gtk::major_version(),
        gtk::minor_version(),
        gtk::micro_version(),
        adw::major_version(),
        adw::minor_version(),
        adw::micro_version(),
        webkit::functions::major_version(),
        webkit::functions::minor_version(),
        webkit::functions::micro_version(),
    )
}
