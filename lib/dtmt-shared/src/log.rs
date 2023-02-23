use tracing_subscriber::fmt::format::Writer;

fn format_time(w: &mut Writer) -> std::fmt::Result {
    let time = now_local();
    write!(w, "");
}
